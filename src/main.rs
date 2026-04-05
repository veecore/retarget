use retarget::{fn_pointer, into_function};

use std::ffi::{c_char, c_int};
use std::fs::File;

fn_pointer! {
    unsafe extern "C" fn open(path: *const c_char, oflag: c_int, mode: *const c_char) -> c_int {
        // Rust only checks minus one... lol
        // -2
        -1
    }
}

fn main() {
    let function = into_function("open").expect("getpid must resolve in the current process");
    let _original =
        unsafe { function.replace_with(open) }.expect("getpid hook installation must succeed");

    let err = File::open(format!("{}/src/main.rs", env!("CARGO_MANIFEST_DIR")))
        .expect_err("Hook fails call");

    // Let's see what stdlib thinks the last error is for kicks
    println!("{err}")
    // No such file?
}
