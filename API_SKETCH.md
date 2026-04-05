# `retarget` API Sketch

This is the current UX target for the standalone hook crate.

It intentionally favors:

- fewer macros
- typed macro arguments
- grouped object hooks
- less raw pointer exposure
- one obvious story per hook kind

## Design Stance

The crate should support three public hook families:

- `hook::c`
- `hook::com`
- `hook::objc::class`
- `hook::objc::instance`

The important distinction is:

- `c` hooks target exported functions
- `com` hooks target interface vtable fields
- `objc` hooks target Objective-C methods

Those are different user concepts, so the API should say that clearly.

## Core Rules

### 1. Macro arguments must be typed

We should not make users pass raw pointers as macro arguments.

Good:

- `interface = com::of::<IStream>()`
- `instance = test_stream()`
- `class = NSWorkspace`
- `selector = objc::selector!(sharedWorkspace)`
- `image = images::kernel32()`
- `symbol = symbols::named("GetCurrentProcessId")`

Bad:

- `resolve = unsafe { ... raw pointer math ... }`
- `instance = some_ptr as *mut c_void`
- `class = "NSWorkspace"` as the main path

Raw-pointer and raw-string escape hatches can exist, but they should not be
the first-class public story.

### 2. Group object hooks behind `impl`

Object hooks should be declared as groups.

If the user only wants one method, they can still write a one-method `impl`.

That is still clearer than asking them to describe object context again on
every free function.

### 3. Per-method attributes are overrides, not the main story

The outer decorator should carry the shared context.

The method-level decorator should only exist for exceptions such as:

- the Rust method name differs from the target method name
- the target method is optional
- the method wants a custom fallback

## Public Layout

The root should feel small:

```rust
use retarget::{
    hook,
    intercept,
    images,
    symbols,
    install_registered_hooks,
};

use retarget::intercept::{Event, Mode};
```

Support modules should also exist at the root:

```rust
use retarget::{com, objc};
```

Those modules should provide typed helpers and traits, not low-level patching
APIs.

Examples:

- `images::kernel32()`
- `objc::selector!(sharedWorkspace)`
- `objc::named_class("MyCustomClass")`
- `com::of::<IStream>()`

The backend patch engines stay internal.

## C Hook Story

`hook::c` stays a free-function hook because that matches the underlying
surface.

```rust
#[hook::c(("kernel32.dll", "GetCurrentProcessId"))]
unsafe extern "system" fn get_current_process_id() -> u32 {
    forward!() + 1
}
```

The macro should accept:

- no argument when the Rust function name already matches
- one positional target expression
- optional `name`
- optional `fallback`
- optional `optional`

The positional target should still be typed internally.

That means the public path should be something like:

- `symbols::named("GetCurrentProcessId")`

But convenience should still matter.

So plain values like:

- `"GetCurrentProcessId"`
- `("kernel32.dll", "GetCurrentProcessId")`

should be accepted through an `IntoSymbol` conversion into one opaque
function target.

`name` should only be a display override for probe/install/observation output.
If omitted, the crate should derive it from the symbol or, failing that, from
the Rust function name.

It should not expose backend-install choices.

The crate should try the applicable strategy for the platform.

## COM Hook Story

This is the main Windows object-hook story.

The public UX should be impl-based only.

The current free-function `hook::com(...)` shape should not be public at all.

### Outer COM Decorator

```rust
#[hook::com(interface = com::of::<IStream>(), instance = test_stream())]
impl StreamHooks {
    unsafe extern "system" fn commit(this: *mut c_void, flags: u32) -> HRESULT {
        HRESULT(E_FAIL.0)
    }
}
```

The outer decorator owns:

- `interface`
- `instance`

Meaning:

- `interface`: one Rust interface type
- `instance`: one expression that yields an instance of that interface

The important bit is that `instance` is typed.

The important bit is that `interface` should also be converted into one opaque
`ComInterface`.

That means the public API should rely on conversion traits, not on public
pointer-returning traits.

### COM Method Defaults

Inside the impl block:

- every method is a hook by default
- the Rust method name is converted to PascalCase
- the PascalCase name is treated as the vtable field name
- display name becomes `Interface::Field`

So:

```rust
unsafe extern "system" fn commit(...) -> HRESULT
```

maps to:

- field `Commit`
- display name `IStream::Commit`

### COM Method Overrides

Overrides should use one small method decorator:

```rust
#[hook::com_method(SetSize)]
unsafe extern "system" fn resize(...) -> HRESULT {
    HRESULT(E_FAIL.0)
}
```

That is clearer than putting another full `hook::com(...)` block on the
method.

Method overrides should support:

- `field`
- `name`
- `optional`
- `fallback`

Not:

- `resolve`
- `interface`
- `instance`

Those belong to the outer decorator.

### COM Custom Interface Story

Users should be able to hook their own interfaces too.

The public API should only need conversion traits and opaque values:

- `IntoComInterface`
- `IntoComInstance`

Those conversions should produce:

```rust
ComInterface
ComInstance
```

Those opaque types can embed raw pointers or resolved metadata internally.
That does not mean we expose raw pointers publicly.

This is the key distinction:

- raw pointer storage internally is fine
- raw pointer authoring in macro arguments is not

`IStream` in the examples should therefore appear through one helper:

```rust
com::of::<IStream>()
```

not as a naked type token in the public docs.

That helper can internally use whatever platform integration we need, but the
crate does not need to expose `windows_core` as part of the user-facing story.

So for the common system path, the pair:

- `interface = com::of::<IStream>()`
- `instance = test_stream()`

is enough to resolve the method from the instance vtable field. We do not need
to force users to also spell a DLL/image there.

One important constraint:

- a field-based COM hook like `#[hook::com_method(SetSize)]`
  needs compile-time vtable shape somewhere

So a loose string like `"IStream"` is not enough by itself for the polished
field-based path.

If users want a string-based or image-based escape hatch later, that likely
belongs to a more advanced mode where they also provide extra resolution
details such as slot information.

For example:

```rust
#[hook::com_method(slot = 8)]
```

But that should not be the main story.

For custom interfaces with known Rust bindings, the pleasant path is still:

```rust
interface = com::of::<MyInterface>()
```

If we later want named/interface-image conversions, one typed conversion path
like this is fine:

```rust
(images::ole32(), com::named::<MyInterface>("IMyInterface"))
```

but only if the interface descriptor still carries enough vtable-shape
information for field hooks.

That keeps the macro arguments typed for both:

- system APIs
- user-defined interfaces

### COM Example

```rust
use retarget::{hook, install_registered_hooks};
use windows::Win32::System::Com::IStream;

struct StreamHooks;

#[hook::com(interface = com::of::<IStream>(), instance = test_stream())]
impl StreamHooks {
    unsafe extern "system" fn commit(this: *mut c_void, flags: u32) -> HRESULT {
        let _ = (this, flags);
        HRESULT(0x80004005u32 as i32)
    }

    #[hook::com_method(SetSize)]
    unsafe extern "system" fn resize(this: *mut c_void, new_size: u64) -> HRESULT {
        let _ = (this, new_size);
        HRESULT(0x80004005u32 as i32)
    }
}

install_registered_hooks()?;
```

This is the shape we should optimize around.

## Objective-C Hook Story

Objective-C should mirror the grouped object-hook shape.

The difference is that Objective-C needs:

- class-method hooks
- instance-method hooks

So the public story should be:

- `hook::objc::class`
- `hook::objc::instance`

and both should decorate `impl` blocks.

### Outer ObjC Decorators

Class methods:

```rust
#[hook::objc::class(class = NSWorkspace)]
impl WorkspaceClassHooks {
    unsafe fn shared_workspace() -> Retained<NSWorkspace> {
        forward!()
    }
}
```

Instance methods:

```rust
#[hook::objc::instance(class = NSWorkspace)]
impl WorkspaceInstanceHooks {
    unsafe fn frontmost_application(
        this: &NSWorkspace,
    ) -> Option<Retained<NSRunningApplication>> {
        forward!()
    }
}
```

The outer decorator owns:

- `class`

The common path should be typed:

- `class = NSWorkspace`

That works well for system APIs and for user classes that have Rust bindings.

The escape hatch can still exist:

```rust
#[hook::objc::instance(class = objc::named_class("MyCustomClass"))]
```

### ObjC Method Defaults

Like COM, every method in the impl block is a hook by default.

The default selector should be derived from the Rust method name only for the
simple case:

- `shared_workspace` -> `sharedWorkspace`
- `frontmost_application` -> `frontmostApplication`

That covers the delightful common path.

### ObjC Method Overrides

For anything less obvious, the method-level override should be explicit.

The exact ObjC method override name is still open for now because we are
deferring ObjC API cleanup in this pass.

```rust
#[hook::objc_method(selector = objc::selector!(URLForApplicationWithBundleIdentifier:))]
unsafe fn url_for_application(...) -> Option<Retained<NSURL>> {
    forward!()
}
```

This decorator should also support:

- `name`
- `optional`
- `fallback`

The important part is just that ObjC also gets a small override decorator
rather than repeating the full outer macro on every method.

### ObjC Example

```rust
use retarget::{hook, objc};
use objc2_app_kit::{NSRunningApplication, NSWorkspace};
use objc2_foundation::Retained;

struct WorkspaceHooks;

#[hook::objc::class(class = NSWorkspace)]
impl WorkspaceHooks {
    unsafe fn shared_workspace() -> Retained<NSWorkspace> {
        forward!()
    }
}

#[hook::objc::instance(class = NSWorkspace)]
impl WorkspaceHooks {
    unsafe fn frontmost_application(
        this: &NSWorkspace,
    ) -> Option<Retained<NSRunningApplication>> {
        forward!()
    }

    #[hook::objc_method(selector = objc::selector!(URLForApplicationWithBundleIdentifier:))]
    unsafe fn url_for_application(...) -> Retained<objc2_foundation::NSURL> {
        forward!()
    }
}
```

That is the shape I think we should target.

## Interception Story

Interception should stay separate from hook shape.

The good UX is still:

```rust
#[hook::observer(default = Mode::FirstHit)]
fn on_interception(event: Event) {}
```

with rare per-hook overrides:

```rust
#[hook::observe(Mode::EveryHit)]
#[hook::com_method(Present)]
unsafe extern "system" fn present(...) -> HRESULT {
    forward!()
}
```

or:

```rust
#[hook::observe(Mode::EveryHit)]
#[hook::c("GetCursorPos")]
unsafe extern "system" fn get_cursor_pos(...) -> BOOL {
    forward!()
}
```

The important point is that observation is not the main story of the hook
declaration.

## Install Story

Install can still be:

- explicit
- registry-based

Explicit:

```rust
MyHooks::install()?;
```

Registry:

```rust
install_registered_hooks()?;
```

The grouped object-hook APIs fit registry mode well because the user declares
all related methods together.

## What We Should Drop

We should stop optimizing the public API around:

- raw pointer expressions in macro arguments
- free-function COM hooks as the primary story
- method-level mini copies of the full outer macro
- stringly typed ObjC class names as the default
- resolver boilerplate as part of hook authoring

Those can still exist as power-user escape hatches later.

They should not define the crate’s personality.

## Recommended Next Step

Before more implementation work, the crate should be reshaped around this
public surface:

1. keep `hook::c`
2. make impl-style `hook::com` the only public COM story
3. add `hook::com_method` for COM overrides
4. redesign ObjC around impl-block hooks too
5. keep raw-address and resolver APIs internal or clearly advanced

## Nice-To-Have Backlog

- teach the `Function` conversion path to recover `module + symbol` from one raw
  function address when the platform can do that cleanly
- teach the Objective-C path to recover one typed `ObjcMethod` from one raw
  runtime method pointer for advanced power users

Those are convenience improvements, not the main public story.
