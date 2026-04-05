//! Integration tests for dynamic libraries loaded after hook install.

#![allow(unreachable_code)]

#[cfg(target_os = "macos")]
#[path = "goals/dynamic_loaded_after_install/macos.rs"]
mod macos;

#[cfg(target_os = "windows")]
#[path = "goals/dynamic_loaded_after_install/windows.rs"]
mod windows;
