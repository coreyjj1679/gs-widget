#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;

#[cfg(target_os = "macos")]
pub use macos::{set_enabled, spawn};
#[cfg(target_os = "windows")]
pub use windows::{set_enabled, spawn};

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
compile_error!("The widget currently supports Windows and macOS.");
