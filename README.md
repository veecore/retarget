# retarget

`retarget` is a typed hook crate for macOS and Windows with a deliberately
small, convenient public surface.

It is built to make native hooks feel straightforward in Rust:

- exported functions
- Objective-C methods
- COM methods

The intended flow is simple:

- declare hooks near the code that owns them
- optionally log or observe hits
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

If you want the short version, `retarget` is trying to make this kind of work
feel normal:

- declare a hook in ordinary Rust
- install once near startup
- watch regular code flow through the detour

Quick feel:

```rust
use std::fs::File;
use std::io::ErrorKind;

use retarget::{hook, install_registered_hooks};

#[cfg(target_os = "macos")]
#[hook::c]
unsafe extern "C" fn open(
    _path: *const libc::c_char,
    _flags: libc::c_int,
    _mode: libc::mode_t,
) -> libc::c_int {
    unsafe {
        *libc::__error() = libc::ENOENT;
    }
    -1
}

#[cfg(target_os = "windows")]
#[hook::c(("kernel32.dll", "CreateFileW"))]
unsafe extern "system" fn create_file_w(
    _path: *const u16,
    _access: u32,
    _share: u32,
    _security: *const std::ffi::c_void,
    _creation: u32,
    _flags: u32,
    _template: *mut std::ffi::c_void,
) -> *mut std::ffi::c_void {
    unsafe {
        windows_sys::Win32::Foundation::SetLastError(
            windows_sys::Win32::Foundation::ERROR_FILE_NOT_FOUND,
        );
    }
    windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE
}

fn main() -> std::io::Result<()> {
    install_registered_hooks()?;

    let error = File::open("Cargo.toml").unwrap_err();
    assert_eq!(error.kind(), ErrorKind::NotFound);

    Ok(())
}
```

That is the smallest useful loop:

- declare the hook
- install once
- call normal Rust code and see the replacement take effect

If you also want to observe hook hits, add `#[hook::observer]` and
`#[hook::observe(...)]`. That path is optional, not the starting point.

Some of the design choices behind that feel:

- `hook::c` should describe what to hook, not how installation works
- `hook::com_impl` is the main COM story, so related hooks can live in one
  inherent impl block
- observation is event-oriented and local, not a big retained runtime
- lower-level target types still exist when you need them, but they are not the
  first thing the crate asks you to touch

## Examples

- [`examples/interception_story.rs`](examples/interception_story.rs) shows the lightweight
  "print on hit" observation path in one file while still changing what
  `std::fs::File::open(...)` sees.
- [`examples/screenshot_agent`](examples/screenshot_agent) is a single package
  that carries the injected `cdylib`, a `screenshot_victim` bin, and a
  `screenshot_demo` bin for the full one-command run.

That screenshot demo is the most fun place to start if you want the crate to
feel real. It launches a small victim app that captures the screen with `xcap`,
then injects a hook library that swaps the live screenshot out for a synthetic
frame.

So instead of a toy return-value demo, you get a real before-and-after:

- the victim captures the real desktop first
- the agent gets injected into that running process
- later captures come back as our synthetic frame instead

Run the full screenshot demo like this:

```bash
cargo run --manifest-path examples/screenshot_agent/Cargo.toml --features inject --bin screenshot_demo
```

It builds the victim and agent, starts the victim, lets a few captures succeed,
and then injects the screenshot agent automatically. It is intentionally a
showpiece: one command, one victim, one injected library, one visible change.

This is a live desktop demo rather than a normal CI target. It wants a real
screen-capture environment and the usual OS permissions for screenshot APIs.

If you want to do the two steps yourself, you still can:

```bash
cargo run --manifest-path examples/screenshot_agent/Cargo.toml --bin screenshot_victim
```

Then inject the agent into the printed PID:

```bash
cargo run --manifest-path examples/screenshot_agent/Cargo.toml --features inject --bin inject -- <pid> /tmp/retarget-screenshot-agent.log
```

The screenshot agent is meant to be loaded with [`hook-inject`](https://crates.io/crates/hook-inject).
Its injector path looks like this:

```rust
use hook_inject::{inject_process, Library, Process};
use std::ffi::CString;

let pid = 1234;
let process = Process::from_pid(pid)?;
let log_path = CString::new("/tmp/retarget-screenshot-agent.log")?;

let library = Library::from_crate("examples/screenshot_agent")?
    .with_data(log_path);

let _injected = inject_process(process, library)?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

That keeps the "write hooks in Rust" story and the "load them into another
process" story nicely separate.

Warnings:

- The crate is still experimental, so expect API churn while the surface
  settles.
- Install hooks as early as practical in process startup.
- Anything under `retarget::__macro_support` and generated `__retarget_*`
  names is internal and may change without notice.
