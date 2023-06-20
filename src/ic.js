async function query(n) {
    ic0.debug_print(await managementCanister.canister_status(ic0.canister_self()));
    return "test";
}
