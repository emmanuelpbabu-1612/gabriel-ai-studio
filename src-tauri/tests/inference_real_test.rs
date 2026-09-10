#![cfg(feature = "candle-cuda")]

use std::collections::HashSet;

use tokio::sync::mpsc;

use gabriel_lib::inference::candle_backend::CandleTextBackend;
use gabriel_lib::inference::TextBackend;
use gabriel_lib::types::{ChatEvent, GenParams};

const MODEL_ID: &str = "qwen2.5-3b-instruct-test";

async fn drain(rx: mpsc::Receiver<ChatEvent>) -> (String, Option<String>) {
    let mut text = String::new();
    let mut failure = None;
    let mut rx = rx;
    while let Some(ev) = rx.recv().await {
        match ev {
            ChatEvent::Token(t) => text.push_str(&t),
            ChatEvent::Done { .. } => break,
            ChatEvent::Failed(m) => {
                failure = Some(m);
                break;
            }
        }
    }
    (text, failure)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires an NVIDIA GPU + network access; run with `cargo test --features candle-cuda -- --ignored`"]
async fn qwen_real_inference_lifecycle() {
    let backend = CandleTextBackend::load(MODEL_ID.to_string())
        .await
        .expect("model load must succeed");

    // Measured, not estimated: cudarc mem_get_info delta around weight load.
    let measured = backend
        .measured_vram_bytes()
        .expect("candle backend must report measured VRAM");
    assert!(
        measured > 512 * 1024 * 1024,
        "a Q4_K_M 3B model must occupy well over 512 MiB, got {measured} bytes"
    );
    println!("measured VRAM after load: {measured} bytes ({:.2} GiB)", measured as f64 / (1024.0 * 1024.0 * 1024.0));

    // Multiple sequential streams over one resident model (registry semantics).
    for (turn, prompt) in [
        "Write one short sentence about the sea.",
        "Name one primary color.",
        "Say hello in exactly three words.",
    ]
    .into_iter()
    .enumerate()
    {
        let (tx, rx) = mpsc::channel(256);
        let generated = backend
            .stream_tokens(
                prompt,
                GenParams {
                    max_tokens: 64,
                    temperature: 0.6,
                },
                tx,
            )
            .await
            .unwrap_or_else(|e| panic!("turn {turn}: streaming failed: {e}"));

        let (text, failure) = drain(rx).await;
        assert!(failure.is_none(), "turn {turn}: unexpected failure {failure:?}");
        assert!(generated >= 3, "turn {turn}: too few tokens decoded: {generated}");
        assert!(
            text.chars().count() >= 8,
            "turn {turn}: decoded output suspiciously short: {text:?}"
        );

        let unique_chars = text.chars().collect::<HashSet<_>>().len();
        assert!(
            unique_chars > 6,
            "turn {turn}: output looks degenerate/repeating: {text:?}"
        );
        assert!(
            text.split_whitespace().count() >= 2,
            "turn {turn}: expected multi-word prose, got: {text:?}"
        );
        println!("turn {turn}: {generated} tokens -> {text:?}");
    }
}
