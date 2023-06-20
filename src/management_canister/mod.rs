use candid::utils::{decode_args, encode_args};
use ic_cdk::{
    api::management_canister::main::{CanisterIdRecord, CanisterStatusResponse},
    export::Principal,
};
use quickjs_wasm_rs::{CallbackArg, JSContextRef, JSError, JSValueRef};

use crate::engine;

pub fn link(context: &JSContextRef) -> Result<(), anyhow::Error> {
    fn raw_rand<'a>(
        context: &'a JSContextRef,
        _this: &CallbackArg,
        args: &[CallbackArg],
    ) -> Result<JSValueRef<'a>, anyhow::Error> {
        if args.len() != 0 {
            return Err(JSError::Type(format!("Expected 0 arguments, got {}", args.len())).into());
        }
        let args = encode_args(())?;

        engine::call(
            context,
            Principal::management_canister(),
            "raw_rand",
            &args,
            |context, bytes| {
                let (result,) = decode_args::<(Vec<u8>,)>(&bytes)?;
                context.array_buffer_value(&result)
            },
        )
    }

    fn canister_status<'a>(
        context: &'a JSContextRef,
        _this: &CallbackArg,
        args: &[CallbackArg],
    ) -> Result<JSValueRef<'a>, anyhow::Error> {
        if args.len() != 1 {
            return Err(JSError::Type(format!("Expected 1 argument, got {}", args.len())).into());
        }
        let canister_id: String = args[0].try_into()?;
        let canister_id = Principal::from_text(canister_id)?;
        let canister_id = CanisterIdRecord { canister_id };

        let args = encode_args((canister_id,))?;

        engine::call(
            context,
            Principal::management_canister(),
            "canister_status",
            &args,
            |context, bytes| {
                let (response,) = decode_args::<(CanisterStatusResponse,)>(&bytes)?;

                let js = context.object_value()?;
                js.set_property(
                    "status",
                    context.value_from_str(&format!("{:?}", response.status))?,
                )?;

                let cycles: u128 = response.cycles.0.try_into()?;
                js.set_property("cycles", context.value_from_f64(cycles as f64)?)?;

                let memory_size: u128 = response.memory_size.0.try_into()?;
                js.set_property("memory_size", context.value_from_f64(memory_size as f64)?)?;

                Ok(js)
            },
        )
    }

    let management = context.object_value()?;
    management.set_property("raw_rand", context.wrap_callback2(raw_rand)?)?;
    management.set_property("canister_status", context.wrap_callback2(canister_status)?)?;

    let global = context.global_object()?;
    global.set_property("managementCanister", management)?;
    Ok(())
}
