//! Embed the app icon and version info in the Windows executable.

fn main() {
    println!("cargo:rerun-if-changed=../../packaging/icons/photo-studio.ico");
    #[cfg(windows)]
    {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("../../packaging/icons/photo-studio.ico");
        res.set("ProductName", "Photo Studio");
        res.set("FileDescription", "Photo Studio");
        res.set("CompanyName", "Abhay Pratap Singh");
        res.set("LegalCopyright", "© 2026 Abhay Pratap Singh");
        if let Err(e) = res.compile() {
            println!("cargo:warning=could not embed the Windows icon: {e}");
        }
    }
}
