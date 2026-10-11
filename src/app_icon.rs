/// Called on the AppKit main thread after the application is initialized.
/// Embed the icon so development launches and replaced app bundles do not rely
/// on Launch Services or an already-running process's cached bundle resources.
#[cfg(target_os = "macos")]
#[allow(deprecated)]
pub fn install() {
    use cocoa::{
        appkit::{NSApp, NSApplication, NSImage},
        base::nil,
        foundation::NSData,
    };
    let bytes = include_bytes!("../assets/alloy-logo.png");
    // NSData copies the embedded bytes. StrongPtr owns the allocated NSImage;
    // AppKit retains it when assigning the application icon.
    unsafe {
        let data = NSData::dataWithBytes_length_(nil, bytes.as_ptr().cast(), bytes.len() as _);
        let image = NSImage::initWithData_(NSImage::alloc(nil), data);
        if image != nil {
            let image = objc::rc::StrongPtr::new(image);
            NSApp().setApplicationIconImage_(*image);
        }
    }
}
#[cfg(not(target_os = "macos"))]
pub fn install() {}
