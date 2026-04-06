use hook_inject::{Library, Process, inject_process};
use std::env;
use std::ffi::CString;
use std::path::PathBuf;

fn usage(program: &str) -> String {
    format!("usage: {program} <pid> [log-path]")
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = env::args();
    let program = args.next().unwrap_or_else(|| "inject".to_string());
    let pid: i32 = args
        .next()
        .ok_or_else(|| usage(&program))?
        .parse()
        .map_err(|_| usage(&program))?;

    let log_path = args
        .next()
        .map(PathBuf::from)
        .unwrap_or_else(|| env::temp_dir().join(format!("retarget-screenshot-agent-{pid}.log")));

    let library = Library::from_crate(env!("CARGO_MANIFEST_DIR"))?
        .with_data(CString::new(log_path.to_string_lossy().into_owned())?);
    let process = Process::from_pid(pid)?;
    let _injected = inject_process(process, library)?;

    println!("injected screenshot agent into pid {pid}");
    println!("logging screenshot requests to {}", log_path.display());

    Ok(())
}
