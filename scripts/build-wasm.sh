#!/bin/sh
# Builds crates/fsim-wasm/pkg for the app.
set -e
cd "$(dirname "$0")/.."
export PATH="$HOME/.cargo/bin:$PATH"
# On this Mac, rust-lld can't find libLLVM.dylib without the toolchain's lib directory.
export DYLD_LIBRARY_PATH="$(rustc --print sysroot)/lib${DYLD_LIBRARY_PATH:+:$DYLD_LIBRARY_PATH}"
wasm-pack build crates/fsim-wasm --target web --out-dir pkg
