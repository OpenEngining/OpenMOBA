target := env_var_or_default("CARGO_TARGET_DIR", "target")

web_dir := "web"
assets_dir := web_dir / "assets"

default: server

assets:
    cargo run --release -p mock-assets -- --assets {{ assets_dir }}

mock-assets:
    cargo run --release -p mock-assets -- --assets {{ assets_dir }}

import repo="":
    @if [ -z "{{ repo }}" ]; then \
        echo "Usage: just import repo=<git-url>"; \
        exit 1; \
    fi
    cargo run --git {{ repo }} --release -- --assets {{ assets_dir }}

build:
    cargo build --release -p game --target wasm32-unknown-unknown
    wasm-bindgen --target web --no-typescript --out-dir {{ web_dir }}/pkg {{ target }}/wasm32-unknown-unknown/release/game.wasm
    wasm-opt -O3 --enable-bulk-memory --enable-nontrapping-float-to-int -o {{ web_dir }}/pkg/game_bg.wasm {{ web_dir }}/pkg/game_bg.wasm

server port="7777": build
    cargo run --release -p server -- --port {{ port }} --web {{ web_dir }} --assets {{ assets_dir }} --data data

test:
    cargo test --workspace

fmt:
    cargo fmt --all

lint:
    cargo clippy --workspace --all-targets -- -D warnings
    cargo clippy -p game --target wasm32-unknown-unknown -- -D warnings

check: lint test
    cargo fmt --all --check
