use hook_inject::{Library, Process, inject_process};
use std::env;
use std::ffi::CString;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::Duration;

fn crate_manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn crate_manifest_path() -> PathBuf {
    crate_manifest_dir().join("Cargo.toml")
}

fn victim_binary_path() -> PathBuf {
    let mut path = crate_manifest_dir()
        .join("target")
        .join("debug")
        .join("screenshot_victim");

    if cfg!(windows) {
        path.set_extension("exe");
    }

    path
}

fn run_cargo_build(extra_args: &[&str]) -> Result<(), Box<dyn std::error::Error>> {
    let mut command = Command::new("cargo");
    command
        .arg("build")
        .arg("--manifest-path")
        .arg(crate_manifest_path());

    for arg in extra_args {
        command.arg(arg);
    }

    let status = command
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()?;

    if status.success() {
        Ok(())
    } else {
        Err("cargo build failed for screenshot demo".into())
    }
}

fn spawn_victim() -> Result<Child, Box<dyn std::error::Error>> {
    Command::new(victim_binary_path())
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(Into::into)
}

fn parse_log_path() -> PathBuf {
    let mut args = env::args().skip(1);

    args.next()
        .map(PathBuf::from)
        .unwrap_or_else(|| env::temp_dir().join("retarget-screenshot-agent.log"))
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let log_path = parse_log_path();

    run_cargo_build(&["--bin", "screenshot_victim", "--lib"])?;

    let mut victim = spawn_victim()?;
    let pid = victim.id();

    println!("started screenshot victim with pid {pid}");
    println!("waiting a moment before injection so you can see clean captures first");

    thread::sleep(Duration::from_secs(4));

    let library = Library::from_crate(crate_manifest_dir())?
        .with_data(CString::new(log_path.to_string_lossy().into_owned())?);
    let process = Process::from_pid(pid as i32)?;

    match inject_process(process, library) {
        Ok(_injected) => {
            println!("injected screenshot agent into pid {pid}");
            println!("logging screenshot requests to {}", log_path.display());
            println!("press Ctrl-C when you are done");
        }
        Err(error) => {
            let _ = victim.kill();
            return Err(error.into());
        }
    }

    let status = victim.wait()?;

    if status.success() {
        Ok(())
    } else {
        Err(format!("screenshot victim exited with status {status}").into())
    }
}
