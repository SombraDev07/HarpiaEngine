//! Platform WSI. The **only** module allowed to care about X11 / Wayland / Win32.
//! Implementation is `ash-window`; we do not call Xlib/Wayland/Win32 ourselves.

use ash::{Entry, Instance, vk};
use raw_window_handle::{RawDisplayHandle, RawWindowHandle};

use crate::Result;

pub fn instance_extension_ptrs(display: RawDisplayHandle) -> Result<Vec<*const i8>> {
    let names = ash_window::enumerate_required_extensions(display)?;
    Ok(names.to_vec())
}

pub unsafe fn create_surface(
    entry: &Entry,
    instance: &Instance,
    display: RawDisplayHandle,
    window: RawWindowHandle,
) -> Result<vk::SurfaceKHR> {
    // Linux: X11 or Wayland according to the winit handle.
    // Windows: Win32.
    Ok(unsafe { ash_window::create_surface(entry, instance, display, window, None)? })
}
