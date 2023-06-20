use quickjs_wasm_rs::{CallbackArg, JSContextRef, JSValueRef};

pub fn link(context: &JSContextRef) -> Result<(), anyhow::Error> {
    fn debug_print<'a>(
        context: &'a JSContextRef,
        _this: &CallbackArg,
        args: &[CallbackArg],
    ) -> Result<JSValueRef<'a>, anyhow::Error> {
        for arg in args {
            let value = arg.to_js_value()?;
            ic_cdk::println!("{:?} ", value);
        }
        context.undefined_value()
    }

    fn canister_self<'a>(
        context: &'a JSContextRef,
        _this: &CallbackArg,
        _args: &[CallbackArg],
    ) -> Result<JSValueRef<'a>, anyhow::Error> {
        let canister_id = ic_cdk::id().to_text();
        context.value_from_str(&canister_id)
    }

    let ic0 = context.object_value()?;
    ic0.set_property("debug_print", context.wrap_callback2(debug_print)?)?;
    ic0.set_property("canister_self", context.wrap_callback2(canister_self)?)?;

    let global = context.global_object()?;
    global.set_property("ic0", ic0)?;
    Ok(())
}
