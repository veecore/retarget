//! Integration tests for statically or already-available hook targets.

#![allow(unreachable_code)]

#[cfg(target_os = "macos")]
#[path = "goals/static_loaded/macos.rs"]
mod macos;

#[cfg(target_os = "windows")]
#[path = "goals/static_loaded/windows.rs"]
mod windows;
