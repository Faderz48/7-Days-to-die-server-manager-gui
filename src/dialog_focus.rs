//! Tiny helper to make native file dialogs come to the front on Windows.
//!
//! Background: Windows blocks processes from stealing focus from the
//! current foreground window (Win32 focus-stealing prevention). When our
//! HTTP API receives a request from the user's browser, the *browser* is
//! the foreground window — so when we open an `rfd` file dialog, Windows
//! happily opens it but refuses to bring it forward, leaving it blinking
//! on the taskbar.
//!
//! The standard workaround is `AttachThreadInput`: we temporarily attach
//! our thread's input queue to the foreground window's thread. Windows
//! then treats the two threads as one for input purposes, so when rfd
//! calls `SetForegroundWindow` it succeeds.
//!
//! We declare the four Win32 functions directly rather than depending on
//! `windows-sys`, because its module layout shifts between versions
//! and the signatures we need have been stable since Windows NT 4.

#[cfg(windows)]
pub fn focus_dialogs_for_this_thread() {
    use std::ffi::c_void;
    use std::ptr;

    type HWND = *mut c_void;

    #[link(name = "user32")]
    extern "system" {
        fn GetForegroundWindow() -> HWND;
        fn GetWindowThreadProcessId(hwnd: HWND, lpdw_process_id: *mut u32) -> u32;
        fn AttachThreadInput(id_attach: u32, id_attach_to: u32, f_attach: i32) -> i32;
    }

    #[link(name = "kernel32")]
    extern "system" {
        fn GetCurrentThreadId() -> u32;
    }

    unsafe {
        let fg = GetForegroundWindow();
        if fg.is_null() {
            return;
        }
        let fg_thread = GetWindowThreadProcessId(fg, ptr::null_mut());
        let our_thread = GetCurrentThreadId();
        if fg_thread != 0 && fg_thread != our_thread {
            // Attach. A subsequent SetForegroundWindow from rfd will now
            // be allowed because Windows sees us as part of the same
            // input group as the browser. We don't bother detaching;
            // it's harmless and the worker thread will get reset anyway.
            let _ = AttachThreadInput(our_thread, fg_thread, 1 /* TRUE */);
        }
    }
}

#[cfg(not(windows))]
pub fn focus_dialogs_for_this_thread() {}
