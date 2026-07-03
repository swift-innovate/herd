//! Embed the tray's exe icon on Windows. No-op on other platforms so the crate
//! builds identically on ubuntu/macOS.

fn main() {
    #[cfg(windows)]
    {
        println!("cargo:rerun-if-changed=assets/herd-tray-green.ico");
        let mut res = winres::WindowsResource::new();
        res.set_icon("assets/herd-tray-green.ico");
        if let Err(e) = res.compile() {
            // A missing resource compiler shouldn't fail the build — warn only.
            println!("cargo:warning=herd-tray: embedding exe icon failed: {e}");
        }
    }
}
