use retarget::{Signal, hook, install_registered_hooks};
use std::sync::{Mutex, OnceLock};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DemoIntercept {
    ProcessId,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ObservedIntercept {
    signal: Signal<DemoIntercept>,
}

fn events() -> &'static Mutex<Vec<ObservedIntercept>> {
    static EVENTS: OnceLock<Mutex<Vec<ObservedIntercept>>> = OnceLock::new();
    EVENTS.get_or_init(|| Mutex::new(Vec::new()))
}

fn take_events() -> Vec<ObservedIntercept> {
    std::mem::take(&mut *events().lock().expect("event buffer should stay available"))
}

#[hook::observer(default = retarget::intercept::FirstHit)]
fn on_interception(signal: Signal<DemoIntercept>) {
    events()
        .lock()
        .expect("event buffer should stay available")
        .push(ObservedIntercept { signal });
}

#[cfg(target_os = "macos")]
#[hook::observe(
    DemoIntercept::ProcessId,
    mode = retarget::intercept::EveryHit
)]
#[hook::c]
unsafe extern "C" fn getpid() -> libc::pid_t {
    forward!()
}

#[cfg(target_os = "windows")]
#[hook::observe(
    DemoIntercept::ProcessId,
    mode = retarget::intercept::EveryHit
)]
#[hook::c(("kernel32.dll", "GetCurrentProcessId"))]
unsafe extern "system" fn hooked_get_current_process_id() -> u32 {
    forward!()
}

fn main() -> std::io::Result<()> {
    install_registered_hooks()?;

    #[cfg(target_os = "macos")]
    unsafe {
        let _ = libc::getpid();
        let _ = libc::getpid();
    }

    #[cfg(target_os = "windows")]
    unsafe {
        let _ = windows_sys::Win32::System::Threading::GetCurrentProcessId();
        let _ = windows_sys::Win32::System::Threading::GetCurrentProcessId();
    }

    for observed in take_events() {
        println!(
            "observed {:?} via {} at {}",
            observed.signal.value, observed.signal.event.hook_id, observed.signal.event.unix_ms
        );
    }

    Ok(())
}
