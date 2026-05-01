#![cfg(target_os = "macos")]

use cocoa::appkit::{NSApp, NSApplication, NSApplicationActivationPolicyAccessory};
use cocoa::base::nil;
use cocoa::foundation::NSAutoreleasePool;
use std::panic::AssertUnwindSafe;

/// Run a TUI workload on a worker thread while the main thread runs the
/// AppKit event loop. macOS requires `NSApplication` callbacks (used by
/// `MPRemoteCommandCenter` / `MPNowPlayingInfoCenter`) to be dispatched
/// from the main thread.
///
/// Exits the process when the worker thread completes — there is no graceful
/// `NSApp.stop()` path here. The terminal is already restored by the
/// session's panic hook + explicit cleanup before the worker returns, so
/// the brutal exit is observably indistinguishable from a graceful one.
pub fn run_with_tui_thread<F>(work: F) -> !
where
    F: FnOnce() -> i32 + Send + 'static,
{
    unsafe {
        let _pool = NSAutoreleasePool::new(nil);
        let app = NSApp();
        app.setActivationPolicy_(NSApplicationActivationPolicyAccessory);
    }

    let _handle = std::thread::Builder::new()
        .name("looper-tui".to_string())
        .spawn(move || {
            let result = std::panic::catch_unwind(AssertUnwindSafe(work));
            let exit_code = match result {
                Ok(code) => code,
                Err(_) => 1,
            };
            std::process::exit(exit_code);
        })
        .expect("failed to spawn looper-tui thread");

    unsafe {
        let app = NSApp();
        app.run();
    }

    unreachable!("NSApp.run() returned unexpectedly");
}
