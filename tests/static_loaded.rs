//! Integration tests for statically or already-available function hooks.

#[path = "support/linked.rs"]
mod linked_support;

use retarget::{hook, install_registered_hooks};

#[hook::c]
unsafe extern "C" fn hook_test_add_one(value: i32) -> i32 {
    forward!() + 100
}

#[test]
fn installs_and_intercepts_symbol_from_build_linked_image() {
    let baseline = linked_support::call_linked_hook_test_add_one(2);
    assert_eq!(baseline, 3);

    install_registered_hooks().expect("expected build-linked hook install to succeed");

    let observed = linked_support::call_linked_hook_test_add_one(2);
    assert_eq!(observed, 103);
}
