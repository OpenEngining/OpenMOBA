# OpenMOBA

OpenMOBA is a high-performance, data-driven MOBA (Multiplayer Online Battle Arena) engine written in Rust and WebAssembly. It provides a browser-native game client rendered via WebGL alongside an authoritative server and deterministic simulation engine.

## Features

- **Web-First Client**: Fast, hardware-accelerated WebGL 3D rendering and WebAudio engine running purely inside standard browsers via WebAssembly.
- **Deterministic Simulation**: Separation of simulation rules (`crates/sim`) and rendering (`crates/game`), ensuring reproducible match logic and client-side prediction.
- **Data-Driven Architecture**: Champions, abilities, items, stat scaling, and maps are completely decoupled from the runtime engine.
- **Standalone Out-of-the-box**: Ships with a procedural mock asset generator (`crates/mock-assets`) allowing instant development and local play without external assets.
- **Lightweight Server**: High-throughput async WebSocket server (`crates/server`) handling lobbies, chat, champion drafting, and real-time state synchronization.

## Architecture

| Crate | Purpose |
| --- | --- |
| `crates/sim` | Core gameplay simulation, navigation grids, combat formulas, status effects, and kit logic. |
| `crates/game` | WebAssembly client: WebGL rendering pipeline, animation blending, sound playback, and DOM UI. |
| `crates/server` | Async server supporting lobby management, matchmaking, and game tick synchronization. |
| `crates/protocol` | Compact binary serialization protocol shared across client and server. |
| `crates/asset` | Data structures and loaders for 3D meshes, skeleton hierarchies, animations, and game stats. |
| `crates/mock-assets` | Procedural asset generator producing valid meshes, textures, and data tables for standalone testing. |

## Quick Start

### Prerequisites

- [Rust](https://rustup.rs/) (with `wasm32-unknown-unknown` target)
- [wasm-bindgen-cli](https://rustwasm.github.io/wasm-bindgen/)
- [wasm-opt](https://github.com/WebAssembly/binaryen)
- [just](https://github.com/casey/just)

*(Or simply enter the provided [devenv](https://devenv.sh/) environment via `devenv shell` which includes all tools automatically).*

### Building and Running

1. **Generate Assets:**
   ```bash
   just assets
   ```
   This generates procedural test assets into `web/assets`.

2. **Run Server and Web Client:**
   ```bash
   just server
   ```
   Open `http://localhost:7777` in your browser.

3. **Run Tests:**
   ```bash
   just check
   ```

## Custom Asset Bundles

OpenMOBA supports importing data-driven community asset bundles:

```bash
just import repo=<git-url>
```

## Disclaimer

OpenMOBA is an independent open-source project. All trademarks, service marks, and trade names referenced for compatibility or educational purposes belong to their respective owners. OpenMOBA is not endorsed by, sponsored by, or affiliated with any commercial game publisher.
