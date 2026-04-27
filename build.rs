//! Build script. On Windows, attaches `assets/icon.ico` to the .exe so the
//! application icon shows up in the taskbar, the Alt+Tab switcher, and
//! Windows Explorer. No-op on other platforms.

#[cfg(windows)]
fn main() {
    let mut res = winres::WindowsResource::new();
    res.set_icon("assets/icon.ico");
    if let Err(e) = res.compile() {
        // Don't hard-fail the build — let cargo report a warning so the
        // binary still gets produced even if winres can't find rc.exe
        // for some reason (e.g. only Visual Studio Build Tools missing).
        eprintln!("cargo:warning=could not embed Windows icon: {e}");
    }
}

#[cfg(not(windows))]
fn main() {}
