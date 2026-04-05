use std::env;
use std::ffi::{c_char, c_void, CStr, CString};
use std::process::ExitCode;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Barrier, Mutex};
use std::thread;

unsafe extern "C" {
    /// Resolves one symbol through one clean upstream Dobby build.
    fn DobbySymbolResolver(image_name: *const c_char, symbol_name: *const c_char) -> *mut c_void;
}

/// One symbol resolution request used by the stress harness.
#[derive(Clone, Copy, Debug)]
struct Candidate {
    /// Optional image name passed to Dobby.
    image: Option<&'static str>,
    /// Symbol name passed to Dobby.
    symbol: &'static str,
}

/// One successfully resolved case paired with owned C strings and its baseline address.
#[derive(Debug)]
struct Baseline {
    /// The original candidate that resolved successfully.
    candidate: Candidate,
    /// Optional owned image name.
    image: Option<CString>,
    /// Owned symbol name.
    symbol: CString,
    /// The address returned during probing.
    address: usize,
}

/// Returns the default number of worker threads for the stress run.
fn default_threads() -> usize {
    thread::available_parallelism()
        .map(|parallelism| parallelism.get().saturating_mul(4))
        .unwrap_or(16)
        .max(8)
}

/// Returns one integer configuration value from the environment.
fn env_usize(name: &str, default: usize) -> usize {
    env::var(name)
        .ok()
        .and_then(|value| value.parse().ok())
        .filter(|value| *value > 0)
        .unwrap_or(default)
}

/// Returns the candidate list used for probing and stressing Dobby.
fn candidates() -> &'static [Candidate] {
    &[
        Candidate {
            image: None,
            symbol: "malloc",
        },
        Candidate {
            image: None,
            symbol: "_malloc",
        },
        Candidate {
            image: None,
            symbol: "getpid",
        },
        Candidate {
            image: None,
            symbol: "_getpid",
        },
        Candidate {
            image: None,
            symbol: "dlopen",
        },
        Candidate {
            image: None,
            symbol: "_dlopen",
        },
        Candidate {
            image: Some("libSystem.B.dylib"),
            symbol: "malloc",
        },
        Candidate {
            image: Some("libSystem.B.dylib"),
            symbol: "_malloc",
        },
        Candidate {
            image: Some("libSystem.B.dylib"),
            symbol: "getpid",
        },
        Candidate {
            image: Some("libSystem.B.dylib"),
            symbol: "_getpid",
        },
        Candidate {
            image: Some("libsystem_kernel.dylib"),
            symbol: "getpid",
        },
        Candidate {
            image: Some("libsystem_kernel.dylib"),
            symbol: "_getpid",
        },
        Candidate {
            image: Some("dyld"),
            symbol: "dlopen",
        },
        Candidate {
            image: Some("dyld"),
            symbol: "_dlopen",
        },
    ]
}

/// Converts one candidate into the owned representation used during stress.
fn baseline(candidate: Candidate, address: usize) -> Baseline {
    Baseline {
        candidate,
        image: candidate.image.map(|image| CString::new(image).expect("invalid image")),
        symbol: CString::new(candidate.symbol).expect("invalid symbol"),
        address,
    }
}

/// Resolves one candidate through one clean upstream Dobby build.
fn resolve(image: Option<&CStr>, symbol: &CStr) -> Option<usize> {
    let image_ptr = image.map_or(std::ptr::null(), CStr::as_ptr);
    let address = unsafe { DobbySymbolResolver(image_ptr, symbol.as_ptr()) };
    (!address.is_null()).then_some(address as usize)
}

/// Probes the candidate list and keeps only successful resolutions.
fn probe() -> Vec<Baseline> {
    candidates()
        .iter()
        .copied()
        .filter_map(|candidate| {
            let image = candidate
                .image
                .map(|image| CString::new(image).expect("invalid image"));
            let symbol = CString::new(candidate.symbol).expect("invalid symbol");
            resolve(image.as_deref(), &symbol).map(|address| baseline(candidate, address))
        })
        .collect()
}

/// Runs one concurrent stress round and returns the number of observed failures.
fn run_round(
    baselines: Arc<Vec<Baseline>>,
    threads: usize,
    iterations: usize,
    round: usize,
) -> usize {
    let failures = Arc::new(AtomicUsize::new(0));
    let notes = Arc::new(Mutex::new(Vec::new()));
    let start = Arc::new(Barrier::new(threads));
    let mut workers = Vec::with_capacity(threads);

    for worker_index in 0..threads {
        let baselines = Arc::clone(&baselines);
        let failures = Arc::clone(&failures);
        let notes = Arc::clone(&notes);
        let start = Arc::clone(&start);
        workers.push(thread::spawn(move || {
            start.wait();
            for iteration in 0..iterations {
                let baseline = &baselines[(worker_index + iteration) % baselines.len()];
                match resolve(baseline.image.as_deref(), &baseline.symbol) {
                    Some(address) if address == baseline.address => {}
                    Some(address) => {
                        failures.fetch_add(1, Ordering::Relaxed);
                        let mut notes = notes.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                        if notes.len() < 8 {
                            notes.push(format!(
                                "round {round} worker {worker_index} iteration {iteration}: {:?} changed from {:#x} to {:#x}",
                                baseline.candidate, baseline.address, address,
                            ));
                        }
                    }
                    None => {
                        failures.fetch_add(1, Ordering::Relaxed);
                        let mut notes = notes.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                        if notes.len() < 8 {
                            notes.push(format!(
                                "round {round} worker {worker_index} iteration {iteration}: {:?} resolved to null",
                                baseline.candidate,
                            ));
                        }
                    }
                }
            }
        }));
    }

    for worker in workers {
        worker.join().expect("worker thread panicked");
    }

    let failures = failures.load(Ordering::Relaxed);
    let notes = notes.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if !notes.is_empty() {
        eprintln!("notes:");
        for note in notes.iter() {
            eprintln!("  {note}");
        }
    }
    failures
}

/// Executes the stress harness and exits non-zero on mismatches or null resolutions.
fn main() -> ExitCode {
    let threads = env_usize("DOBBY_RACE_THREADS", default_threads());
    let iterations = env_usize("DOBBY_RACE_ITERATIONS", 20_000);
    let rounds = env_usize("DOBBY_RACE_ROUNDS", 5);
    let baselines = probe();

    println!(
        "clean Dobby race probe on {} with {threads} threads, {iterations} iterations, {rounds} rounds",
        env::consts::OS,
    );
    if baselines.is_empty() {
        eprintln!("no candidate symbols resolved during probe");
        return ExitCode::from(2);
    }

    println!("resolved {} baseline cases:", baselines.len());
    for baseline in &baselines {
        println!(
            "  image={:?} symbol={} address={:#x}",
            baseline.candidate.image, baseline.candidate.symbol, baseline.address,
        );
    }

    let baselines = Arc::new(baselines);
    let mut total_failures = 0usize;
    for round in 0..rounds {
        let failures = run_round(Arc::clone(&baselines), threads, iterations, round);
        println!("round {round}: {failures} failures");
        total_failures += failures;
    }

    if total_failures == 0 {
        println!("result: no mismatches or null resolutions observed");
        ExitCode::SUCCESS
    } else {
        eprintln!("result: observed {total_failures} total failures");
        ExitCode::from(1)
    }
}
