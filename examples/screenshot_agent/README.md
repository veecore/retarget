# Screenshot Agent

This is the flashiest demo package in the repo.

It hooks public screenshot APIs that many binaries use:

- macOS: `CGWindowListCreateImage`
- Windows: `BitBlt`

That keeps the demo generic. The paired victim happens to use `xcap`, but the
agent itself is aimed at screenshot APIs many other binaries call too.

When the hook takes effect, it does not just fail the capture. It swaps in a
synthetic frame instead and writes a log line for each hit, which makes the
result much more obvious than a silent failure.

The library is meant to be loaded into another process with
[`hook-inject`](https://crates.io/crates/hook-inject).

This package also carries two helper binaries:

- `screenshot_victim`, which uses `xcap` to capture in a loop
- `screenshot_demo`, which launches the victim and injects the agent for you

If you want the shortest path, use:

```bash
cargo run --manifest-path examples/screenshot_agent/Cargo.toml --features inject --bin screenshot_demo
```

You should see a few real captures first, then the victim switches over to a
smaller synthetic frame with a bright top-left pixel once the injection lands.
That moment is the whole point of the demo: the process does not restart, the
victim code does not change, and yet the returned screenshot data is now ours.

## Run It

Run the victim by itself:

```bash
cargo run --manifest-path examples/screenshot_agent/Cargo.toml --bin screenshot_victim
```

Then build and inject into the printed process id:

```bash
cargo run --manifest-path examples/screenshot_agent/Cargo.toml --features inject --bin inject -- <pid>
```

Optionally pass a log file path:

```bash
cargo run --manifest-path examples/screenshot_agent/Cargo.toml --features inject --bin inject -- <pid> /tmp/retarget-screenshot-agent.log
```

If you omit the log path, the injector chooses one in your temp directory.

## What It Shows

- `retarget` hook declarations inside a `cdylib`
- installing hooks from an injected entrypoint
- targeting public screenshot APIs instead of crate-specific helpers
- using `hook-inject::Library::from_crate` so the injector can discover and
  build the agent library for you
- giving a live desktop app different image data without changing its code
