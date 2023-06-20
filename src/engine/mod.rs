use anyhow::Error;
use ic_cdk::api::call::ManualReply;
use quickjs_wasm_rs::{JSContextRef, JSValueRef};
use std::{cell::RefCell, collections::BTreeMap};

// The name and contents of the JS engine script.
const ENGINE_FILE: &str = "engine.js";
const ENGINE_SCRIPT: &[u8] = include_bytes!("engine.js");

// Keep these field and method names in sync with engine.js.
const ENGINE: &str = "__engine__";
const ID: &str = "id";
const REPLIED: &str = "replied";
const REJECTED: &str = "rejected";
const EXECUTE_ENDPOINT: &str = "executeEndpoint";
const EXECUTE_REPLY_CALLBACK: &str = "executeReplyCallback";
const EXECUTE_REJECT_CALLBACK: &str = "executeRejectCallback";
const CREATE_CALLBACK: &str = "createCallback";
const REMOVE_CALLBACK: &str = "removeCallback";
const GET_ENTERED_CALL_CONTEXT: &str = "getEnteredCallContext";

/// A function that returns the JS arguments for a public endpoint.
/// Usually this function converts the input arguments of the endpoint from
/// Candid to JS using the given JS context.
pub trait Arguments: FnOnce(&JSContextRef) -> Result<Vec<JSValueRef>, Error> {}

/// A function that converts the result of execution into the actual reply of
/// the endpoint.
pub trait Replier<R>: FnOnce(&JSContextRef, Result<JSValueRef, Error>) -> ManualReply<R> {}

// The internal representation of `Replier` with the result type erased such
// that it is possible to store the replier in a collection.
trait StoredReplier: FnOnce(&JSContextRef, Result<JSValueRef, Error>) -> () {}

/// A function that deserializes the result of an outgoing call.
/// Usually it converts from serialized Candid into a JS value.
pub trait CallResultDeserializer:
    FnOnce(&JSContextRef, Vec<u8>) -> Result<JSValueRef, Error>
{
}

// The unique ID of a call context.
//
// A call context represents an execution of a public endpoint. The
// execution starts when the public endpoint is invoked and finishes when all
// outgoing calls made by the endpoint finish.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
struct CallContextId(i32);

// The unique ID of a callback of an outgoing call.
//
// Each outgoing call has a callback with a pair of handlers (one for reply and
// another for reject).
#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
struct CallbackId(i32);

thread_local! {
    // The JS context in which all JS code is executed.
    static CONTEXT: RefCell<Option<JSContextRef>> = RefCell::new(None);

    // For each pending execution (call context), there is one replier that
    // produces an actual reply from the result of execution.
    static REPLIERS: RefCell<BTreeMap<CallContextId, Box<dyn StoredReplier>>> = RefCell::new(Default::default());

    // For each pending outgoing call, there is a deserializer that converts
    // the result of the call into a JS value.
    static DESERIALIZERS: RefCell<BTreeMap<CallbackId, Box<dyn CallResultDeserializer>>> = RefCell::new(Default::default());
}

/// The embedders must call this function to initialize the engine.
///
/// The first argument specifies the linker function that sets up functions for
/// calling other canisters.
///
/// The last two arguments specify the user JS script to be invoked.
pub fn init(
    linker: impl FnOnce(&JSContextRef) -> Result<(), Error>,
    script_name: &str,
    script: &str,
) -> Result<(), Error> {
    let context = JSContextRef::default();
    linker(&context)?;
    context.eval_global(ENGINE_FILE, std::str::from_utf8(ENGINE_SCRIPT).unwrap())?;
    context.eval_global(script_name, script)?;
    CONTEXT.with(|ctx| {
        let mut ctx = ctx.borrow_mut();
        *ctx = Some(context);
    });
    Ok(())
}

/// This helper starts execution of a public endpoint of the canister with the
/// given JS method name.
///
/// The arguments to the JS method are produced by the `arguments` function.
///
/// When the result of execution is ready, then the given `replier` function
/// will be invoked to produce the reply of the endpoint based on the JS result.
pub fn execute<R>(
    method: &str,
    arguments: impl Arguments,
    replier: impl Replier<R> + 'static,
) -> ManualReply<R> {
    CONTEXT.with(|context| {
        let mut context = context.borrow_mut();
        let context = context.as_mut().unwrap();
        match execute_js_endpoint(context, method, arguments) {
            Ok((_id, Some(value))) => replier(context, Ok(value)),
            Ok((id, None)) => {
                put_replier(id, |context, result| {
                    replier(context, result);
                });
                ManualReply::empty()
            }
            Err(err) => replier(context, Err(err)),
        }
    })
}

/// This helper starts an outgoing call the given method of another canister.
/// The arguments should be already in the serialized wire format (e.g. Candid).
/// When the call completes, the result of the call will be deserialized and
/// converted to a JS value using the given `call_result_deserializer` function.
pub fn call<'a>(
    context: &'a JSContextRef,
    canister_id: ic_cdk::export::Principal,
    method: &str,
    serialized_args: &[u8],
    call_result_deserializer: impl CallResultDeserializer + 'static,
) -> Result<JSValueRef<'a>, Error> {
    let global = context.global_object()?;
    let (callback_id, promise) = create_js_callback(&global)?;
    put_deserializer(callback_id, call_result_deserializer);

    let canister_id = canister_id.as_slice();
    let method = method.as_bytes();

    let err = unsafe {
        ic0::call_new(
            canister_id.as_ptr() as i32,
            canister_id.len() as i32,
            method.as_ptr() as i32,
            method.len() as i32,
            handle_call_reply as usize as i32,
            callback_id.0,
            handle_call_reject as usize as i32,
            callback_id.0,
        );
        ic0::call_data_append(
            serialized_args.as_ptr() as i32,
            serialized_args.len() as i32,
        );
        ic0::call_on_cleanup(remove_js_callback as usize as i32, callback_id.0);
        ic0::call_perform()
    };

    if err != 0 {
        let err = format!("Failed to make a call, error code: {}", err);
        let err = context.value_from_str(&err).unwrap();
        execute_js_callback(context, EXECUTE_REJECT_CALLBACK, callback_id, err);
    }
    Ok(promise)
}

// The reply callback of an outgoing call. It is marked as `extern "C"` because
// it is passed to `call_new` as a raw pointer.
#[no_mangle]
extern "C" fn handle_call_reply(callback_id: i32) {
    let callback_id = CallbackId(callback_id);
    CONTEXT.with(|context| {
        let mut context = context.borrow_mut();
        let context = context.as_mut().unwrap();
        let result = ic_cdk::api::call::arg_data_raw();
        let deserialize_call_result_fn = get_deserializer(callback_id).unwrap();
        match deserialize_call_result_fn(context, result) {
            Ok(result) => execute_js_callback(context, EXECUTE_REPLY_CALLBACK, callback_id, result),
            Err(err) => {
                let err = context.value_from_str(&err.to_string()).unwrap();
                execute_js_callback(context, EXECUTE_REJECT_CALLBACK, callback_id, err)
            }
        }
    });
}

// The reject callback of an outgoing call. It is marked as `extern "C"` because
// it is passed to `call_new` as a raw pointer.
#[no_mangle]
extern "C" fn handle_call_reject(callback_id: i32) {
    let callback_id = CallbackId(callback_id);
    CONTEXT.with(|context| {
        let mut context = context.borrow_mut();
        let context = context.as_mut().unwrap();
        let err = ic_cdk::api::call::reject_message();
        let err = context.value_from_str(&err.to_string()).unwrap();
        let _ignore = get_deserializer(callback_id);
        execute_js_callback(context, EXECUTE_REJECT_CALLBACK, callback_id, err)
    });
}

// The cleanup callback of an outgoing call. It is marked as `extern "C"` because
// it is passed to `call_new` as a raw pointer.
#[no_mangle]
extern "C" fn remove_js_callback(callback_id: i32) {
    let callback_id = CallbackId(callback_id);
    CONTEXT.with(|context| {
        let mut context = context.borrow_mut();
        let context = context.as_mut().unwrap();
        let _ignore = get_deserializer(callback_id);
        let global = context.global_object().unwrap();
        let engine = global.get_property(ENGINE).unwrap();
        let cleanup_method = engine.get_property(REMOVE_CALLBACK).unwrap();
        let _ignore = cleanup_method.call(&engine, &[]).unwrap();
    });
}

// An internal helper that invokes a JS endpoint.
fn execute_js_endpoint<'a>(
    context: &'a JSContextRef,
    method: &str,
    arguments: impl Arguments,
) -> Result<(CallContextId, Option<JSValueRef<'a>>), Error> {
    let global = context.global_object()?;
    let engine = global.get_property(ENGINE)?;
    let execute_method = engine.get_property(EXECUTE_ENDPOINT)?;
    let js_endpoint = global.get_property(method)?;
    let args = arguments(context)?;
    let args = [&[js_endpoint], args.as_slice()].concat();
    execute_js_task(context, &engine, &execute_method, &args)
}

// An internal helper that invokes a JS callback.
fn execute_js_callback<'a>(
    context: &'a JSContextRef,
    callback_method: &str,
    callback_id: CallbackId,
    result: JSValueRef<'a>,
) {
    let global = context.global_object().unwrap();
    let engine = global.get_property(ENGINE).unwrap();
    let callback_id = context.value_from_i32(callback_id.0).unwrap();
    let args = &[callback_id, result];
    let callback_method = engine.get_property(callback_method).unwrap();
    match execute_js_task(context, &engine, &callback_method, args) {
        Ok((id, Some(value))) => {
            let reply_fn = get_replier(id).unwrap();
            reply_fn(context, Ok(value));
        }
        Ok((_id, None)) => {}
        Err(err) => {
            let method = engine.get_property(GET_ENTERED_CALL_CONTEXT).unwrap();
            let entered_call_context = method.call(&engine, &[]).unwrap();
            let id = entered_call_context
                .get_property(ID)
                .unwrap()
                .try_as_integer()
                .unwrap();
            let reply_fn = get_replier(CallContextId(id)).unwrap();
            reply_fn(context, Err(err));
        }
    }
}

// An internal helper that invokes either a JS endpoint or a JS callback.
// It advances pending jobs and processes the result of execution.
// It returns:
// - `Err(error)` if there was any error during the execution.
// - `Ok(call_context_id, Some(js_value))` if the execution produced a result.
// - `Ok(call_context_id, None)` if the execution did not produce any result
//   due to pending outgoing calls.
fn execute_js_task<'a>(
    context: &'a JSContextRef,
    engine: &JSValueRef<'a>,
    method: &JSValueRef<'a>,
    args: &[JSValueRef<'a>],
) -> Result<(CallContextId, Option<JSValueRef<'a>>), Error> {
    let entered_call_context = method.call(engine, &args)?;
    context.execute_pending()?;
    let id = entered_call_context.get_property(ID)?.try_as_integer()?;
    let replied = entered_call_context.get_property(REPLIED)?;
    let rejected = entered_call_context.get_property(REJECTED)?;
    match (
        replied.is_null_or_undefined(),
        rejected.is_null_or_undefined(),
    ) {
        (true, true) => Ok((CallContextId(id), None)),
        (false, true) => Ok((CallContextId(id), Some(replied))),
        (true, false) => {
            let exception = quickjs_wasm_rs::Exception::from(rejected)?;
            let err = exception.into_error();
            Err(err)
        }
        (false, false) => unreachable!("The result cannot be both replied and rejected."),
    }
}

// An internal helper that creates a JS callback for an outgoing call.
fn create_js_callback<'a>(global: &JSValueRef<'a>) -> Result<(CallbackId, JSValueRef<'a>), Error> {
    let engine = global.get_property(ENGINE)?;
    let callback_method = engine.get_property(CREATE_CALLBACK)?;
    let result = callback_method.call(&engine, &[])?;
    let callback_id = result.get_indexed_property(0)?;
    let promise = result.get_indexed_property(1)?;
    let callback_id = callback_id.try_as_integer()?;
    Ok((CallbackId(callback_id), promise))
}

// An internal helper that saves the given replier function.
fn put_replier(id: CallContextId, replier: impl StoredReplier + 'static) {
    REPLIERS.with(|store| {
        let mut store = store.borrow_mut();
        store.insert(id, Box::new(replier));
    });
}

// An internal helper that retrieves the previously saved replier function.
fn get_replier(id: CallContextId) -> Option<impl StoredReplier> {
    REPLIERS.with(|store| {
        let mut store = store.borrow_mut();
        store.remove(&id)
    })
}

// An internal helper that saves the given deserializer function.
fn put_deserializer(id: CallbackId, deserializer: impl CallResultDeserializer + 'static) {
    DESERIALIZERS.with(|store| {
        let mut store = store.borrow_mut();
        store.insert(id, Box::new(deserializer));
    });
}

// An internal helper that retrieves the previously saved deserializer function.
fn get_deserializer(id: CallbackId) -> Option<impl CallResultDeserializer> {
    DESERIALIZERS.with(|store| {
        let mut store = store.borrow_mut();
        store.remove(&id)
    })
}

// Boilerplate for the function traits.
impl<F: FnOnce(&JSContextRef) -> Result<Vec<JSValueRef>, Error>> Arguments for F {}
impl<R, F: FnOnce(&JSContextRef, Result<JSValueRef, Error>) -> ManualReply<R>> Replier<R> for F {}
impl<F: FnOnce(&JSContextRef, Result<JSValueRef, Error>) -> ()> StoredReplier for F {}
impl<F: FnOnce(&JSContextRef, Vec<u8>) -> Result<JSValueRef, Error>> CallResultDeserializer for F {}
