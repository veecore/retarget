//! macOS dynamic-library tests for caller and target images loaded before install.

use retarget::{
    hook, install_registered_hooks,
    intercept::{Event, Mode},
};
use std::sync::{Mutex, OnceLock};

#[path = "../macos_support.rs"]
mod support;

/// Intercepts the helper dylib export after the library is already loaded.
#[hook::observer(default = Mode::Off)]
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

/// Intercepts the helper dylib export after the library is already loaded.
#[hook::observe(Mode::EveryHit)]
#[hook::c(function = "hook_test_add_one")]
unsafe extern "C" fn hook_test_add_one(value: i32) -> i32 {
    forward!() + 100
}

/// Installs and intercepts the override after both dylibs are loaded.
#[test]
fn intercepts_symbol_from_loaded_before_install_caller_image() {
    take_events();

    let _target = support::open_test_dylib();
    let caller = support::open_test_caller_dylib();
    install_registered_hooks().expect("expected loaded-before-install hook install to succeed");

    let call_add_one = support::load_hook_test_call_add_one(caller);
    let observed = unsafe { call_add_one(2) };
    assert_eq!(observed, 103);

    let event = take_events()
        .into_iter()
        .find(|event| event.hook_id.ends_with("hook_test_add_one"))
        .expect("expected interception event for helper hook");

    assert_eq!(event.mode, Mode::EveryHit);
}
