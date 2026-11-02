//! Embeds the app icon and version info into the Windows executable. GPUI
//! loads the window/taskbar icon from resource id 1.

fn main() {
    println!("cargo:rerun-if-changed=../../packaging/icons/hostwick.ico");
    #[cfg(windows)]
    {
        if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
            return;
        }
        let mut res = winresource::WindowsResource::new();
        res.set_icon("../../packaging/icons/hostwick.ico")
            .set("ProductName", "Hostwick")
            .set("FileDescription", "Hostwick")
            .set("CompanyName", "meetdhruva");
        if let Err(e) = res.compile() {
            // A missing icon must never break the build.
            println!("cargo:warning=Windows icon resource not embedded: {e}");
        }
    }
}
