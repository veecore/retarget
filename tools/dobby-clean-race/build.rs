use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Configures and builds one clean vendored Dobby copy for this harness.
fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("missing manifest dir"));
    let source_dir = manifest_dir.join("../../vendor/dobby-clean");
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("missing OUT_DIR"));
    let build_dir = out_dir.join("dobby-build");
    let lib_dir = out_dir.join("dobby-lib");

    std::fs::create_dir_all(&build_dir).expect("failed to create Dobby build directory");
    std::fs::create_dir_all(&lib_dir).expect("failed to create Dobby output directory");

    let mut configure = Command::new("cmake");
    configure
        .arg("-S")
        .arg(&source_dir)
        .arg("-B")
        .arg(&build_dir)
        .arg("-G")
        .arg("Ninja")
        .arg("-DCMAKE_BUILD_TYPE=Release")
        .arg("-DCMAKE_POSITION_INDEPENDENT_CODE=ON")
        .arg("-DBUILD_SHARED_LIBS=OFF")
        .arg("-DDOBBY_DEBUG=OFF")
        .arg("-DPlugin.SymbolResolver=ON")
        .arg(format!("-DCMAKE_ARCHIVE_OUTPUT_DIRECTORY={}", lib_dir.display()))
        .arg(format!("-DCMAKE_LIBRARY_OUTPUT_DIRECTORY={}", lib_dir.display()));
    run_command(&mut configure, "failed to configure clean Dobby");

    let mut build = Command::new("cmake");
    build
        .arg("--build")
        .arg(&build_dir)
        .arg("--target")
        .arg("dobby_static");
    run_command(&mut build, "failed to build clean Dobby");

    println!("cargo:rustc-link-search=native={}", lib_dir.display());
    println!("cargo:rustc-link-lib=static=dobby");

    if cfg!(target_os = "macos") {
        println!("cargo:rustc-link-lib=c++");
    } else if cfg!(target_os = "linux") {
        println!("cargo:rustc-link-lib=stdc++");
        println!("cargo:rustc-link-lib=dl");
    }

    emit_rerun_if_changed(&source_dir);
}

/// Emits one `rerun-if-changed` line for each file under the clean Dobby tree.
fn emit_rerun_if_changed(root: &Path) {
    println!("cargo:rerun-if-changed={}", root.display());
}

/// Runs one external command and panics with context on failure.
fn run_command(command: &mut Command, context: &str) {
    let status = command.status().unwrap_or_else(|error| panic!("{context}: {error}"));
    if !status.success() {
        panic!("{context}: status {status}");
    }
}
