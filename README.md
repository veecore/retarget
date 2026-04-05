# retarget

`retarget` is a work-in-progress hook crate for macOS and Windows.

It is aimed at small, typed hook declarations over three target families:

- exported functions
- Objective-C methods
- COM methods

The crate is still being shaped in the open, so the API should be treated as
experimental for now.

Current goals:

- keep the public API root-first
- make hook declarations small and typed
- keep macro-support details internal
- keep observation generic and separate from product reporting
- remove legacy metadata and macro arguments

Current intended feel:

```rust
use retarget::{
    hook,
    Signal,
    intercept::Mode,
};
use std::sync::{Mutex, OnceLock};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MonitoringIntercept {
    CursorQuery,
    Present,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ObservedIntercept {
    signal: Signal<MonitoringIntercept>,
}

fn events() -> &'static Mutex<Vec<ObservedIntercept>> {
    static EVENTS: OnceLock<Mutex<Vec<ObservedIntercept>>> = OnceLock::new();
    EVENTS.get_or_init(|| Mutex::new(Vec::new()))
}

#[hook::observer(default = Mode::FirstHit)]
fn on_interception(signal: Signal<MonitoringIntercept>) {
    events().lock().unwrap().push(ObservedIntercept { signal });
}

#[hook::observe(MonitoringIntercept::CursorQuery)]
#[hook::c(("user32.dll", "GetCursorPos"))]
unsafe extern "system" fn get_cursor_pos(...) -> BOOL {
    forward!()
}

struct SwapChainHooks;

#[hook::com_impl(interface = IDXGISwapChain, instance = swap_chain_ptr())]
impl SwapChainHooks {
    #[hook::com(field = ResizeBuffers)]
    unsafe extern "system" fn resize_buffers(...) -> HRESULT {
        forward!()
    }

    #[hook::observe(MonitoringIntercept::Present, mode = Mode::EveryHit)]
    unsafe extern "system" fn present(...) -> HRESULT {
        forward!()
    }
}

let events = std::mem::take(&mut *events().lock().unwrap());
assert!(events.iter().all(|entry| entry.signal.event.mode != Mode::Off));
```

`hook::function` should not expose install-mechanism choices. The macro says what to
hook, and the crate decides how to install it for the current platform. When
the Rust function name already matches the export, `#[hook::c]` is enough.
Otherwise the target accepts either a global symbol like `"GetCursorPos"` or
one scoped pair like `("user32.dll", "GetCursorPos")`.
`hook::com_impl` is the intended COM story: group related hooks behind an
inherent impl block, let method names default to the PascalCase COM field,
and only override with `#[hook::com(field = ...)]` when the Rust name differs.

Status: work in progress. Expect API churn until the surface settles.
