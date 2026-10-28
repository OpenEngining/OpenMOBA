{pkgs, ...}: {
  packages = with pkgs; [
    rustc
    cargo
    rustfmt
    clippy
    rust-analyzer
    lld
    wasm-bindgen-cli_0_2_121
    binaryen
    just
    vgmstream
    ffmpeg-headless
  ];
}
