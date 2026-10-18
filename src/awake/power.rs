//! An assertion belongs to Alloy; macOS also releases it if Alloy crashes.
#[cfg(target_os = "macos")]
mod native {
    use std::ffi::{c_char, c_void};
    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        pub fn CFStringCreateWithCString(
            allocator: *const c_void,
            text: *const c_char,
            encoding: u32,
        ) -> *const c_void;
        pub fn CFRelease(value: *const c_void);
    }
    #[link(name = "IOKit", kind = "framework")]
    unsafe extern "C" {
        pub fn IOPMAssertionCreateWithName(
            kind: *const c_void,
            level: u32,
            name: *const c_void,
            id: *mut u32,
        ) -> i32;
        pub fn IOPMAssertionRelease(id: u32) -> i32;
    }
}

pub(super) struct Assertion(u32);
impl Assertion {
    pub fn new() -> Result<Self, String> {
        #[cfg(target_os = "macos")]
        unsafe {
            let kind = native::CFStringCreateWithCString(
                std::ptr::null(),
                c"PreventUserIdleSystemSleep".as_ptr(),
                0x08000100,
            );
            let name = native::CFStringCreateWithCString(
                std::ptr::null(),
                c"Alloy: Codex or Herdr is working".as_ptr(),
                0x08000100,
            );
            if kind.is_null() || name.is_null() {
                if !kind.is_null() {
                    native::CFRelease(kind);
                }
                if !name.is_null() {
                    native::CFRelease(name);
                }
                return Err("Cannot create macOS power assertion strings".into());
            }
            let mut id = 0;
            let result = native::IOPMAssertionCreateWithName(kind, 255, name, &mut id);
            native::CFRelease(kind);
            native::CFRelease(name);
            return if result == 0 {
                Ok(Self(id))
            } else {
                Err(format!("macOS power assertion failed: {result}"))
            };
        }
        #[cfg(not(target_os = "macos"))]
        Err("Keep awake is supported on macOS only".into())
    }
}
impl Drop for Assertion {
    fn drop(&mut self) {
        #[cfg(target_os = "macos")]
        unsafe {
            native::IOPMAssertionRelease(self.0);
        }
    }
}
