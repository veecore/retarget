# retarget

`retarget` is a typed hook crate for macOS and Windows with a deliberately
small, convenient public surface.

It is built to make native hooks feel straightforward in Rust:

- exported functions
- Objective-C methods
- COM methods

The intended flow is simple:

- declare hooks near the code that owns them
- optionally opt hooks into observation
- call `install_registered_hooks()` once

The crate is still evolving in the open, so the API should be treated as
experimental for now, but the direction is clear: less boilerplate, fewer
runtime concepts, and a more readable hook story.

What it is trying to be:

- keep the public API root-first
- make hook declarations small and typed
- keep macro-support details internal
- keep observation generic and separate from product reporting
- make the common path obvious and ergonomic

Intended feel:

```rust
use retarget::{
    hook,
    install_registered_hooks,
    intercept::{Mode, Signal},
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

Observation is opt-in per hook. Use `#[hook::observe]` to apply the observer's
default mode, or pass an explicit payload and/or mode when one hook needs a
different shape.

Some of the design choices behind that feel:

- `hook::c` should describe what to hook, not how installation works
- `hook::com_impl` is the main COM story, so related hooks can live in one
  inherent impl block
- observation is event-oriented and local, not a big retained runtime
- lower-level target types still exist when you need them, but they are not the
  first thing the crate asks you to touch

Warnings:

- The crate is still experimental, so expect API churn while the surface
  settles.
- Install hooks as early as practical in process startup.
- Anything under `retarget::__macro_support` and generated `__retarget_*`
  names is internal and may change without notice.
