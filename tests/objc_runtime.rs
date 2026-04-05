//! Integration tests for Objective-C target resolution and swizzle install.

#[cfg(target_os = "macos")]
#[path = "goals/objc_runtime/macos.rs"]
mod macos;
