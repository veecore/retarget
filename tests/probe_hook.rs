//! Integration tests for public hook probing behavior.

#[cfg(target_os = "macos")]
#[path = "goals/probe_hook/macos.rs"]
mod macos;

#[cfg(target_os = "windows")]
#[path = "goals/probe_hook/windows.rs"]
mod windows;
