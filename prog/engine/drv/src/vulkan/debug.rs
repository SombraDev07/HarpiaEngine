use std::ffi::CStr;
use std::os::raw::c_void;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use ash::vk;

pub unsafe extern "system" fn debug_callback(
    severity: vk::DebugUtilsMessageSeverityFlagsEXT,
    _ty: vk::DebugUtilsMessageTypeFlagsEXT,
    data: *const vk::DebugUtilsMessengerCallbackDataEXT,
    user: *mut c_void,
) -> vk::Bool32 {
    if data.is_null() {
        return vk::FALSE;
    }
    let data = unsafe { &*data };
    let msg = if data.p_message.is_null() {
        CStr::from_bytes_with_nul(b"<null>\0").unwrap_or(CStr::from_bytes_with_nul(b"?\0").unwrap())
    } else {
        unsafe { CStr::from_ptr(data.p_message) }
    };
    let text = msg.to_string_lossy();

    if severity.contains(vk::DebugUtilsMessageSeverityFlagsEXT::ERROR) {
        tracing::error!(target: "vk.validation", "{text}");
        if !user.is_null() {
            unsafe { (*user.cast::<AtomicU32>()).fetch_add(1, Ordering::Relaxed) };
        }
    } else if severity.contains(vk::DebugUtilsMessageSeverityFlagsEXT::WARNING) {
        tracing::warn!(target: "vk.validation", "{text}");
    } else {
        tracing::debug!(target: "vk.validation", "{text}");
    }
    vk::FALSE
}

pub fn leak_error_counter(counter: &Arc<AtomicU32>) -> *mut c_void {
    Arc::into_raw(Arc::clone(counter)) as *mut c_void
}

pub unsafe fn release_error_counter(ptr: *mut c_void) {
    if !ptr.is_null() {
        drop(unsafe { Arc::from_raw(ptr as *const AtomicU32) });
    }
}
