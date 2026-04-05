use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

/// Builds native test fixtures and backend support for the standalone hook crate.
fn main() {
    println!("cargo:rerun-if-changed=build.rs");

    let target = env::var("TARGET").unwrap_or_default();
    if target.contains("apple-darwin") || target.contains("linux") {
        build_vendored_dobby(&target);
    }
    if target.contains("apple-darwin") {
        build_macos_test_dylib();
    }

    let host = env::var("HOST").unwrap_or_default();
    if target.contains("windows") && host.contains("windows") {
        build_windows_test_static_lib();
        build_windows_test_dlls();
    }

    if !target.contains("windows") {
        return;
    }

    if !host.contains("windows") {
        println!("cargo:warning=skipping Detours native build on non-Windows host");
        return;
    }

    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let detours_root = manifest_dir.join("vendor").join("detours");
    let detours_src = detours_root.join("src");
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let detours_support = out_dir.join("detours_support.cpp");
    fs::write(&detours_support, detours_support_source())
        .expect("failed to write generated Detours support shim");

    for rel in [
        "README.md",
        "src/detours.cpp",
        "src/modules.cpp",
        "src/disasm.cpp",
        "src/image.cpp",
        "src/creatwth.cpp",
        "src/disolx86.cpp",
        "src/disolx64.cpp",
        "src/disolia64.cpp",
        "src/disolarm.cpp",
        "src/disolarm64.cpp",
        "src/detours.h",
        "src/detver.h",
    ] {
        println!(
            "cargo:rerun-if-changed={}",
            detours_root.join(rel).display()
        );
    }
    let mut build = cc::Build::new();
    build.cpp(true);
    build.include(&detours_src);
    build.define("DETOURS_INTERNAL", None);
    build.define("WIN32_LEAN_AND_MEAN", None);
    build.define("_WIN32_WINNT", Some("0x0A00"));

    for file in [
        "detours.cpp",
        "modules.cpp",
        "disasm.cpp",
        "image.cpp",
        "creatwth.cpp",
        "disolx86.cpp",
        "disolx64.cpp",
        "disolia64.cpp",
        "disolarm.cpp",
        "disolarm64.cpp",
    ] {
        build.file(detours_src.join(file));
    }
    build.file(detours_support);

    if target.contains("msvc") {
        build.flag_if_supported("/EHsc");
        build.flag_if_supported("/Zl");
    }

    build.compile("detours_support");
}

/// Returns the generated Windows Detours support shim.
fn detours_support_source() -> &'static str {
    r#"
// Tiny Detours helpers that keep transaction details out of Rust.

#include "detours.h"

extern "C" LONG WINAPI HookDetourAttach(_Inout_ PVOID *ppPointer,
                                        _In_ PVOID pDetour) {
  LONG status = DetourTransactionBegin();
  if (status != NO_ERROR) {
    return status;
  }

  status = DetourUpdateThread(GetCurrentThread());
  if (status != NO_ERROR) {
    DetourTransactionAbort();
    return status;
  }

  status = DetourAttach(ppPointer, pDetour);
  if (status != NO_ERROR) {
    DetourTransactionAbort();
    return status;
  }

  return DetourTransactionCommit();
}
"#
}

/// Builds one vendored Dobby static library for Unix targets.
fn build_vendored_dobby(target: &str) {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let source_dir = manifest_dir.join("vendor").join("dobby");
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let build_dir = out_dir.join("dobby-build");
    let lib_dir = out_dir.join("dobby-lib");

    for rel in [
        "CMakeLists.txt",
        "include/dobby.h",
        "source/dobby.cpp",
        "source/Interceptor.h",
        "source/InterceptRouting/InlineHookRouting.h",
        "source/InterceptRouting/InstrumentRouting.h",
        "builtin-plugin/SymbolResolver/CMakeLists.txt",
        "builtin-plugin/SymbolResolver/dobby_symbol_resolver.h",
        "builtin-plugin/SymbolResolver/macho/dobby_symbol_resolver.cc",
        "builtin-plugin/SymbolResolver/elf/dobby_symbol_resolver.cc",
        "source/Backend/UserMode/PlatformUtil/Darwin/ProcessRuntime.cc",
        "source/Backend/UserMode/PlatformUtil/Linux/ProcessRuntime.cc",
    ] {
        println!("cargo:rerun-if-changed={}", source_dir.join(rel).display());
    }

    // `cargo publish` verifies from `target/package/...`, so stale CMake caches
    // from an earlier source root must be cleared before reconfiguring.
    if build_dir.exists() {
        fs::remove_dir_all(&build_dir).expect("failed to reset Dobby build directory");
    }
    if lib_dir.exists() {
        fs::remove_dir_all(&lib_dir).expect("failed to reset Dobby library output directory");
    }
    fs::create_dir_all(&build_dir).expect("failed to create Dobby build directory");
    fs::create_dir_all(&lib_dir).expect("failed to create Dobby library output directory");

    let mut configure = Command::new("cmake");
    configure
        .arg("-S")
        .arg(&source_dir)
        .arg("-B")
        .arg(&build_dir)
        .arg("-DCMAKE_BUILD_TYPE=Release")
        .arg(format!(
            "-DCMAKE_ARCHIVE_OUTPUT_DIRECTORY={}",
            lib_dir.display()
        ))
        .arg(format!(
            "-DCMAKE_LIBRARY_OUTPUT_DIRECTORY={}",
            lib_dir.display()
        ))
        .arg("-DDOBBY_DEBUG=OFF")
        .arg("-DDOBBY_BUILD_EXAMPLE=OFF")
        .arg("-DDOBBY_BUILD_TEST=OFF")
        .arg("-DPlugin.SymbolResolver=ON");

    if target.contains("apple-darwin") {
        configure.arg("-DPlugin.ImportTableReplace=ON");

        match env::var("CARGO_CFG_TARGET_ARCH")
            .unwrap_or_default()
            .as_str()
        {
            "aarch64" => {
                configure.arg("-DCMAKE_OSX_ARCHITECTURES=arm64");
            }
            "x86_64" => {
                configure.arg("-DCMAKE_OSX_ARCHITECTURES=x86_64");
            }
            _ => {}
        }
    }

    run_command(configure, "failed to configure vendored Dobby");

    let mut build = Command::new("cmake");
    build
        .arg("--build")
        .arg(&build_dir)
        .arg("--config")
        .arg("Release")
        .arg("--target")
        .arg("dobby_static");
    run_command(build, "failed to build vendored Dobby");

    println!("cargo:rustc-link-search=native={}", lib_dir.display());
    println!("cargo:rustc-link-lib=static=dobby");

    if target.contains("apple-darwin") {
        println!("cargo:rustc-link-lib=dylib=c++");
    } else if target.contains("linux") {
        println!("cargo:rustc-link-lib=dylib=stdc++");
        println!("cargo:rustc-link-lib=dylib=dl");
    }
}

/// Compiles a tiny exported test dylib used by macOS hook integration tests.
fn build_macos_test_dylib() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let target_fixture = manifest_dir
        .join("tests")
        .join("fixtures")
        .join("hook_test_target.c");
    let caller_fixture = manifest_dir
        .join("tests")
        .join("fixtures")
        .join("hook_test_caller.c");
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let target_output = out_dir.join("libhook_test_target.dylib");
    let caller_output = out_dir.join("libhook_test_caller.dylib");

    println!("cargo:rerun-if-changed={}", target_fixture.display());
    println!("cargo:rerun-if-changed={}", caller_fixture.display());

    let target_status = Command::new("cc")
        .arg("-dynamiclib")
        .arg(&target_fixture)
        .arg(format!("-Wl,-install_name,{}", target_output.display()))
        .arg("-o")
        .arg(&target_output)
        .status()
        .expect("failed to launch cc for macOS hook test dylib");

    if !target_status.success() {
        panic!("failed to build macOS hook target dylib");
    }

    let caller_status = Command::new("cc")
        .arg("-dynamiclib")
        .arg(&caller_fixture)
        .arg(format!("-Wl,-install_name,{}", caller_output.display()))
        .arg("-L")
        .arg(&out_dir)
        .arg("-lhook_test_target")
        .arg("-Wl,-rpath")
        .arg("-Wl")
        .arg(&out_dir)
        .arg("-o")
        .arg(&caller_output)
        .status()
        .expect("failed to launch cc for macOS hook caller dylib");

    if !caller_status.success() {
        panic!("failed to build macOS hook caller dylib");
    }

    println!(
        "cargo:rustc-env=BLINDER_HOOK_TEST_DYLIB={}",
        target_output.display()
    );
    println!(
        "cargo:rustc-env=BLINDER_HOOK_TEST_CALLER_DYLIB={}",
        caller_output.display()
    );
    println!("cargo:rustc-link-search=native={}", out_dir.display());
}

/// Compiles tiny exported test DLLs used by Windows hook integration tests.
fn build_windows_test_dlls() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let target_fixture = manifest_dir
        .join("tests")
        .join("fixtures")
        .join("hook_test_target.c");
    let caller_fixture = manifest_dir
        .join("tests")
        .join("fixtures")
        .join("hook_test_caller.c");
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let target_output = out_dir.join("hook_test_target.dll");
    let target_import = out_dir.join("hook_test_target.lib");
    let caller_output = out_dir.join("hook_test_caller.dll");
    let caller_import = out_dir.join("hook_test_caller.lib");

    println!("cargo:rerun-if-changed={}", target_fixture.display());
    println!("cargo:rerun-if-changed={}", caller_fixture.display());

    let compiler = cc::Build::new().get_compiler();
    let mut target_command = compiler.to_command();
    target_command
        .arg("/nologo")
        .arg("/LD")
        .arg(&target_fixture)
        .arg("/link")
        .arg(format!("/OUT:{}", target_output.display()))
        .arg(format!("/IMPLIB:{}", target_import.display()));

    let target_status = target_command
        .status()
        .expect("failed to launch compiler for Windows hook target DLL");
    if !target_status.success() {
        panic!("failed to build Windows hook target DLL");
    }

    let mut caller_command = compiler.to_command();
    caller_command
        .arg("/nologo")
        .arg("/LD")
        .arg(&caller_fixture)
        .arg(&target_import)
        .arg("/link")
        .arg(format!("/OUT:{}", caller_output.display()))
        .arg(format!("/IMPLIB:{}", caller_import.display()));

    let caller_status = caller_command
        .status()
        .expect("failed to launch compiler for Windows hook caller DLL");
    if !caller_status.success() {
        panic!("failed to build Windows hook caller DLL");
    }

    println!(
        "cargo:rustc-env=BLINDER_HOOK_TEST_TARGET_DLL={}",
        target_output.display()
    );
    println!(
        "cargo:rustc-env=BLINDER_HOOK_TEST_CALLER_DLL={}",
        caller_output.display()
    );
}

/// Compiles one tiny static library used by Windows build-linked hook tests.
fn build_windows_test_static_lib() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let target_fixture = manifest_dir
        .join("tests")
        .join("fixtures")
        .join("hook_test_target.c");

    println!("cargo:rerun-if-changed={}", target_fixture.display());

    let mut build = cc::Build::new();
    build.file(target_fixture);
    build.compile("hook_test_target_static");
}

/// Runs one subprocess and fails the build when it returns one non-zero status.
fn run_command(mut command: Command, message: &str) {
    let status = command.status().expect(message);
    if !status.success() {
        panic!("{message}");
    }
}
