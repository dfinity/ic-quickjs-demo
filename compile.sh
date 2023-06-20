#!/usr/bin/bash
QUICKJS_WASM_SYS_WASI_SDK_PATH="/opt/wasi-sdk" CC_wasm32_wasi="/opt/wasi-sdk/bin/clang" cargo build --release --target=wasm32-wasi
wasi2ic ./target/wasm32-wasi/release/quickjs.wasm ic.wasm
