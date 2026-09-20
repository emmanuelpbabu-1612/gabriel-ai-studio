# Gabriel – Local AI Studio

Gabriel is a native, zero-crash, multi-modal local AI desktop application. It acts as the premium graphical interface for the Gabriel Rust Engine, providing OpenAI-compatible inference entirely on your machine—with zero cloud dependencies and strict hardware-aware resource management.

## 🎨 UI/UX Design System: "Aurora Glass"
The frontend is built with a bespoke, hyper-premium design language inspired by modern macOS, visionOS, and Linear. 

- **Soft Matte Glass:** Extensive use of heavy background blurs (`blur-40px`, `saturate-150%`), sheer surfaces, and crisp light-catching edges.
- **Dynamic Aurora Motion:** Subtle, heavenly, slow-moving gradients trap ambient light inside containers, popups, and sidebars.
- **Typography:** Driven entirely by **DM Sans** for a clean, highly readable, and modern geometric hierarchy.
- **True Dual Themes:** 
  - *Light Mode:* Translucent off-white glass, soft gray borders, and pastel aurora motion.
  - *Dark Mode:* Deep charcoal base (`#0a0a0a`), vibrant accent colors, and sleek dark glass surfaces.

## ✨ Core Features
The frontend strictly maps to the verified capabilities of the Rust backend:

- **💬 Chat (LLMs):** Interactive SSE token streaming with priority `-1` scheduling to preempt background tasks.
- **🎨 Image Generation:** Background task-queued image diffusion with real-time grid previews.
- **🎙️ Text-to-Speech:** Natural speech synthesis generating 22 kHz 16-bit PCM WAV audio with a dynamic vertical-bar amplitude visualizer.
- **🧠 Advanced Model Management:**
  - **Load / Unload:** Instantly mount or destroy weights in VRAM.
  - **Offload:** Force idle models from VRAM into System RAM via the backend `MemoryPager`.
- **⚙️ Engine Governor UI:** Exposes the backend's hardware tuning directly to the user:
  - Configure VRAM High/Low Watermarks.
  - Adjust the Memory-Bandwidth Ceiling.
  - Set Idle Offload Timeouts and Max Model Slots.
- **📊 Live Hardware Telemetry:** Real-time polling of CPU, GPU, System RAM, and VRAM budgets via NVML/sysinfo.

## ⌨️ Global Keyboard Shortcuts
Gabriel is designed for power users with a fully wired global shortcut system:

| Shortcut | Action | Scope |
| :--- | :--- | :--- |
| `Ctrl + K` | Open Command Palette / Global Search | Global |
| `Ctrl + N` | New Chat Session (Clears Context) | Global |
| `Ctrl + T` | Toggle Light/Dark Theme | Global |
| `Ctrl + ,` | Open Settings | Global |
| `Ctrl + Enter` | Send Prompt | Chat Context |
| `Ctrl + .` | Stop/Abort Generation | Chat Context |

## 🏗️ Architecture
Gabriel uses a **React + Tauri v2** architecture to communicate with the Rust core.

- **Frontend:** React, TypeScript, Tailwind CSS (Customized for Aurora Glass).
- **IPC Layer:** Strongly typed Tauri commands (`load_model`, `unload_model`, `offload_model`, `get_telemetry`, `list_loaded_models`).
- **REST Layer:** Local Axum server at `127.0.0.1:8080` for OpenAI-compatible `/v1/chat/completions`, `/v1/images/generations`, and `/v1/audio/speech`.

## 🚀 Getting Started

### Prerequisites
- [Node.js](https://nodejs.org/) installed.
- [pnpm](https://pnpm.io/) package manager.
- Rust 1.85+ (for compiling the Tauri backend).

### Installation & Development
1. **Install dependencies:**
   ```bash
   pnpm install
