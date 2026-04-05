//! macOS tests for public hook probing behavior.

use retarget::{HookSpec, finish_named_install, into_module, into_symbol, probe_hook};

fn test_module_name() -> &'static str {
    "/usr/lib/libSystem.B.dylib"
}

fn test_symbol_name() -> &'static str {
    "getpid"
}

/// Verifies that public probing resolves process-global exports.
#[test]
fn resolves_global_exports() {
    let spec = HookSpec {
        name: "global-probe-test",
        symbol: into_symbol(test_symbol_name()).expect("expected symbol"),
        module: None,
        optional: false,
    };

    probe_hook(&spec).expect("expected global export to resolve");
}

/// Verifies that public probing resolves module-scoped exports.
#[test]
fn resolves_scoped_exports() {
    let spec = HookSpec {
        name: "scoped-probe-test",
        symbol: into_symbol(test_symbol_name()).expect("expected symbol"),
        module: Some(into_module(test_module_name()).expect("expected module")),
        optional: false,
    };

    probe_hook(&spec).expect("expected scoped export to resolve");
}

/// Verifies that optional missing probes stay non-fatal through the public finish path.
#[test]
fn allows_optional_missing_exports() {
    let spec = HookSpec {
        name: "optional-missing-probe-test",
        symbol: into_symbol("DefinitelyMissingExport").expect("expected symbol"),
        module: Some(into_module(test_module_name()).expect("expected module")),
        optional: true,
    };

    let result = probe_hook(&spec);
    finish_named_install(spec.name, spec.optional, result)
        .expect("expected optional missing export to be tolerated");
}
