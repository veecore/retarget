//! macOS static or build-linked function-hook integration tests.

use retarget::{
    hook, install_registered_hooks,
    intercept::{Mode, Signal},
};
use std::sync::{Mutex, OnceLock};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HookNotice {
    AddOne,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ObservedHook {
    signal: Signal<HookNotice>,
}

#[hook::observer(default = Mode::Off)]
fn observe_interception(signal: Signal<HookNotice>) {
    events()
        .lock()
        .expect("event buffer should stay available")
        .push(ObservedHook { signal });
}

fn events() -> &'static Mutex<Vec<ObservedHook>> {
    static EVENTS: OnceLock<Mutex<Vec<ObservedHook>>> = OnceLock::new();
    EVENTS.get_or_init(|| Mutex::new(Vec::new()))
}

fn take_events() -> Vec<ObservedHook> {
    std::mem::take(&mut *events().lock().expect("event buffer should stay available"))
}

#[link(name = "hook_test_target")]
unsafe extern "C" {
    #[link_name = "hook_test_add_one"]
    fn linked_hook_test_add_one(value: i32) -> i32;
}

/// Intercepts one helper dylib export that the test binary links at build time.
#[hook::observe(HookNotice::AddOne, mode = Mode::EveryHit)]
#[hook::c]
unsafe extern "C" fn hook_test_add_one(value: i32) -> i32 {
    forward!() + 100
}

/// Installs and intercepts one build-linked exported function on macOS.
#[test]
fn installs_and_intercepts_symbol_from_build_linked_image() {
    take_events();

    install_registered_hooks().expect("expected build-linked hook install to succeed");

    let observed = unsafe { linked_hook_test_add_one(2) };
    assert_eq!(observed, 103);

    let observed_hook = take_events()
        .into_iter()
        .find(|observed| observed.signal.event.hook_id.ends_with("hook_test_add_one"))
        .expect("expected interception event for helper hook");

    assert_eq!(observed_hook.signal.event.mode, Mode::EveryHit);
    assert_eq!(observed_hook.signal.value, HookNotice::AddOne);
}
