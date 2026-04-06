//! macOS dynamic-library tests for images loaded after install.

use retarget::{hook, install_registered_hooks, intercept::Event};

#[path = "../macos_support.rs"]
mod support;

#[hook::observer(default = retarget::intercept::EveryHit)]
fn observe_interception(_: Event) {}

/// Declares the helper dylib export hook before the library is present.
#[hook::observe]
#[hook::c(function = "hook_test_add_one")]
unsafe extern "C" fn hook_test_add_one(value: i32) -> i32 {
    forward!() + 100
}

/// Documents the current gap when the target dylib is loaded after install.
#[test]
fn install_fails_before_loaded_after_install_target_is_present_today() {
    let error = install_registered_hooks()
        .expect_err("expected pre-load install failure for unresolved dylib symbol");
    let message = error.to_string();
    assert!(
        message.contains("required hook")
            || message.contains("required symbol not found")
            || message.contains("could not resolve")
            || message.contains("was not found"),
        "unexpected install error: {message}"
    );
}

/// Captures the desired future behavior once add-image interception is fixed.
#[test]
#[ignore = "dynamic loaded-after-install interception is not implemented yet"]
fn intercepts_symbol_from_loaded_after_install_target() {
    install_registered_hooks().expect("expected hook install to succeed before dlopen");

    let _target = support::open_test_dylib();
    let caller = support::open_test_caller_dylib();
    let call_add_one = support::load_hook_test_call_add_one(caller);
    let observed = unsafe { call_add_one(2) };
    assert_eq!(observed, 103);
}
