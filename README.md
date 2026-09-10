# Gabriel

A native, zero-crash, multi-modal **local AI engine** written in Rust. Gabriel runs
OpenAI-compatible inference entirely on your machine — no cloud, no telemetry — with a
hardware-aware resource governor that makes CUDA out-of-memory crashes structurally
impossible.

It is designed as the backend for a Tauri v2 + React desktop app, but the HTTP surface is
standard OpenAI REST, so any OpenAI SDK, LangChain pipeline, or `curl` can drive it.

---

## What it does

| Capability | Detail |
|---|---|
| LLM chat streaming | `POST /v1/chat/completions` with SSE token streaming (OpenAI chunk protocol incl. `[DONE]`) or buffered JSON |
| Image generation | `POST /v1/images/generations`, task-queued, base64 payload responses |
| Speech synthesis | `POST /v1/audio/speech`, streams a valid 22 kHz 16-bit PCM WAV |
| Model registry | `GET /v1/models` reports every loaded model with residency and VRAM status |
| VRAM pager | Automatically demotes idle models GPU -> RAM when usage crosses a high watermark; promotes on demand |
| Memory-bandwidth governor | Smooths NVML memory-controller utilization and inserts backoff into background diffusion steps when the bus runs hot; interactive chat is never throttled |
| Priority scheduler | Interactive jobs (priority `-1`) strictly preempt background diffusion jobs (priority `0`) via biased channel select |
| Slot pool | Count-based `max_loaded_models` cap with deterministic LRU eviction — saturated pools reject instead of thrashing residents (inspired by Lemonade's residency model) |
| Telemetry | Live GPU name/utilisation, VRAM budget ledger, RAM, CPU, per-model idle times |
| Tauri IPC | Strongly typed commands: `load_model`, `unload_model`, `offload_model`, `get_telemetry`, `list_loaded_models` |

### The zero-crash guarantee

Every model carries a VRAM budget. Before anything touches the GPU, the memory pager:

1. Reads real utilisation from NVML (Linux/Windows) or Metal (macOS).
2. Refuses admission if the projected footprint would cross the high watermark
   (default **85 %** of total VRAM) — returning a typed `VramExhausted` error instead of
   letting CUDA OOM kill the process.
3. Evicts least-recently-used idle models to CPU when pressure builds, restoring them
   transparently on the next request.

The engine degrades gracefully: if no GPU monitor exists, admission control is skipped
and the engine keeps running in software mode.

### Memory-bandwidth sharing

VRAM capacity is only half of the story: concurrent workloads also fight over the
memory bus, which manifests as wall-clock throttling even when capacity is fine. The
`BandwidthGovernor` (`core/bandwidth.rs`) closes that gap:

1. A sampler task feeds NVML's memory-controller utilization (`utilization_rates().memory`)
   into an exponential moving average every pager tick, so single spikes cannot trigger
   backoff.
2. Utilization above the configurable ceiling (default **80 %**) produces a normalized
   `pressure` value (0..=1), exposed as `bandwidth_pressure` in `get_telemetry`.
3. Background diffusion consults `standard_yield()` between denoising steps and sleeps
   proportionally (up to `max_bandwidth_yield`, default 250 ms per step).
4. **Interactive traffic never consults the governor** — chat token streams are exempt
   by construction, not by best effort. This is enforced in tests.

Slot pools follow the same philosophy as [Lemonade](https://github.com/lemonade-sdk/lemonade)'s
residency manager: at most `max_loaded_models` registrations exist at once, a saturated
pool deterministically rejects newcomers with `slot_pool_exhausted` rather than evicting
warm residents, and only genuinely idle LRU entries are evicted to make room.

## Architecture

```
React UI (Tauri WebView)
        │  invoke()                          ▲ telemetry events
        ▼                                     │
┌────────────────────────── src-tauri ────────┴─────────────────────────┐
│  ipc/commands.rs          load_model / unload_model / offload_model   │
│                           get_telemetry / list_loaded_models          │
│                                      │                                │
│                               core/engine.rs  ◄── EngineState (Arc)   │
│                            ┌──────────┼────────────┐                  │
│                 core/scheduler.rs  core/pager.rs  core/registry.rs    │
│                 interactive lane  VRAM budget +   residency table     │
│                 (biased select)   LRU offloading                      │
│                            └──────────┬────────────┘                  │
│                    inference/ (TextBackend · ImageBackend ·           │
│                               SpeechBackend traits → factory)         │
│                                      │                                │
│  api/  axum @ 127.0.0.1:8080 ────────┘                                │
│  /v1/chat/completions (SSE)  /v1/images/generations                   │
│  /v1/audio/speech            /v1/models       /health                 │
└───────────────────────────────────────────────────────────────────────┘
```

### Module map

```
src-tauri/src/
├── lib.rs               Tauri builder wiring: manage state, spawn server, register commands
├── error.rs             GabrielError — one enum, both IPC (serde) and HTTP (status mapping)
├── types/
│   ├── openai.rs        Request/response DTOs mirroring the OpenAI wire format
│   ├── ipc.rs           ModelSpec, ModelStatus, Residency, TelemetrySnapshot
│   └── jobs.rs          Job envelope, Priority, ChatEvent, GenParams
├── core/
│   ├── engine.rs        EngineState: orchestration, admission, job execution
│   ├── scheduler.rs     Two-lane mpsc queues + dispatcher (tokio::select! biased)
│   ├── pager.rs         MemoryPager: admit(), maintenance pass, VRAM ledger
│   └── registry.rs      Model residency table with LRU tracking
├── inference/
│   ├── mod.rs           Backend traits + BackendFactory
│   ├── stub.rs          Deterministic stub engines used by tests/dev
│   ├── audio.rs         WAV encoder + dimension validation
│   ├── image_codec.rs   BMP encoder, HSV gradient renderer
│   └── candle_backend.rs Real-weight integration point (feature `candle-cuda`)
├── api/
│   ├── router.rs        Axum routing table
│   └── routes/          chat.rs (SSE) · images.rs · speech.rs · models_list.rs · health.rs
├── telemetry/
│   ├── gpu.rs           NVML / Metal / fallback monitors behind one trait
│   └── sys.rs           sysinfo-based RAM/CPU sampler
└── ipc/commands.rs      #[tauri::command] handlers
```

## API reference

All endpoints accept and return OpenAI-compatible JSON. Errors use the standard
`{"error": {"message", "type", "code"}}` envelope with correct HTTP status codes
(404 unknown model, 409 double-load, 413 oversized body, 507 VRAM exhausted...).

```bash
# chat (streaming)
curl -N http://127.0.0.1:8080/v1/chat/completions \
  -H 'Content-Type: application/json' \
  -d '{"model":"m","messages":[{"role":"user","content":"hi"}],"stream":true}'

# images
curl http://127.0.0.1:8080/v1/images/generations \
  -H 'Content-Type: application/json' \
  -d '{"prompt":"a neon skyline","size":"512x512"}'

# speech (returns WAV bytes)
curl http://127.0.0.1:8080/v1/audio/speech \
  -H 'Content-Type: application/json' \
  -d '{"model":"tts","input":"Gabriel online","voice":"nova"}' -o out.wav

# registry
curl http://127.0.0.1:8080/v1/models
```

## Building

### Prerequisites

- Rust 1.85+ (`rustup update stable`)
- Tauri v2 system dependencies:
  - Linux: `libwebkit2gtk-4.1-dev build-essential curl wget file libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev`
  - macOS: Xcode Command Line Tools
  - Windows: WebView2 + MSVC Build Tools
- Optional (real GPU inference): NVIDIA CUDA toolkit for the `candle-cuda` feature.
  Everything else compiles and runs without CUDA.

### Commands

```bash
git clone <repo> && cd Gabriel

cargo check              # fast type-check of the whole workspace
cargo run --example smoke_server
                         # boots the engine + REST API on 127.0.0.1:8080 with three stub models
cargo test               # full test suite (39 tests, ~20 s, no GPU required)
cargo clippy --all-targets   # must stay warning-free
cargo tauri dev          # full desktop shell (needs a frontend at ../dist or tauri.conf.json update)
cargo build --release    # optimized binary (LTO + strip enabled)
```

### Feature flags

| Feature | Default | Purpose |
|---|---|---|
| `nvml` | yes | NVML-backed VRAM/GPU telemetry (Linux, Windows) |
| `metal` | no | Metal-backed telemetry (macOS) |
| `candle-cuda` | no | Candle/CUDA weight loading path in `inference/candle_backend.rs` |

## Testing

The suite is split by concern so failures are immediately diagnosable:

| Suite | File | Covers |
|---|---|---|
| Functionality | `tests/api_test.rs` | SSE streaming, buffered chat, image b64 output, WAV output, registry |
| Stress | `tests/stress_test.rs` | 32-way concurrent SSE storm, mixed-modality load, 50 load/unload cycles, 100 sequential requests, interactive-priority preemption timing |
| Failure management | `tests/failure_test.rs` | Client disconnect mid-stream, unload during active stream, pager-offloaded model errors, oversized VRAM rejection + recovery, queue backpressure rejection, typed error slugs |
| Bandwidth & sharing | `tests/bandwidth_test.rs` | Governor EMA/backoff math, diffusion slowdown under bus pressure vs. unpressured baseline, interactive-lane immunity under saturation, slot-pool deterministic rejection + LRU eviction |
| Security | `tests/security_test.rs` | Malformed JSON, wrong content-type, >2 MB body limit, prompt/input length caps, parameter clamping, invalid dimensions, SSE JSON well-formedness |

```bash
cargo test                                  # everything
cargo test --test stress_test               # one category
cargo test --test security_test -- --nocapture
```

Bugs the suite has already caught and fixed:

- SSE streams never emitted the OpenAI `[DONE]` terminator (found by the 32-stream storm).
- Systems without a GPU monitor could never load a model (pager computed a 0-byte budget).
- Concurrent loads of the same model id could race past the registry check (now serialized
  by an internal gate).
- Admission control double-counted engine-resident bytes against the VRAM cap.

## Configuration

Defaults live in `core::EngineConfig` and can be tuned before constructing the engine:

```rust
EngineConfig {
    host: "127.0.0.1".into(),
    port: 8080,
    vram_high_watermark: 0.85,   // refuse/offload above this fraction of total VRAM
    vram_low_watermark: 0.70,    // pager evicts until usage drops below this
    idle_offload_after: 120 s,   // models idle this long are first to be paged out
    pager_poll_interval: 2 s,
    queue_capacity: 256,         // per scheduler lane
    max_concurrent_image_jobs: 1,
    auto_load_on_request: false, // if true, requests may transparently cold-start models
    bandwidth_ceiling_percent: 80.0, // memory-bus utilization where background jobs yield
    max_bandwidth_yield: 250 ms, // per-step pause at full pressure
    max_loaded_models: 4,        // registration slot pool (Lemonade-style residency cap)
}
```

## Contributing

Pull requests welcome. House rules that keep the engine predictable:

1. **Zero-warning policy** — `cargo clippy --all-targets` must be clean before review.
2. **No lock across `.await`** — parking-lot guards are dropped before awaiting; the
   existing code never holds one across a suspension point, keep it that way.
3. **New behaviour needs a failing test first** — add it to the matching suite above;
   cross-cutting failure modes belong in `failure_test.rs`.
4. **Errors are typed** — extend `GabrielError` rather than stringifying; every variant
   gets a slug and an HTTP status mapping in `error.rs`.
5. **Stub parity** — anything the stub engines demonstrate (streaming, WAV/BMP output)
   must keep working; they are the contract the test suite verifies against.

### Adding a real model backend

1. Implement `TextBackend` / `ImageBackend` / `SpeechBackend` from `inference/mod.rs`.
2. Register it in `BackendFactory::create` (see the feature-gated example in
   `inference/candle_backend.rs`, which already resolves the CUDA device through Candle).
3. Add a functionality test that loads your model id through
   `EngineState::load_model` and exercises the matching HTTP route.

### Debugging tips

- `RUST_LOG=debug cargo run --example smoke_server` shows every scheduler dequeue,
  pager eviction, and job completion.
- `/health` returns `200` once the Axum task is serving.
- `get_telemetry` (IPC) or `GET /v1/models` expose the full residency picture at runtime.
