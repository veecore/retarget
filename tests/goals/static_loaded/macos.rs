//! macOS static or build-linked function-hook integration tests.

use retarget::{
    InterceptionMode, InterceptionState, hook, install_registered_hooks, interception_snapshot,
};

#[link(name = "hook_test_target")]
unsafe extern "C" {
    #[link_name = "hook_test_add_one"]
    fn linked_hook_test_add_one(value: i32) -> i32;
}

/// Intercepts one helper dylib export that the test binary links at build time.
#[hook::observe(EveryHit)]
#[hook::c(function = "hook_test_add_one")]
unsafe extern "C" fn hook_test_add_one(value: i32) -> i32 {
    forward!() + 100
}

/// Installs and intercepts one build-linked exported function on macOS.
#[test]
fn installs_and_intercepts_symbol_from_build_linked_image() {
    install_registered_hooks().expect("expected build-linked hook install to succeed");

    let observed = unsafe { linked_hook_test_add_one(2) };
    assert_eq!(observed, 103);

    let event = interception_snapshot()
        .into_iter()
        .find(|event| event.hook_id.ends_with("hook_test_add_one"))
        .expect("expected interception snapshot for helper hook");

    assert_eq!(event.mode, InterceptionMode::EveryHit);
    assert!(matches!(
        event.state,
        InterceptionState::Observed { count: 1, .. }
    ));
}
