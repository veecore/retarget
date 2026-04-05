//! macOS tests for public Objective-C resolution and hook installation.

use objc2::runtime::{NSObject, NSObjectProtocol};
use retarget::{
    InterceptionEvent, InterceptionMode, InterceptionState, ObjcMethod, hook,
    install_registered_hooks, interception_snapshot, into_objc_class, into_objc_selector,
};
use std::ffi::c_void;

/// Enables interception tracking for this integration test binary.
#[hook::observer(default = EveryHit)]
fn observe_interception(_: InterceptionEvent) {}

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
    let object = NSObject::new();
    let baseline = object.hash();

    install_registered_hooks().expect("expected Objective-C hook install to succeed");

    let observed = object.hash();
    assert_eq!(observed, baseline.wrapping_add(100));

    let snapshot = interception_snapshot();
    assert_hook_observed(&snapshot, "NSObjectHooks::hash");
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
