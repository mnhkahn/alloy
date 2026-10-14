//! Main-thread adapter for Sparkle. Sparkle verifies archives and installs on quit.
#[cfg(target_os = "macos")]
mod native {
    unsafe extern "C" {
        pub fn alloy_updater_start() -> i32;
        pub fn alloy_updater_available() -> i32;
        pub fn alloy_updater_can_check() -> i32;
        pub fn alloy_updater_check() -> i32;
        pub fn alloy_updater_set_blocked(blocked: i32);
    }
}
pub fn start() {
    #[cfg(target_os = "macos")]
    unsafe {
        native::alloy_updater_start();
    }
}
pub fn available() -> bool {
    #[cfg(target_os = "macos")]
    unsafe {
        return native::alloy_updater_available() != 0;
    }
    #[cfg(not(target_os = "macos"))]
    false
}
pub fn can_check() -> bool {
    #[cfg(target_os = "macos")]
    unsafe {
        return native::alloy_updater_can_check() != 0;
    }
    #[cfg(not(target_os = "macos"))]
    false
}
pub fn check() -> Result<(), String> {
    #[cfg(target_os = "macos")]
    if unsafe { native::alloy_updater_check() != 0 } {
        return Ok(());
    }
    Err(
        "Update checking is unavailable. Use the packaged Alloy.app and finish unsaved work first."
            .into(),
    )
}
pub fn set_blocked(blocked: bool) {
    #[cfg(target_os = "macos")]
    unsafe {
        native::alloy_updater_set_blocked(i32::from(blocked));
    }
    #[cfg(not(target_os = "macos"))]
    let _ = blocked;
}
