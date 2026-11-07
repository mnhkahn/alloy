fn main() {
    println!("cargo:rerun-if-changed=src/updater.m");
    println!("cargo:rerun-if-changed=src/audio_player.m");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos")
        && std::env::var_os("CARGO_FEATURE_GUI").is_some()
    {
        cc::Build::new()
            .file("src/updater.m")
            .file("src/audio_player.m")
            .flag("-fobjc-arc")
            .flag("-fblocks")
            .compile("alloy_updater");
        println!("cargo:rustc-link-lib=framework=Foundation");
        println!("cargo:rustc-link-lib=framework=AppKit");
        println!("cargo:rustc-link-lib=framework=AVFoundation");
    }
}
