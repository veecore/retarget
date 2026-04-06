//! Integration tests for Objective-C target resolution and swizzle install.

#[cfg(target_os = "macos")]
mod macos {
    use objc2::rc::Retained;
    use objc2::runtime::{NSObject, NSObjectProtocol};
    use objc2::{ClassType, define_class, extern_methods};
    use retarget::{
        ObjcMethod, hook, install_registered_hooks, into_objc_class, into_objc_selector,
    };
    use std::ffi::c_void;
    use std::sync::{Mutex, OnceLock};

    fn test_lock() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(|error| error.into_inner())
    }

    define_class!(
        #[unsafe(super(NSObject))]
        #[name = "__RetargetObjcInheritedBase"]
        struct InheritedBase;

        impl InheritedBase {
            #[unsafe(method(markerValue))]
            fn __marker_value(&self) -> usize {
                7
            }

            #[unsafe(method(classMarkerValue))]
            fn __class_marker_value() -> usize {
                70
            }
        }
    );

    define_class!(
        #[unsafe(super(InheritedBase))]
        #[name = "__RetargetObjcInheritedTarget"]
        struct InheritedTarget;
    );

    define_class!(
        #[unsafe(super(InheritedBase))]
        #[name = "__RetargetObjcInheritedSibling"]
        struct InheritedSibling;
    );

    impl InheritedBase {
        extern_methods!(
            #[unsafe(method(new))]
            fn new() -> Retained<Self>;

            #[unsafe(method(markerValue))]
            fn marker_value(&self) -> usize;

            #[unsafe(method(classMarkerValue))]
            fn class_marker_value() -> usize;
        );
    }

    impl InheritedTarget {
        extern_methods!(
            #[unsafe(method(new))]
            fn new() -> Retained<Self>;

            #[unsafe(method(markerValue))]
            fn marker_value(&self) -> usize;

            #[unsafe(method(classMarkerValue))]
            fn class_marker_value() -> usize;
        );
    }

    impl InheritedSibling {
        extern_methods!(
            #[unsafe(method(new))]
            fn new() -> Retained<Self>;

            #[unsafe(method(markerValue))]
            fn marker_value(&self) -> usize;

            #[unsafe(method(classMarkerValue))]
            fn class_marker_value() -> usize;
        );
    }

    struct NSObjectHooks;

    #[hook::objc::methods(class = "NSObject")]
    impl NSObjectHooks {
        #[hook::objc::instance]
        unsafe extern "C" fn hash(this: *mut c_void, cmd: *mut c_void) -> usize {
            let _ = (this, cmd);
            forward!() + 100
        }
    }

    struct InheritedHooks;

    #[hook::objc::methods(class = "__RetargetObjcInheritedTarget")]
    impl InheritedHooks {
        #[hook::objc::instance(selector = "markerValue")]
        unsafe extern "C" fn marker_value(this: *mut c_void, cmd: *mut c_void) -> usize {
            let _ = (this, cmd);
            forward!() + 100
        }

        #[hook::objc::class(selector = "classMarkerValue")]
        unsafe extern "C" fn class_marker_value(cls: *mut c_void, cmd: *mut c_void) -> usize {
            let _ = (cls, cmd);
            forward!() + 100
        }
    }

    #[test]
    fn resolves_public_objective_c_targets() {
        let _guard = test_lock();

        let class = into_objc_class("NSObject").expect("expected NSObject to resolve");
        let instance_selector = into_objc_selector("hash").expect("expected hash selector");
        let class_selector = into_objc_selector("new").expect("expected new selector");

        let instance_method = ObjcMethod::instance(class.clone(), instance_selector)
            .expect("expected instance method");
        let class_method = ObjcMethod::class(class, class_selector).expect("expected class method");

        assert!(instance_method.is_instance());
        assert!(class_method.is_class());
    }

    #[test]
    fn installs_objective_c_hooks_and_localizes_inherited_methods() {
        let _guard = test_lock();

        let object = NSObject::new();
        let object_baseline = object.hash();

        let _ = InheritedBase::class();
        let _ = InheritedTarget::class();
        let _ = InheritedSibling::class();

        let base = InheritedBase::new();
        let target = InheritedTarget::new();
        let sibling = InheritedSibling::new();

        assert_eq!(base.marker_value(), 7);
        assert_eq!(target.marker_value(), 7);
        assert_eq!(sibling.marker_value(), 7);
        assert_eq!(InheritedBase::class_marker_value(), 70);
        assert_eq!(InheritedTarget::class_marker_value(), 70);
        assert_eq!(InheritedSibling::class_marker_value(), 70);

        install_registered_hooks().expect("expected Objective-C hook install to succeed");

        let observed = object.hash();
        assert_eq!(observed, object_baseline.wrapping_add(100));

        assert_eq!(base.marker_value(), 7);
        assert_eq!(target.marker_value(), 107);
        assert_eq!(sibling.marker_value(), 7);
        assert_eq!(InheritedBase::class_marker_value(), 70);
        assert_eq!(InheritedTarget::class_marker_value(), 170);
        assert_eq!(InheritedSibling::class_marker_value(), 70);
    }
}
