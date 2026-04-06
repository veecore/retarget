//! Integration tests for module-scoped targets loaded before hook install.

mod support;

use retarget::{hook, install_registered_hooks};

#[hook::c((support::test_dylib_path(), "hook_test_add_one"))]
unsafe extern "C" fn hook_test_add_one(value: i32) -> i32 {
    forward!() + 100
}

#[test]
fn intercepts_symbol_from_loaded_before_install_caller_image() {
    let _images = support::open_test_images();

    install_registered_hooks().expect("expected loaded-before-install hook install to succeed");

    let observed = support::call_test_caller_add_one(2);
    assert_eq!(observed, 103);

    let observed = support::call_test_target_add_one(2);
    assert_eq!(observed, 103);
}
