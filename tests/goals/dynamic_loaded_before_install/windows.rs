//! Windows tests for dynamic libraries loaded before hook install.

use retarget::{
    InterceptionEvent, InterceptionMode, InterceptionState, hook, install_registered_hooks,
    interception_snapshot,
};

#[path = "../windows_dynamic_support.rs"]
mod support;

/// Enables interception tracking for this integration test binary.
#[hook::observer(default = EveryHit)]
fn observe_interception(_: InterceptionEvent) {}

/// Intercepts one function exported from the helper target DLL.
#[hook::observe(EveryHit)]
#[hook::c(function = ("hook_test_target.dll", "hook_test_add_one"))]
unsafe extern "system" fn hook_test_add_one(value: i32) -> i32 {
    forward!() + 100
}

/// Verifies that a caller DLL loaded before install still hits the detoured target.
#[test]
fn windows_intercepts_dynamic_target_loaded_before_install() {
    let _target = support::open_test_target_dll();
    let caller = support::open_test_caller_dll();

    install_registered_hooks().expect("expected Windows dynamic hook to install");

    let call_add_one = support::load_hook_test_call_add_one(caller);
    let observed = unsafe { call_add_one(2) };
    assert_eq!(observed, 103);

    let snapshot = interception_snapshot();
    assert_hook_observed(&snapshot, "hook_test_add_one");
}

fn assert_hook_observed(snapshot: &[InterceptionEvent], suffix: &str) {
    let event = snapshot
        .iter()
        .find(|event| event.hook_id.ends_with(suffix))
        .unwrap_or_else(|| panic!("expected interception snapshot entry for {suffix}"));

    assert_eq!(event.mode, InterceptionMode::EveryHit);
    match &event.state {
        InterceptionState::Observed { count, .. } => assert!(*count >= 1),
        InterceptionState::Unobserved => {
            panic!("expected observed interception state for {suffix}")
        }
    }
}
