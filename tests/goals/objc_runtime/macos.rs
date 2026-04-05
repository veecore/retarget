//! macOS tests for public Objective-C resolution and hook installation.

use objc2::runtime::{NSObject, NSObjectProtocol};
use retarget::{
    ObjcMethod, hook, install_registered_hooks,
    intercept::{Event, Mode},
    into_objc_class, into_objc_selector,
};
use std::ffi::c_void;
use std::sync::{Mutex, OnceLock};

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

/// Groups one Objective-C hook set into one Rust impl block.
struct NSObjectHooks;

/// Intercepts `-[NSObject hash]` through the public Objective-C impl-style hook surface.
#[hook::objc::methods(class = "NSObject")]
impl NSObjectHooks {
    /// Intercepts one instance method while keeping the user-facing code in one impl block.
    #[hook::objc::instance]
    unsafe extern "C" fn hash(this: *mut c_void, cmd: *mut c_void) -> usize {
        let _ = (this, cmd);
        forward!() + 100
    }
}

/// Verifies that the public Objective-C resolution APIs resolve both instance and class methods.
#[test]
fn resolves_public_objective_c_targets() {
    let class = into_objc_class("NSObject").expect("expected NSObject to resolve");
    let instance_selector = into_objc_selector("hash").expect("expected hash selector");
    let class_selector = into_objc_selector("new").expect("expected new selector");

    let instance_method =
        ObjcMethod::instance(class.clone(), instance_selector).expect("expected instance method");
    let class_method = ObjcMethod::class(class, class_selector).expect("expected class method");

    assert!(instance_method.is_instance());
    assert!(class_method.is_class());
}

/// Installs one real Objective-C swizzle and verifies interception through the public macros.
#[test]
fn installs_objective_c_hook_and_intercepts_runtime_calls() {
    take_events();

    let object = NSObject::new();
    let baseline = object.hash();

    install_registered_hooks().expect("expected Objective-C hook install to succeed");

    let observed = object.hash();
    assert_eq!(observed, baseline.wrapping_add(100));

    let events = take_events();
    assert_hook_observed(&events, "NSObjectHooks::hash");
}

fn assert_hook_observed(events: &[Event], suffix: &str) {
    let event = events
        .iter()
        .find(|event| event.hook_id.ends_with(suffix))
        .unwrap_or_else(|| panic!("expected interception event for {suffix}"));

    assert_eq!(event.mode, Mode::EveryHit);
}
