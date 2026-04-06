//! Integration tests for unscoped symbol resolution contracts.

mod support;

use retarget::{hook, install_registered_hooks};

#[hook::c]
unsafe extern "C" fn hook_test_add_one(value: i32) -> i32 {
    forward!() + 100
}

#[test]
fn unscoped_symbol_requires_target_to_be_loaded_before_install() {
    let error = install_registered_hooks()
        .expect_err("expected pre-load install failure for unresolved global symbol");
    let message = error.to_string();
    assert!(
        message.contains("required hook")
            || message.contains("required symbol not found")
            || message.contains("could not resolve")
            || message.contains("was not found"),
        "unexpected install error: {message}"
    );

    let _target = support::open_test_dylib();

    install_registered_hooks()
        .expect("expected unscoped hook install to succeed once the target is loaded");

    let observed = support::call_test_caller_add_one(2);
    assert_eq!(observed, 103);
}
