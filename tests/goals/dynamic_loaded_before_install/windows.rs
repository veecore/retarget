//! Windows tests for dynamic libraries loaded before hook install.

use retarget::{
    hook, install_registered_hooks,
    intercept::{Event, Mode},
};
use std::sync::{Mutex, OnceLock};

#[path = "../windows_dynamic_support.rs"]
mod support;

/// Enables interception tracking for this integration test binary.
#[hook::observer(default = Mode::EveryHit)]
fn observe_interception(event: Event) {
    events()
        .lock()
        .expect("event buffer should stay available")
        .push(event);
}

fn events() -> &'static Mutex<Vec<Event>> {
    static EVENTS: OnceLock<Mutex<Vec<Event>>> = OnceLock::new();
    EVENTS.get_or_init(|| Mutex::new(Vec::new()))
}

fn take_events() -> Vec<Event> {
    std::mem::take(&mut *events().lock().expect("event buffer should stay available"))
}

/// Intercepts one function exported from the helper target DLL.
#[hook::observe(Mode::EveryHit)]
#[hook::c(("hook_test_target.dll", "hook_test_add_one"))]
unsafe extern "system" fn hook_test_add_one(value: i32) -> i32 {
    forward!() + 100
}

/// Verifies that a caller DLL loaded before install still hits the detoured target.
#[test]
fn windows_intercepts_dynamic_target_loaded_before_install() {
    take_events();

    let _target = support::open_test_target_dll();
    let caller = support::open_test_caller_dll();

    install_registered_hooks().expect("expected Windows dynamic hook to install");

    let call_add_one = support::load_hook_test_call_add_one(caller);
    let observed = unsafe { call_add_one(2) };
    assert_eq!(observed, 103);

    let events = take_events();
    assert_hook_observed(&events, "hook_test_add_one");
}

fn assert_hook_observed(events: &[Event], suffix: &str) {
    let event = events
        .iter()
        .find(|event| event.hook_id.ends_with(suffix))
        .unwrap_or_else(|| panic!("expected interception event for {suffix}"));

    assert_eq!(event.mode, Mode::EveryHit);
}
