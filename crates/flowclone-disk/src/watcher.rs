//! Event-driven disk attach/detach notifications.
//!
//! Instead of polling `diskutil` on a timer, the catalog can be refreshed only
//! when storage actually changes. On macOS this uses the DiskArbitration
//! framework; other platforms get a no-op watcher until their native source
//! (udev on Linux, `WM_DEVICECHANGE` on Windows) is implemented.

/// Watches for disks appearing or disappearing and invokes a callback on change.
///
/// Implementations are non-blocking: [`DiskWatcher::start`] spawns whatever
/// background thread / run loop it needs and returns immediately. The watcher
/// runs for the lifetime of the process.
pub trait DiskWatcher: Send + Sync {
    /// Begin delivering change notifications to `on_change`.
    ///
    /// `on_change` may be called from a background thread, possibly several
    /// times in quick succession (plugging one drive surfaces the whole disk
    /// and each of its partitions); debounce on the consumer side.
    fn start(&self, on_change: Box<dyn Fn() + Send + 'static>);
}

/// Pick the disk watcher for the current platform.
pub fn platform_disk_watcher() -> Box<dyn DiskWatcher> {
    #[cfg(target_os = "macos")]
    {
        Box::new(macos::MacosDiskWatcher)
    }
    #[cfg(not(target_os = "macos"))]
    {
        Box::new(NoopDiskWatcher)
    }
}

/// Fallback for platforms without a native watcher yet. Callers should keep a
/// periodic refresh as a safety net regardless of platform.
#[cfg(not(target_os = "macos"))]
struct NoopDiskWatcher;

#[cfg(not(target_os = "macos"))]
impl DiskWatcher for NoopDiskWatcher {
    fn start(&self, _on_change: Box<dyn Fn() + Send + 'static>) {}
}

#[cfg(target_os = "macos")]
mod macos {
    use super::DiskWatcher;
    use core_foundation_sys::base::{kCFAllocatorDefault, CFAllocatorRef};
    use core_foundation_sys::dictionary::CFDictionaryRef;
    use core_foundation_sys::runloop::{kCFRunLoopDefaultMode, CFRunLoopGetCurrent, CFRunLoopRun};
    use core_foundation_sys::string::CFStringRef;
    use std::ffi::c_void;
    use std::ptr;

    // Opaque DiskArbitration handles; we only ever pass these pointers around.
    #[repr(C)]
    struct DASession(c_void);
    type DASessionRef = *mut DASession;
    #[repr(C)]
    struct DADisk(c_void);
    type DADiskRef = *mut DADisk;

    type DADiskCallback = extern "C" fn(disk: DADiskRef, context: *mut c_void);

    #[link(name = "DiskArbitration", kind = "framework")]
    extern "C" {
        fn DASessionCreate(allocator: CFAllocatorRef) -> DASessionRef;
        fn DASessionScheduleWithRunLoop(
            session: DASessionRef,
            run_loop: core_foundation_sys::runloop::CFRunLoopRef,
            run_loop_mode: CFStringRef,
        );
        fn DARegisterDiskAppearedCallback(
            session: DASessionRef,
            match_: CFDictionaryRef,
            callback: DADiskCallback,
            context: *mut c_void,
        );
        fn DARegisterDiskDisappearedCallback(
            session: DASessionRef,
            match_: CFDictionaryRef,
            callback: DADiskCallback,
            context: *mut c_void,
        );
    }

    pub struct MacosDiskWatcher;

    impl DiskWatcher for MacosDiskWatcher {
        fn start(&self, on_change: Box<dyn Fn() + Send + 'static>) {
            // The run loop blocks, so it gets its own thread. Only the boxed
            // closure (which is Send) crosses the thread boundary.
            std::thread::spawn(move || run_session(on_change));
        }
    }

    fn run_session(on_change: Box<dyn Fn() + Send + 'static>) {
        // Hand the closure to the C callbacks as an opaque context. It outlives
        // the run loop (which never returns), so it is intentionally leaked.
        let context = Box::into_raw(Box::new(on_change)) as *mut c_void;

        // SAFETY: standard DiskArbitration setup. `context` points to a live
        // `Box<dyn Fn() + Send>` for the run loop's lifetime; the callbacks only
        // borrow it. `kCFAllocatorDefault` and `kCFRunLoopDefaultMode` are valid
        // CoreFoundation globals, and the session is scheduled before running the
        // loop so the loop has a source and blocks instead of returning.
        unsafe {
            let session = DASessionCreate(kCFAllocatorDefault);
            if session.is_null() {
                // Reclaim the box so we don't leak on the failure path.
                drop(Box::from_raw(context as *mut Box<dyn Fn() + Send>));
                return;
            }
            DARegisterDiskAppearedCallback(session, ptr::null(), on_disk_changed, context);
            DARegisterDiskDisappearedCallback(session, ptr::null(), on_disk_changed, context);
            DASessionScheduleWithRunLoop(session, CFRunLoopGetCurrent(), kCFRunLoopDefaultMode);
            CFRunLoopRun();
        }
    }

    extern "C" fn on_disk_changed(_disk: DADiskRef, context: *mut c_void) {
        // SAFETY: `context` is the leaked `Box<dyn Fn() + Send>` from
        // `run_session`, alive for the process lifetime; we only borrow it.
        let on_change = unsafe { &*(context as *const Box<dyn Fn() + Send>) };
        on_change();
    }
}
