//! Integration tests for module-scoped targets loaded after hook install.

mod support;

use retarget::{hook, install_registered_hooks};

#[hook::c((support::test_dylib_path(), "hook_test_add_one"))]
unsafe extern "C" fn hook_test_add_one(value: i32) -> i32 {
    forward!() + 100
}

#[test]
fn module_scoped_symbol_auto_primes_loaded_after_install_target() {
    install_registered_hooks()
        .expect("expected module-scoped hook install to auto-prime the target image");

    let observed = support::call_test_caller_add_one(2);
    assert_eq!(observed, 103);

    let observed = support::call_test_target_add_one(2);
    assert_eq!(observed, 103);
}
