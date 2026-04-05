//! Hook registry and generic interception observation state.

#[cfg(target_os = "macos")]
use crate::objc::ObjcMethodError;
use crate::{FunctionError, FunctionReplaceError, Symbol, function::Module};
use linkme::distributed_slice;
use std::collections::BTreeMap;
use std::error::Error;
use std::io;
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

/// One hook-install failure that may represent target absence.
pub trait HookFailure: Error {
    /// Returns whether this error only indicates absence.
    fn is_absent(&self) -> bool;
}

impl HookFailure for FunctionError {
    /// Returns whether this function error only indicates absence.
    fn is_absent(&self) -> bool {
        self.is_absent()
    }
}

impl HookFailure for FunctionReplaceError {
    /// Function replacement failures never mean the target is absent.
    fn is_absent(&self) -> bool {
        false
    }
}

#[cfg(target_os = "macos")]
impl HookFailure for ObjcMethodError {
    /// Returns whether this Objective-C method error only indicates absence.
    fn is_absent(&self) -> bool {
        self.is_absent()
    }
}

/// One registered system-API hook specification.
#[derive(Debug, Clone)]
pub struct HookSpec {
    /// Stable hook display name.
    pub name: &'static str,
    /// Exported symbol or selector text used for installation.
    pub symbol: Symbol,
    /// Module that should contain the hook target when relevant.
    pub module: Option<Module>,
    /// Whether missing support should be tolerated.
    pub optional: bool,
}

/// One proc-macro-registered hook install function.
pub struct HookDef {
    /// Installs one registered hook.
    pub install: fn() -> io::Result<()>,
}

/// One proc-macro-registered interception observer callback.
pub struct InterceptionObserverDef {
    /// Default interception mode for hooks without a local override.
    pub default_mode: InterceptionMode,
    /// Callback invoked for emitted interception events.
    pub callback: fn(InterceptionEvent),
}

/// One proc-macro-registered per-hook interception mode override.
pub struct InterceptionOverrideDef {
    /// Stable internal hook identifier.
    pub hook_id: &'static str,
    /// Interception mode override for the given hook.
    pub mode: InterceptionMode,
}

/// Installs every hook registered in the distributed slice.
pub fn install_registered_hooks() -> io::Result<()> {
    let _ = interception_runtime()?;
    for hook in HOOKS {
        (hook.install)()?;
    }
    Ok(())
}

/// Resolves the target symbol before hook installation begins.
pub fn probe_hook(spec: &HookSpec) -> Result<(), FunctionError> {
    match spec.module.as_ref() {
        Some(module) => spec.symbol.resolve_in(module).map(|_| ()),
        None => spec.symbol.resolve().map(|_| ()),
    }
}

/// Returns an install-time error for one required hook failure.
pub fn finish_install<E: HookFailure>(spec: &HookSpec, result: Result<(), E>) -> io::Result<()> {
    finish_named_install(spec.name, spec.optional, result)
}

/// Returns an install-time error for one named hook failure.
pub fn finish_named_install<E: HookFailure>(
    name: &'static str,
    optional: bool,
    result: Result<(), E>,
) -> io::Result<()> {
    match result {
        Ok(()) => Ok(()),
        Err(error) if optional && error.is_absent() => Ok(()),
        Err(error) => Err(io::Error::other(format!(
            "required hook {name} failed: {error}"
        ))),
    }
}

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

/// One hook's retained interception proof state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InterceptionState {
    /// The hook has not yet been observed at runtime.
    Unobserved,
    /// The hook was observed at least once.
    Observed {
        /// Timestamp of the first observed hook entry in Unix milliseconds.
        first_hit_unix_ms: u64,
        /// Timestamp of the most recent observed hook entry in Unix milliseconds.
        last_hit_unix_ms: u64,
        /// Number of retained observed hook entries.
        count: u64,
    },
}

/// One generic interception observation event.
#[derive(Debug, Clone)]
pub struct InterceptionEvent {
    /// Stable internal hook identifier.
    pub hook_id: &'static str,
    /// Effective interception observation mode for this hook.
    pub mode: InterceptionMode,
    /// Current retained proof state for this hook.
    pub state: InterceptionState,
}

/// Registers one hook id so unobserved state appears in later snapshots.
pub fn register_interception_hook(hook_id: &'static str) {
    if let Ok(mut states) = interception_state_store().lock() {
        states
            .entry(hook_id)
            .or_insert(InterceptionState::Unobserved);
    }
}

/// Records one runtime interception for the given hook id.
///
/// Returns `true` when an interception event was emitted.
pub fn record_interception(hook_id: &'static str) -> bool {
    let Ok(runtime) = interception_runtime() else {
        return false;
    };

    let mode = runtime.effective_mode(hook_id);
    if matches!(mode, InterceptionMode::Off) {
        return false;
    }

    let now_unix_ms = current_unix_ms();
    let emitted = if let Ok(mut states) = interception_state_store().lock() {
        let state = states
            .entry(hook_id)
            .or_insert(InterceptionState::Unobserved);
        match mode {
            InterceptionMode::Off => None,
            InterceptionMode::FirstHit => match state {
                InterceptionState::Unobserved => {
                    *state = InterceptionState::Observed {
                        first_hit_unix_ms: now_unix_ms,
                        last_hit_unix_ms: now_unix_ms,
                        count: 1,
                    };
                    Some(InterceptionEvent {
                        hook_id,
                        mode,
                        state: state.clone(),
                    })
                }
                InterceptionState::Observed { .. } => None,
            },
            InterceptionMode::EveryHit => {
                match state {
                    InterceptionState::Unobserved => {
                        *state = InterceptionState::Observed {
                            first_hit_unix_ms: now_unix_ms,
                            last_hit_unix_ms: now_unix_ms,
                            count: 1,
                        };
                    }
                    InterceptionState::Observed {
                        last_hit_unix_ms,
                        count,
                        ..
                    } => {
                        *last_hit_unix_ms = now_unix_ms;
                        *count = count.saturating_add(1);
                    }
                }
                Some(InterceptionEvent {
                    hook_id,
                    mode,
                    state: state.clone(),
                })
            }
        }
    } else {
        None
    };

    if let Some(event) = emitted {
        if let Some(callback) = runtime.callback {
            callback(event);
        }
        return true;
    }

    false
}

/// Returns the current interception proof snapshot.
pub fn interception_snapshot() -> Vec<InterceptionEvent> {
    let runtime = interception_runtime().ok();
    interception_state_store()
        .lock()
        .map(|states| {
            states
                .iter()
                .map(|(hook_id, state)| InterceptionEvent {
                    hook_id,
                    mode: runtime
                        .as_ref()
                        .map(|value| value.effective_mode(hook_id))
                        .unwrap_or(InterceptionMode::Off),
                    state: state.clone(),
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Distributed slice of every hook install function registered in one target crate.
#[distributed_slice]
pub static HOOKS: [HookDef];

/// Distributed slice of every interception observer registered in one target crate.
#[distributed_slice]
pub static INTERCEPTION_OBSERVERS: [InterceptionObserverDef];

/// Distributed slice of every per-hook interception override registered in one target crate.
#[distributed_slice]
pub static INTERCEPTION_OVERRIDES: [InterceptionOverrideDef];

static INTERCEPTION_RUNTIME: OnceLock<InterceptionRuntime> = OnceLock::new();
static INTERCEPTION_STATES: OnceLock<Mutex<BTreeMap<&'static str, InterceptionState>>> =
    OnceLock::new();

struct InterceptionRuntime {
    default_mode: InterceptionMode,
    callback: Option<fn(InterceptionEvent)>,
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

fn interception_state_store() -> &'static Mutex<BTreeMap<&'static str, InterceptionState>> {
    INTERCEPTION_STATES.get_or_init(|| Mutex::new(BTreeMap::new()))
}

fn current_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::{HookSpec, finish_named_install, probe_hook};
    use crate::function::{into_module, into_symbol};

    #[cfg(target_os = "windows")]
    fn test_module_name() -> &'static str {
        "kernel32.dll"
    }

    #[cfg(target_os = "macos")]
    fn test_module_name() -> &'static str {
        "/usr/lib/libSystem.B.dylib"
    }

    #[cfg(target_os = "windows")]
    fn test_symbol_name() -> &'static str {
        "GetCurrentProcessId"
    }

    #[cfg(target_os = "macos")]
    fn test_symbol_name() -> &'static str {
        "getpid"
    }

    #[test]
    fn probe_hook_resolves_global_exports() {
        let spec = HookSpec {
            name: "global-test",
            symbol: into_symbol(test_symbol_name()).expect("valid symbol"),
            module: None,
            optional: false,
        };

        probe_hook(&spec).expect("global export should resolve");
    }

    #[test]
    fn probe_hook_resolves_scoped_exports() {
        let spec = HookSpec {
            name: "scoped-test",
            symbol: into_symbol(test_symbol_name()).expect("valid symbol"),
            module: Some(into_module(test_module_name()).expect("valid module")),
            optional: false,
        };

        probe_hook(&spec).expect("scoped export should resolve");
    }

    #[test]
    fn probe_hook_reports_missing_exports() {
        let spec = HookSpec {
            name: "missing-test",
            symbol: into_symbol("DefinitelyMissingExport").expect("valid symbol"),
            module: Some(into_module(test_module_name()).expect("valid module")),
            optional: false,
        };

        let error = probe_hook(&spec).expect_err("missing export should fail");
        assert_eq!(
            error.to_string(),
            format!(
                "function '{}' was not found in module '{}'",
                "DefinitelyMissingExport",
                test_module_name()
            )
        );
    }

    #[test]
    fn finish_named_install_ignores_optional_absence() {
        let spec = HookSpec {
            name: "missing-optional-test",
            symbol: into_symbol("DefinitelyMissingExport").expect("valid symbol"),
            module: Some(into_module(test_module_name()).expect("valid module")),
            optional: true,
        };

        let result = probe_hook(&spec);
        finish_named_install(spec.name, spec.optional, result).expect("optional miss is allowed");
    }

    #[test]
    fn finish_named_install_reports_required_hook_name_and_error() {
        let result = finish_named_install(
            "missing-required-test",
            false,
            probe_hook(&HookSpec {
                name: "missing-required-test",
                symbol: into_symbol("DefinitelyMissingExport").expect("valid symbol"),
                module: Some(into_module(test_module_name()).expect("valid module")),
                optional: false,
            }),
        )
        .expect_err("required missing hook should report an install error");

        assert_eq!(
            result.to_string(),
            format!(
                "required hook missing-required-test failed: function '{}' was not found in module '{}'",
                "DefinitelyMissingExport",
                test_module_name()
            )
        );
    }
}
