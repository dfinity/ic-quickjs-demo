# Demo of embedding the QuickJS engine in an Internet Computer canister

This repository shows how to run JavaScript code in a Rust canister using QuickJS.

## Background and dependencies

QuickJS is a JavaScript engine that supports the ES2020 specification. This demo uses the `quickjs-wasm-rs` crate from the [Javy](https://github.com/bytecodealliance/javy) project of Bytecode Alliance.
Since `quickjs-wasm-rs` targets `wasm32-wasi`, the demo also depends on the [wasi2ic](https://github.com/wasm-forge/wasi2ic) project to convert a WASI Wasm binary into an IC Wasm binary.

To try this demo, you would need to install the following:

- [wasi-sdk-19](https://github.com/WebAssembly/wasi-sdk/releases/tag/wasi-sdk-19): the `compile.sh` script assumes that the WASI SDK is installed in `/opt/wasi-sdk`. If that's not the case, then modify the script.
- [wasi2ic](https://github.com/wasm-forge/wasi2ic): the `compile.sh` script assumes that the `wasi2ic` is in the `$PATH`.

## How to run the demo

- The user JavaScript code goes in `ic.js`. You can modify it to try different JavaScript code. If you add a new public endpoint, then you also need to modify `lib.rs` to expose the endpoint in Rust code and convert between Candid and JS values.
- Run `./compile.sh`. This builds a WASI Wasm binary and then translates it into an IC Wasm binary named `ic.wasm`, which is a proper IC canister that can be installed using `dfx`.
- Call the public endpoint with `dfx`.

## How it works

TODO: explain the main idea

## Disclaimer

This demo is intended as a proof-of-concept prototype to show the IC community how to use QuickJS. Ideally, code here is used more as a source of inspiration for high-level ideas rather than being copied verbatim to production codebase.
Due to these reasons, this repository is mostly read-only and does not accept external contributions.