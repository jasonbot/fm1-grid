//! Platform selection for the update backend. Windows has Store and hosted-MSIX
//! paths, Linux has Flatpak; other targets keep the no-op stub.

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
mod unsupported;
#[cfg(target_os = "windows")]
mod windows;

#[cfg(target_os = "linux")]
pub(crate) use linux::Backend;
#[cfg(target_os = "macos")]
pub(crate) use macos::Backend;
#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
pub(crate) use unsupported::Backend;
#[cfg(target_os = "windows")]
pub(crate) use windows::Backend;

/// Pure MSIX (OPC ZIP) inspection shared by the Windows backend and its tests.
#[cfg_attr(not(windows), allow(dead_code))]
mod msix;

#[cfg(not(any(target_os = "linux", target_os = "windows")))]
mod stub;
