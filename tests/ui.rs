//! Compile-time diagnostics for hook declaration macros.

/// Verifies that hook macro misuse reports actionable diagnostics.
#[test]
fn hook_macro_diagnostics_are_useful() {
    let cases = trybuild::TestCases::new();
    cases.compile_fail("tests/ui/common/*.rs");

    #[cfg(target_os = "macos")]
    cases.compile_fail("tests/ui/macos/*.rs");
}
