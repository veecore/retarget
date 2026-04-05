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
use retarget::*;

#[hook::observer(default = FirstHit)]
fn on_interception(event: InterceptionEvent) {
    // consume generic interception events
}

#[hook::c(function = ("user32.dll", "GetCursorPos"))]
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

    #[hook::observe(EveryHit)]
    unsafe extern "system" fn present(...) -> HRESULT {
        forward!()
    }
}
```

`hook::function` should not expose install-mechanism choices. The macro says what to
hook, and the crate decides how to install it for the current platform. The
single `function` target accepts either a global symbol like
`"GetCursorPos"` or one scoped pair like `("user32.dll", "GetCursorPos")`.
`hook::com_impl` is the intended COM story: group related hooks behind an
inherent impl block, let method names default to the PascalCase COM field,
and only override with `#[hook::com(field = ...)]` when the Rust name differs.

Status: work in progress. Expect API churn until the surface settles.
