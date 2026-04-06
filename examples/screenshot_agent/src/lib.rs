use retarget::install_registered_hooks;
use std::ffi::{CStr, c_char, c_void};
use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::sync::OnceLock;

static LOG_PATH: OnceLock<PathBuf> = OnceLock::new();
static INSTALL_STATUS: OnceLock<Result<(), String>> = OnceLock::new();

fn log_path() -> &'static PathBuf {
    LOG_PATH.get_or_init(|| std::env::temp_dir().join("retarget-screenshot-agent.log"))
}

fn append_log_line(line: &str) {
    let _ = OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path())
        .and_then(|mut file| writeln!(file, "{line}"));
}

#[cfg(target_os = "macos")]
mod platform {
    use super::append_log_line;
    use objc2_core_foundation::{CFRetained, CGRect};
    use objc2_core_graphics::{
        CGBitmapInfo, CGColorRenderingIntent, CGColorSpace, CGDataProvider, CGImage,
        CGImageAlphaInfo, CGImageByteOrderInfo, CGImageComponentInfo, CGImagePixelFormatInfo,
    };
    use retarget::hook;
    use std::ffi::c_void;
    use std::ptr;
    use std::ptr::{NonNull, slice_from_raw_parts_mut};

    fn synthetic_bgra_frame(width: usize, height: usize) -> Vec<u8> {
        let mut bytes = vec![0u8; width * height * 4];

        for y in 0..height {
            let band = (y * 3) / height.max(1);
            let (r, g, b) = match band {
                0 => (255, 90, 90),
                1 => (24, 24, 24),
                _ => (255, 255, 255),
            };

            for x in 0..width {
                let index = (y * width + x) * 4;
                bytes[index] = b;
                bytes[index + 1] = g;
                bytes[index + 2] = r;
                bytes[index + 3] = 255;
            }
        }

        bytes
    }

    unsafe extern "C-unwind" fn release_synthetic_data(
        _info: *mut c_void,
        data: NonNull<c_void>,
        size: usize,
    ) {
        let slice = slice_from_raw_parts_mut(data.as_ptr().cast::<u8>(), size);
        unsafe {
            drop(Box::from_raw(slice));
        }
    }

    fn fake_capture(width: usize, height: usize) -> Option<*mut c_void> {
        let len = width.checked_mul(height)?.checked_mul(4)?;
        let buffer: *mut [u8] = Box::into_raw(synthetic_bgra_frame(width, height).into_boxed_slice());
        let data_ptr = buffer.cast::<c_void>();

        let provider = unsafe {
            CGDataProvider::with_data(
                ptr::null_mut(),
                data_ptr.cast_const(),
                len,
                Some(release_synthetic_data),
            )
        }?;

        let color_space = CGColorSpace::new_device_rgb()?;
        let bitmap_info = CGBitmapInfo(
            CGImageAlphaInfo::NoneSkipFirst.0
                | CGImageComponentInfo::Integer.0
                | CGImageByteOrderInfo::Order32Little.0
                | CGImagePixelFormatInfo::Packed.0,
        );

        let image = unsafe {
            CGImage::new(
                width,
                height,
                8,
                32,
                width * 4,
                Some(&color_space),
                bitmap_info,
                Some(&provider),
                ptr::null(),
                false,
                CGColorRenderingIntent::RenderingIntentDefault,
            )
        }?;

        Some(CFRetained::into_raw(image).as_ptr().cast())
    }

    fn requested_size(bounds: CGRect) -> (usize, usize) {
        let width = bounds.size.width.max(1.0).round() as usize;
        let height = bounds.size.height.max(1.0).round() as usize;
        (width, height)
    }

    #[hook::c("CGWindowListCreateImage")]
    unsafe extern "C" fn cg_window_list_create_image(
        screen_bounds: CGRect,
        _list_option: u32,
        _window_id: u32,
        _image_option: u32,
    ) -> *mut c_void {
        let (width, height) = requested_size(screen_bounds);
        append_log_line(&format!(
            "serving synthetic screenshot via CGWindowListCreateImage ({width}x{height})"
        ));

        fake_capture(width, height).unwrap_or_else(|| {
            append_log_line("failed to build synthetic CGImage");
            ptr::null_mut()
        })
    }
}

#[cfg(target_os = "windows")]
mod platform {
    use super::append_log_line;
    use retarget::hook;
    use std::ffi::c_void;
    use windows_sys::Win32::Graphics::Gdi::{PatBlt, WHITENESS};

    #[hook::c(("gdi32.dll", "BitBlt"))]
    unsafe extern "system" fn bit_blt(
        hdc: *mut c_void,
        x: i32,
        y: i32,
        cx: i32,
        cy: i32,
        _hdc_src: *mut c_void,
        _x1: i32,
        _y1: i32,
        _rop: u32,
    ) -> i32 {
        let result = unsafe { PatBlt(hdc.cast(), x, y, cx, cy, WHITENESS) };

        if result != 0 {
            append_log_line(&format!(
                "serving synthetic screenshot via BitBlt ({cx}x{cy})"
            ));
        } else {
            append_log_line("failed to paint synthetic screenshot via BitBlt");
        }

        result
    }
}

fn install_hooks() -> std::io::Result<()> {
    let status = INSTALL_STATUS.get_or_init(|| install_registered_hooks().map_err(|error| error.to_string()));

    match status {
        Ok(()) => Ok(()),
        Err(error) => Err(std::io::Error::other(error.clone())),
    }
}

/// # Safety
///
/// `data` must be null or a valid NUL-terminated C string pointer.
/// `stay_resident` must be null or one valid writable pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hook_inject_entry(
    data: *const c_char,
    stay_resident: *mut i32,
    _state: *mut c_void,
) {
    if let Some(flag) = unsafe { stay_resident.as_mut() } {
        *flag = 1;
    }

    if !data.is_null() {
        let path = unsafe { CStr::from_ptr(data) }.to_string_lossy();
        if !path.is_empty() {
            let _ = LOG_PATH.set(PathBuf::from(path.into_owned()));
        }
    }

    match install_hooks() {
        Ok(()) => append_log_line("retarget screenshot agent installed"),
        Err(error) => append_log_line(&format!("failed to install screenshot hooks: {error}")),
    }
}
