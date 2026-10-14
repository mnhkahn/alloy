fn main() {
    println!("cargo:rerun-if-changed=src/updater.m");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos")
        && std::env::var_os("CARGO_FEATURE_GUI").is_some()
    {
        cc::Build::new()
            .file("src/updater.m")
            .flag("-fobjc-arc")
            .flag("-fblocks")
            .compile("alloy_updater");
        println!("cargo:rustc-link-lib=framework=Foundation");
        println!("cargo:rustc-link-lib=framework=AppKit");
    }
}
