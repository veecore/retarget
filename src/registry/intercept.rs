//! Generic interception observation state and callbacks.

use linkme::distributed_slice;
use std::any::TypeId;
use std::collections::BTreeMap;
use std::io;
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

/// One installed hook's interception observation mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InterceptionMode {
    /// Do not retain proof state or emit callbacks for this hook.
    Off,
    /// Emit only the first observed interception for this hook.
    FirstHit,
    /// Emit every observed interception for this hook.
    EveryHit,
}

/// One direct interception hit emitted to the observer callback.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InterceptionHit {
    /// Stable internal hook identifier.
    pub hook_id: &'static str,
    /// Effective interception observation mode for this hook.
    pub mode: InterceptionMode,
    /// Timestamp of this observed hook entry in Unix milliseconds.
    pub unix_ms: u64,
}

/// Back-compat alias for one interception hit.
pub type InterceptionEvent = InterceptionHit;

/// Shorthand for [`InterceptionMode`] under [`crate::intercept`].
pub type Mode = InterceptionMode;

/// Shorthand for one direct interception hit under [`crate::intercept`].
pub type Event = InterceptionHit;

/// Shorthand for one direct interception hit under [`crate::intercept`].
pub type Hit = InterceptionHit;

/// One typed interception signal emitted to one crate-wide observer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Signal<T> {
    /// Metadata about the intercepted hook entry.
    pub event: Event,
    /// One cloneable signal value chosen by the hook declaration.
    pub value: T,
}

/// Re-export common interception modes for attribute ergonomics.
pub use InterceptionMode::{EveryHit, FirstHit, Off};

/// One runtime observation callback shape registered by one observer macro.
#[doc(hidden)]
#[derive(Clone, Copy)]
pub enum InterceptionObserverCallback {
    /// One observer that only receives raw interception events.
    Event(fn(InterceptionHit)),
    /// One observer that receives one concrete signal type for the whole crate.
    Signal {
        /// Returns the concrete signal type identifier.
        type_id: fn() -> TypeId,
        /// Returns the concrete signal type name for diagnostics.
        type_name: fn() -> &'static str,
        /// Emits one accepted event plus one typed signal value.
        emit: unsafe fn(InterceptionHit, *const ()),
    },
}

/// One proc-macro-registered interception observer callback.
#[doc(hidden)]
pub struct InterceptionObserverDef {
    /// Default interception mode for hooks without a local override.
    pub default_mode: InterceptionMode,
    /// Callback invoked for emitted interception events.
    pub callback: InterceptionObserverCallback,
}

/// One proc-macro-registered per-hook interception mode override.
#[doc(hidden)]
pub struct InterceptionOverrideDef {
    /// Stable internal hook identifier.
    pub hook_id: &'static str,
    /// Interception mode override for the given hook.
    pub mode: InterceptionMode,
}

/// One proc-macro-registered per-hook signal type declaration.
#[doc(hidden)]
pub struct InterceptionSignalDef {
    /// Stable internal hook identifier.
    pub hook_id: &'static str,
    /// Returns the hook's concrete signal type identifier.
    pub type_id: fn() -> TypeId,
    /// Returns the hook's concrete signal type name for diagnostics.
    pub type_name: fn() -> &'static str,
}

/// Distributed slice of every interception observer registered in one target crate.
#[doc(hidden)]
#[distributed_slice]
pub static INTERCEPTION_OBSERVERS: [InterceptionObserverDef];

/// Distributed slice of every per-hook interception override registered in one target crate.
#[doc(hidden)]
#[distributed_slice]
pub static INTERCEPTION_OVERRIDES: [InterceptionOverrideDef];

/// Distributed slice of every hook that emits one typed observation signal.
#[doc(hidden)]
#[distributed_slice]
pub static INTERCEPTION_SIGNALS: [InterceptionSignalDef];

static INTERCEPTION_RUNTIME: OnceLock<InterceptionRuntime> = OnceLock::new();

struct InterceptionRuntime {
    default_mode: InterceptionMode,
    callback: Option<InterceptionObserverCallback>,
    overrides: BTreeMap<&'static str, InterceptionMode>,
}

impl InterceptionRuntime {
    fn effective_mode(&self, hook_id: &'static str) -> InterceptionMode {
        self.overrides
            .get(hook_id)
            .copied()
            .unwrap_or(self.default_mode)
    }
}

pub(crate) fn prepare_interception_runtime() -> io::Result<()> {
    let _ = interception_runtime()?;
    Ok(())
}

/// Records one runtime interception for the given hook id.
///
/// Returns `true` when an interception event was emitted.
#[doc(hidden)]
pub fn record_interception(hook_id: &'static str, first_hit: &'static OnceLock<()>) -> bool {
    let Some(event) = next_interception(hook_id, first_hit) else {
        return false;
    };
    dispatch_interception(event);
    true
}

/// Returns the next interception event when this hook hit should be emitted.
#[doc(hidden)]
pub fn next_interception(
    hook_id: &'static str,
    first_hit: &'static OnceLock<()>,
) -> Option<InterceptionHit> {
    let Ok(runtime) = interception_runtime() else {
        return None;
    };
    next_hit(runtime, hook_id, first_hit)
}

/// Dispatches one already-accepted interception event to the registered observer.
#[doc(hidden)]
pub fn dispatch_interception(event: InterceptionHit) {
    let Ok(runtime) = interception_runtime() else {
        return;
    };

    if let Some(InterceptionObserverCallback::Event(callback)) = runtime.callback {
        callback(event);
    }
}

/// Dispatches one already-accepted interception event plus one typed signal value.
#[doc(hidden)]
pub fn dispatch_signal<T: Clone + 'static>(event: InterceptionHit, value: T) {
    let Ok(runtime) = interception_runtime() else {
        return;
    };

    match runtime.callback {
        Some(InterceptionObserverCallback::Event(callback)) => callback(event),
        Some(InterceptionObserverCallback::Signal {
            type_id,
            type_name,
            emit,
        }) => {
            if type_id() != TypeId::of::<T>() {
                panic!(
                    "hook::observe payload type mismatch: observer expects `{}`, but hook emitted `{}`",
                    type_name(),
                    std::any::type_name::<T>(),
                );
            }
            unsafe {
                emit(event, (&value as *const T).cast());
            }
        }
        None => {}
    }
}

fn interception_runtime() -> io::Result<&'static InterceptionRuntime> {
    if let Some(runtime) = INTERCEPTION_RUNTIME.get() {
        return Ok(runtime);
    }

    let mut overrides = BTreeMap::new();
    for override_def in INTERCEPTION_OVERRIDES {
        overrides.insert(override_def.hook_id, override_def.mode);
    }

    let mut observers = INTERCEPTION_OBSERVERS.iter();
    let first = observers.next();
    if observers.next().is_some() {
        return Err(io::Error::other(
            "multiple hook observers were registered; only one is supported",
        ));
    }

    if let Some(observer) = first
        && let InterceptionObserverCallback::Signal {
            type_id,
            type_name,
            ..
        } = observer.callback
    {
        for signal in INTERCEPTION_SIGNALS {
            if (signal.type_id)() != type_id() {
                return Err(io::Error::other(format!(
                    "hook::observe payload type mismatch for {}: observer expects `{}`, but hook emits `{}`",
                    signal.hook_id,
                    type_name(),
                    (signal.type_name)(),
                )));
            }
        }
    }

    let runtime = match first {
        Some(observer) => InterceptionRuntime {
            default_mode: observer.default_mode,
            callback: Some(observer.callback),
            overrides,
        },
        None => InterceptionRuntime {
            default_mode: InterceptionMode::Off,
            callback: None,
            overrides,
        },
    };

    Ok(INTERCEPTION_RUNTIME.get_or_init(|| runtime))
}

fn next_hit(
    runtime: &InterceptionRuntime,
    hook_id: &'static str,
    first_hit: &'static OnceLock<()>,
) -> Option<InterceptionHit> {
    let mode = runtime.effective_mode(hook_id);
    match mode {
        InterceptionMode::Off => None,
        InterceptionMode::FirstHit => {
            if first_hit.set(()).is_err() {
                return None;
            }
            Some(InterceptionHit {
                hook_id,
                mode,
                unix_ms: current_unix_ms(),
            })
        }
        InterceptionMode::EveryHit => Some(InterceptionHit {
            hook_id,
            mode,
            unix_ms: current_unix_ms(),
        }),
    }
}

fn current_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::{InterceptionMode, InterceptionRuntime, next_hit};
    use std::collections::BTreeMap;
    use std::sync::OnceLock;

    fn runtime(default_mode: InterceptionMode) -> InterceptionRuntime {
        InterceptionRuntime {
            default_mode,
            callback: None,
            overrides: BTreeMap::new(),
        }
    }

    #[test]
    fn first_hit_mode_emits_once_per_hook() {
        static FIRST_HIT: OnceLock<()> = OnceLock::new();
        let runtime = runtime(InterceptionMode::FirstHit);

        assert!(next_hit(&runtime, "demo::hook", &FIRST_HIT).is_some());
        assert!(next_hit(&runtime, "demo::hook", &FIRST_HIT).is_none());
    }

    #[test]
    fn every_hit_mode_emits_every_time() {
        static FIRST_HIT: OnceLock<()> = OnceLock::new();
        let runtime = runtime(InterceptionMode::EveryHit);

        assert!(next_hit(&runtime, "demo::hook", &FIRST_HIT).is_some());
        assert!(next_hit(&runtime, "demo::hook", &FIRST_HIT).is_some());
    }

    #[test]
    fn off_mode_suppresses_hits() {
        static FIRST_HIT: OnceLock<()> = OnceLock::new();
        let runtime = runtime(InterceptionMode::Off);

        assert!(next_hit(&runtime, "demo::hook", &FIRST_HIT).is_none());
    }
}
