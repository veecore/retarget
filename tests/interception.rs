//! Integration tests focused on `#[hook::observer]` and `#[hook::observe]`.

#[path = "support/linked.rs"]
mod linked_support;
mod support;

use retarget::{
    hook, install_registered_hooks,
    intercept::{Mode, Signal},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Notice {
    AddOne,
}

static EVENTS: support::TestBuffer<Signal<Notice>> = support::TestBuffer::new();

#[hook::observer(default = Mode::Off)]
fn observe_interception(signal: Signal<Notice>) {
    EVENTS.push(signal);
}

#[hook::observe(Notice::AddOne, mode = Mode::EveryHit)]
#[hook::c]
unsafe extern "C" fn hook_test_add_one(value: i32) -> i32 {
    forward!() + 100
}

#[test]
fn reports_function_interceptions() {
    let _ = EVENTS.take();

    install_registered_hooks().expect("expected interception hooks to install");

    let observed_add_one = linked_support::call_linked_hook_test_add_one(2);
    assert_eq!(observed_add_one, 103);

    let events = EVENTS.take();
    let signal = events
        .iter()
        .find(|signal| signal.event.hook_id.ends_with("hook_test_add_one"))
        .expect("expected interception signal for hook_test_add_one");

    assert_eq!(signal.event.mode, Mode::EveryHit);
    assert_eq!(signal.value, Notice::AddOne);
}
