//! Windows tests for statically available function and object hooks.

use retarget::{
    hook, install_registered_hooks,
    intercept::{Event, Mode},
};
use std::ffi::c_void;
use std::sync::{Mutex, OnceLock};
use windows::Win32::Foundation::HGLOBAL;
use windows::Win32::System::Com::StructuredStorage::CreateStreamOnHGlobal;
use windows::Win32::System::Com::{IStream, STGC_DEFAULT};
use windows::core::{HRESULT, Interface};

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

#[link(kind = "static", name = "hook_test_target_static")]
unsafe extern "C" {
    #[link_name = "hook_test_add_one"]
    fn linked_hook_test_add_one(value: i32) -> i32;
}

/// Detoured build-linked helper function from the C fixture.
#[hook::observe]
#[hook::c(function = "hook_test_add_one")]
unsafe extern "C" fn hooked_linked_hook_test_add_one(value: i32) -> i32 {
    forward!() + 100
}

/// Decorative grouping for related COM hooks.
struct StreamHooks;

#[hook::com_impl(interface = IStream, instance = test_stream_ptr())]
impl StreamHooks {
    /// Detoured real system COM method `IStream::SetSize`.
    #[hook::observe]
    #[hook::com(field = SetSize)]
    unsafe extern "system" fn set_size(this: *mut c_void, libnewsize: u64) -> HRESULT {
        let _ = (this, libnewsize);
        HRESULT(0x80004005u32 as i32)
    }

    /// Detoured real system COM method `IStream::Commit`.
    #[hook::observe]
    unsafe extern "system" fn commit(this: *mut c_void, grfcommitflags: u32) -> HRESULT {
        let _ = (this, grfcommitflags);
        HRESULT(0x80004005u32 as i32)
    }
}

/// Installs the registered static hooks and verifies both function and COM interception.
#[test]
fn windows_intercepts_static_function_and_com_hooks() {
    take_events();

    let baseline = unsafe { linked_hook_test_add_one(2) };
    assert_eq!(baseline, 3);

    let stream = test_stream();
    unsafe { stream.SetSize(16) }.expect("expected baseline IStream::SetSize to succeed");
    unsafe { stream.Commit(STGC_DEFAULT) }.expect("expected baseline IStream::Commit to succeed");

    install_registered_hooks().expect("expected Windows static hooks to install");

    let observed = unsafe { linked_hook_test_add_one(2) };
    assert_eq!(observed, 103);

    let set_size_error =
        unsafe { stream.SetSize(32) }.expect_err("expected detoured IStream::SetSize");
    assert_eq!(set_size_error.code(), HRESULT(0x80004005u32 as i32));

    let commit_error =
        unsafe { stream.Commit(STGC_DEFAULT) }.expect_err("expected detoured IStream::Commit");
    assert_eq!(commit_error.code(), HRESULT(0x80004005u32 as i32));

    let events = take_events();
    assert_hook_observed(&events, "hooked_linked_hook_test_add_one");
    assert_hook_observed(&events, "StreamHooks::set_size");
    assert_hook_observed(&events, "StreamHooks::commit");
}

fn test_stream() -> IStream {
    let raw = test_stream_ptr();
    unsafe { <IStream as Interface>::from_raw_borrowed(&raw) }
        .expect("shared test stream pointer must stay valid")
        .clone()
}

fn test_stream_ptr() -> *mut c_void {
    *stream_slot() as *mut c_void
}

fn stream_slot() -> &'static usize {
    static STREAM: OnceLock<usize> = OnceLock::new();
    STREAM.get_or_init(|| {
        let stream = unsafe { CreateStreamOnHGlobal(HGLOBAL(std::ptr::null_mut()), false) }
            .expect("expected CreateStreamOnHGlobal to succeed");
        let raw = stream.as_raw() as usize;
        std::mem::forget(stream);
        raw
    })
}

fn assert_hook_observed(events: &[Event], suffix: &str) {
    let event = events
        .iter()
        .find(|event| event.hook_id.ends_with(suffix))
        .unwrap_or_else(|| panic!("expected interception event for {suffix}"));

    assert_eq!(event.mode, Mode::EveryHit);
}
