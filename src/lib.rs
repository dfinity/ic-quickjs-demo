use ic_cdk::api::call::ManualReply;
use quickjs_wasm_rs::JSContextRef;

mod engine;
mod management_canister;
mod system_api;

const SCRIPT_NAME: &str = "ic.js";
const SCRIPT: &[u8] = include_bytes!("ic.js");

#[ic_cdk_macros::update(manual_reply = true)]
fn query() -> ManualReply<String> {
    engine::execute(
        "query",
        |_context| Ok(vec![]),
        |_context, result| match result {
            Ok(value) => {
                let result = value.as_str();
                match result {
                    Ok(value) => ManualReply::one(value.to_string()),
                    Err(err) => ManualReply::reject(err.to_string()),
                }
            }
            Err(err) => ManualReply::reject(err.to_string()),
        },
    )
}

#[ic_cdk_macros::init]
fn init() {
    unsafe { ic_wasi_polyfill::init(&[0_u8; 32]) };
    engine::init(linker, SCRIPT_NAME, std::str::from_utf8(SCRIPT).unwrap()).unwrap();
}

fn linker(context: &JSContextRef) -> Result<(), anyhow::Error> {
    system_api::link(context)?;
    management_canister::link(context)?;
    // Link other canisters here.
    Ok(())
}
