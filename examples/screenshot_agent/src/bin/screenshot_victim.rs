use std::error::Error;
use std::path::PathBuf;
use std::thread;
use std::time::Duration;

use xcap::Monitor;

struct CaptureSummary {
    width: u32,
    height: u32,
    top_left_rgba: [u8; 4],
}

fn choose_monitor() -> Result<Monitor, Box<dyn Error>> {
    let monitors = Monitor::all()?;

    if let Some(primary) = monitors
        .iter()
        .find(|monitor| monitor.is_primary().unwrap_or(false))
    {
        return Ok(primary.clone());
    }

    monitors
        .into_iter()
        .next()
        .ok_or_else(|| "xcap did not report any monitors".into())
}

fn capture_dir() -> PathBuf {
    std::env::temp_dir().join(format!("retarget-screenshot-victim-{}", std::process::id()))
}

fn capture_path(frame: u32) -> PathBuf {
    capture_dir().join(format!("capture-{frame:03}.png"))
}

fn capture_frame(frame: u32) -> Result<CaptureSummary, Box<dyn Error>> {
    let monitor = choose_monitor()?;
    let image = monitor.capture_image()?;
    let top_left_rgba = image.get_pixel(0, 0).0;
    let width = image.width();
    let height = image.height();
    image.save(capture_path(frame))?;
    Ok(CaptureSummary {
        width,
        height,
        top_left_rgba,
    })
}

fn main() -> Result<(), Box<dyn Error>> {
    let capture_dir = capture_dir();
    std::fs::create_dir_all(&capture_dir)?;

    println!("pid: {}", std::process::id());
    println!("writing successful captures to {}", capture_dir.display());
    println!("inject the screenshot agent while this program is running");

    let mut frame = 0u32;

    loop {
        match capture_frame(frame) {
            Ok(summary) => {
                println!(
                    "capture {frame:03}: saved {} ({}x{}, rgba {:?})",
                    capture_path(frame).display(),
                    summary.width,
                    summary.height,
                    summary.top_left_rgba
                );
            }
            Err(error) => {
                println!("capture {frame:03}: failed ({error})");
            }
        }

        frame += 1;
        thread::sleep(Duration::from_secs(2));
    }
}
