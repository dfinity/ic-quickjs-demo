// All the machinery for keeping track of outgoing IC calls using JS promises.
Object.defineProperty(globalThis, "__engine__", {
	enumerable: false,
	value: (function () {
		// The JS engine has an efficient representation for 31-bit integers. 
		// If we ensure that the max ID never exceeds this value, then all
		// operations with IDs will be fast.
		const MAX_ID_MASK = 0x7FFF_FFFF;

		// A call context represents a pending execution of a public endpoint of
		// the cansiter. An incoming message, a heartbeat, and a timer create a
		// new call context. A call context is removed all outgoing calls made
		// within the call context complete.
		let call_context_id = 0;
		const call_contexts = new Map();

		// For each outgoing call, this stores information necessary to execute
		// the corresponding response/reject handler.
		let callback_id = 0;
		const callbacks = new Map();

		// The currently active call context.
		let entered_call_context = null;

		// Activate the given call context.
		function enterCallContext(call_context) {
			if (entered_call_context && entered_call_context != call_context) {
				// Free the previously active call context if it has no outgoing calls.
				if (entered_call_context.pending_calls == 0) {
					call_contexts.delete(entered_call_context.id);
				}
			}
			entered_call_context = call_context;
			entered_call_context.replied = null;
			entered_call_context.rejected = null;
		}

		// Creates a new call context.
		function newCallContext() {
			let new_call_context = {
				id: call_context_id,
				// This field stores the "success" result of the call context.
				replied: null,
				// This field stores the "failure" result of the call context.
				rejected: null,
				// The number of pending outgoing calls.
				pending_calls: 0,
			};
			call_contexts.set(call_context_id, new_call_context);

			// Find the next available call context id.
			do {
				call_context_id = (call_context_id + 1) & MAX_ID_MASK;
			} while (call_contexts.get(call_context_id));

			return new_call_context;
		}

		// Execute the given method with the given arguments.
		function executeEndpoint(method, ...args) {
			// First enter a new call context.
			let call_context = newCallContext();
			enterCallContext(call_context);

			// Actually execute the method.
			let result = method.call(globalThis, ...args);

			// Process the result. `Promise.resolve` allows to process
			// both promise and non-promise values uniformly by wrapping
			// non-promise values in a promise.
			// This ensures that the result of the promise is stored in
			// active call context when the promise settles.
			Promise.resolve(result)
				.then((r) => entered_call_context.replied = r,
					(e) => entered_call_context.rejected = e)

			return entered_call_context;
		}

		// Executes the `reply` callback of an outgoing call.
		function executeReplyCallback(callback_id, ...args) {
			let callback = callbacks.get(callback_id);
			callbacks.delete(callback_id);

			let call_context = call_contexts.get(callback.call_context_id);
			enterCallContext(call_context);

			call_context.pending_calls -= 1;
			callback.reply.call(globalThis, ...args);

			return entered_call_context;
		}

		// Executes the `reject` callback of an outgoing call.
		function executeRejectCallback(callback_id, ...args) {
			let callback = callbacks.get(callback_id);
			callbacks.delete(callback_id);

			let call_context = call_contexts.get(callback.call_context_id);
			enterCallContext(call_context);

			call_context.pending_calls -= 1;
			callback.reject.call(globalThis, ...args);

			return entered_call_context;
		}

		// Registers a new callback for an outgoing call.
		function createCallback() {
			let reply = null;
			let reject = null;
			let promise = new Promise((a, b) => { reply = a; reject = b; });

			let callback = {
				call_context_id: entered_call_context.id,
				promise,
				reply,
				reject,
			};

			callbacks.set(callback_id, callback);
			entered_call_context.pending_calls += 1;

			let result = callback_id;

			// Find the next available callback id.
			do {
				callback_id = (callback_id + 1) & MAX_ID_MASK;
			} while (callbacks.get(callback_id));

			return [result, promise];
		}

		// Unregisters the previously registered callback.
		function removeCallback(callback_id) {
			callbacks.delete(callback_id);
		}

		// Returns the currently active call context.
		function getEnteredCallContext() {
			return entered_call_context;
		}

		// Exports public methods. 
		return {
			executeEndpoint,
			executeReplyCallback,
			executeRejectCallback,
			createCallback,
			removeCallback,
			getEnteredCallContext,
		};
	})()
});