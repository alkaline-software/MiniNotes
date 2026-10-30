//! What is asked of the system, one function a job with a body for each
//! system: nothing else in the app shells out to `open`, `osascript` or
//! `id`, and nothing else names a system's own API.

use crate::i18n::phrase;

/// Open a file or a link with the app the system gives it.
pub fn open(target: &str) {
    let _ = opener::open(target);
}

/// Show `path` selected in the file manager. macOS and Windows can select
/// a file; on Linux the folder opens, as close as xdg gets without D-Bus.
/// A path as a person reads it: a path in iCloud Drive from `iCloud
/// Drive`, as the Finder names the folder, the home folder as `~`, and
/// the system's own separator. The same on every system but the
/// separator and where iCloud Drive is.
pub fn short_path(path: &std::path::Path) -> String {
    let home = onyx_core::paths::home();
    let sep = std::path::MAIN_SEPARATOR;
    let cloud = home.join("Library").join("Mobile Documents").join("com~apple~CloudDocs");
    if let Ok(rest) = path.strip_prefix(&cloud) {
        return if rest.as_os_str().is_empty() { "iCloud Drive".to_string() } else { format!("iCloud Drive{sep}{}", rest.display()) };
    }
    match path.strip_prefix(&home) {
        Ok(rest) if rest.as_os_str().is_empty() => "~".to_string(),
        Ok(rest) => format!("~{sep}{}", rest.display()),
        Err(_) => path.display().to_string(),
    }
}

/// Onyx's own iCloud container, the folder iCloud Drive shows as an app
/// library with the icon on it, when the bundle is signed with the
/// iCloud entitlements and the provisioning profile (`docs/platform.md`).
/// Asked of the system once, off the main thread (`warm_icloud`), since
/// the first answer can take a moment; `None` until then, and for good in
/// a bundle without the entitlements, such as the dev build.
pub fn icloud_container() -> Option<std::path::PathBuf> {
    ICLOUD.get().cloned().flatten()
}

static ICLOUD: std::sync::OnceLock<Option<std::path::PathBuf>> = std::sync::OnceLock::new();

/// Ask the system for the container on a thread of its own, at launch.
pub fn warm_icloud() {
    std::thread::Builder::new()
        .name("icloud".into())
        .spawn(|| {
            let _ = ICLOUD.set(icloud_container_now());
        })
        .ok();
}

#[cfg(target_os = "macos")]
fn icloud_container_now() -> Option<std::path::PathBuf> {
    use objc2_foundation::NSFileManager;
    // The container's Documents folder is what iCloud Drive shows; the
    // rest of the container is the app's own.
    let url = NSFileManager::defaultManager().URLForUbiquityContainerIdentifier(None)?;
    let path = url.path()?;
    let dir = std::path::PathBuf::from(path.to_string()).join("Documents");
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir)
}

#[cfg(not(target_os = "macos"))]
fn icloud_container_now() -> Option<std::path::PathBuf> {
    None
}

pub fn reveal(path: &std::path::Path) {
    #[cfg(target_os = "macos")]
    {
        let _ = onyx_core::process::command("open").arg("-R").arg(path).spawn();
    }
    #[cfg(target_os = "windows")]
    {
        let _ = onyx_core::process::command("explorer").arg(format!("/select,{}", path.display())).spawn();
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        if let Some(parent) = path.parent() {
            let _ = opener::open(parent);
        }
    }
}

/// What revealing is called where we are, drawn through `t`.
pub const REVEAL_LABEL: &str = if cfg!(target_os = "macos") {
    phrase("Reveal in Finder")
} else if cfg!(target_os = "windows") {
    phrase("Show in Explorer")
} else {
    phrase("Show in folder")
};

/// The Dock icon, for a bare binary run from `target/`. A bundle carries
/// the same picture as its `.icns`.
#[cfg(target_os = "macos")]
pub fn install_dock_icon() {
    use objc2::{AllocAnyThread as _, MainThreadMarker};
    use objc2_app_kit::{NSApplication, NSImage};
    use objc2_foundation::NSData;
    let Some(mtm) = MainThreadMarker::new() else { return };
    let data = NSData::with_bytes(include_bytes!("../assets/icon/icon-1024.png"));
    if let Some(image) = NSImage::initWithData(NSImage::alloc(), &data) {
        unsafe { NSApplication::sharedApplication(mtm).setApplicationIconImage(Some(&image)) };
    }
}
#[cfg(not(target_os = "macos"))]
pub fn install_dock_icon() {}

/// The native window's own ground, under everything the app draws. gpui
/// leaves it the system's, near white in the light appearance, and it
/// showed under a dark frame while the window was drawn or resized. Set
/// to the frame's color, whatever shows there is the frame.
#[cfg(target_os = "macos")]
pub fn set_window_ground(window: &gpui::Window, ground: gpui::Rgba) {
    use objc2_app_kit::NSColor;

    if let Some(native) = ns_view(window).and_then(|v| v.window()) {
        let color = NSColor::colorWithSRGBRed_green_blue_alpha(ground.r as f64, ground.g as f64, ground.b as f64, 1.);
        native.setBackgroundColor(Some(&color));
    }
}
#[cfg(not(target_os = "macos"))]
pub fn set_window_ground(_window: &gpui::Window, _ground: gpui::Rgba) {}

/// Where gpui puts the traffic lights' row on the main window: the close
/// button's top left, from the window's top left (`main.rs`).
pub const LIGHTS_ROW: (f32, f32) = (17., 20.);

/// The main window's traffic lights between gpui's row (`k` 1) and a
/// column at the rail's head about `mid` (`k` 0), as the sidebar folds
/// (`Shell::render_rail`). gpui puts them back in its row at a resize,
/// a change of key window or of appearance and more, so each button and
/// their container are watched, and moved again whenever anything moves
/// them. (After emaki's `sys::fold_lights`.)
#[cfg(target_os = "macos")]
pub fn fold_lights(window: &gpui::Window, k: f32, mid: f32) {
    let Some(native) = ns_view(window).and_then(|v| v.window()) else { return };
    lights::FOLD.set((k, mid));
    lights::watch(&native);
    lights::place(&native);
}
#[cfg(not(target_os = "macos"))]
pub fn fold_lights(_: &gpui::Window, _: f32, _: f32) {}

#[cfg(target_os = "macos")]
mod lights {
    use std::cell::{Cell, RefCell};
    use std::ptr::NonNull;

    use objc2::rc::{Retained, Weak};
    use objc2::runtime::ProtocolObject;
    use objc2::Message as _;
    use objc2_app_kit::{NSButton, NSView, NSViewFrameDidChangeNotification, NSWindow, NSWindowButton, NSWindowStyleMask};
    use objc2_foundation::{NSNotification, NSNotificationCenter, NSObjectProtocol, NSPoint, NSRect, NSSize};

    thread_local! {
        pub(super) static FOLD: Cell<(f32, f32)> = const { Cell::new((1., 0.)) };
        /// The distance from one button to the next in the system's row,
        /// read while they stand in it.
        static PITCH: Cell<Option<f64>> = const { Cell::new(None) };
        /// Set while the lights are being moved here, so the moves they
        /// post are not answered.
        static MOVING: Cell<bool> = const { Cell::new(false) };
        static WATCHING: RefCell<(usize, Vec<Retained<ProtocolObject<dyn NSObjectProtocol>>>)> = const { RefCell::new((0, Vec::new())) };
    }

    fn buttons(native: &NSWindow) -> Option<[Retained<NSButton>; 3]> {
        Some([
            native.standardWindowButton(NSWindowButton::CloseButton)?,
            native.standardWindowButton(NSWindowButton::MiniaturizeButton)?,
            native.standardWindowButton(NSWindowButton::ZoomButton)?,
        ])
    }

    /// The view the buttons stand in, and the title bar's container
    /// around it, which gpui sizes to the row.
    fn holders(close: &NSButton) -> (Option<Retained<NSView>>, Option<Retained<NSView>>) {
        // SAFETY: the button is the window's own, from
        // `standardWindowButton`, and each view is returned retained.
        let view = unsafe { close.superview() };
        let container = view.as_ref().and_then(|v| unsafe { v.superview() });
        (view, container)
    }

    /// Each button and their container post their moves to `place`.
    /// AppKit can make the buttons anew, and new ones are watched then.
    pub(super) fn watch(native: &NSWindow) {
        let Some(b) = buttons(native) else { return };
        let key = Retained::as_ptr(&b[0]) as usize;
        if WATCHING.with_borrow(|w| w.0 == key) {
            return;
        }
        let center = NSNotificationCenter::defaultCenter();
        WATCHING.with_borrow_mut(|w| {
            for old in w.1.drain(..) {
                unsafe { center.removeObserver(AsRef::<objc2::runtime::AnyObject>::as_ref(&*old)) };
            }
        });
        let weak = Weak::from_retained(&native.retain());
        let block = block2::RcBlock::new(move |_: NonNull<NSNotification>| {
            if let Some(native) = weak.load() {
                place(&native);
            }
        });
        let container = holders(&b[0]).1;
        let views: Vec<Retained<NSView>> = b.iter().map(|b| Retained::into_super(Retained::into_super(b.clone()))).chain(container).collect();
        let tokens = views
            .iter()
            .map(|v| {
                v.setPostsFrameChangedNotifications(true);
                unsafe { center.addObserverForName_object_queue_usingBlock(Some(NSViewFrameDidChangeNotification), Some(v), None, &block) }
            })
            .collect();
        WATCHING.set((key, tokens));
    }

    /// The buttons where the fold puts them. In full screen they are the
    /// system's, as gpui leaves them.
    pub(super) fn place(native: &NSWindow) {
        if MOVING.get() || native.styleMask().contains(NSWindowStyleMask::FullScreen) {
            return;
        }
        let Some(b) = buttons(native) else { return };
        let (Some(view), Some(container)) = holders(&b[0]) else { return };
        let (close, mini) = (b[0].frame(), b[1].frame());
        if (close.origin.y - mini.origin.y).abs() < 0.01 && mini.origin.x > close.origin.x {
            PITCH.set(Some(mini.origin.x - close.origin.x));
        }
        let Some(pitch) = PITCH.get() else { return };
        let (k, mid) = FOLD.get();
        let (k, mid) = (k as f64, mid as f64);
        let (row_x, row_y) = (super::LIGHTS_ROW.0 as f64, super::LIGHTS_ROW.1 as f64);
        let (w, h) = (close.size.width, close.size.height);
        let col_x = mid - w / 2.;
        // Each button's left and its top under the window's top.
        let at = |i: f64| (col_x + (row_x + pitch * i - col_x) * k, row_y + pitch * i * (1. - k));
        // The container holds the buttons, which take a click only
        // inside it: as tall as the column. It stays the window's width,
        // as gpui leaves it: on some systems it draws the window's rim
        // and corner at its own edge, which narrower stood as a notch
        // and a line on the rail. A click on the card under it passes
        // through to the window, as it does under gpui's own row.
        let window = native.frame().size;
        let height = at(2.).1 + h + row_y;
        MOVING.set(true);
        let mut moved = false;
        let mut frame: NSRect = container.frame();
        if (frame.size.height - height).abs() > 0.01 || (frame.origin.y - (window.height - height)).abs() > 0.01 || (frame.size.width - window.width).abs() > 0.01 || frame.origin.x.abs() > 0.01 {
            frame.size = NSSize::new(window.width, height);
            frame.origin.x = 0.;
            frame.origin.y = window.height - height;
            container.setFrame(frame);
            moved = true;
        }
        let (flipped, inside) = (view.isFlipped(), view.bounds().size.height);
        for (i, button) in b.iter().enumerate() {
            let (x, y) = at(i as f64);
            let origin = NSPoint::new(x, if flipped { y } else { inside - y - h });
            let now = button.frame().origin;
            if (now.x - origin.x).abs() > 0.01 || (now.y - origin.y).abs() > 0.01 {
                button.setFrameOrigin(origin);
                button.updateTrackingAreas();
                moved = true;
            }
        }
        if moved {
            container.updateTrackingAreas();
        }
        MOVING.set(false);
    }
}

#[cfg(target_os = "macos")]
mod languages {
    use objc2::runtime::AnyObject;
    use objc2_foundation::{NSArray, NSBundle, NSString, NSUserDefaults};

    const KEY: &str = "AppleLanguages";

    /// `AppleLanguages` as one domain of the defaults has it, alone: not
    /// the search list's, which falls back from the app's to the system's.
    fn list(domain: &str) -> Vec<String> {
        let defaults = NSUserDefaults::standardUserDefaults();
        let Some(dict) = defaults.persistentDomainForName(&NSString::from_str(domain)) else { return Vec::new() };
        let Some(value) = dict.objectForKey(&NSString::from_str(KEY)) else { return Vec::new() };
        let Ok(all) = value.downcast::<NSArray>() else { return Vec::new() };
        all.iter().filter_map(|o| o.downcast::<NSString>().ok().map(|s| s.to_string())).collect()
    }

    pub fn system() -> Vec<String> {
        list("NSGlobalDomain")
    }

    /// A bare executable has no bundle and so no domain of its own: it
    /// follows the system's.
    pub fn own() -> Option<String> {
        let id = NSBundle::mainBundle().bundleIdentifier()?;
        list(&id.to_string()).into_iter().next()
    }

    pub fn set_own(code: Option<&str>) {
        if NSBundle::mainBundle().bundleIdentifier().is_none() {
            return;
        }
        let defaults = NSUserDefaults::standardUserDefaults();
        let key = NSString::from_str(KEY);
        match code {
            Some(code) => {
                let one = NSArray::from_retained_slice(&[NSString::from_str(code)]);
                let one: &AnyObject = &one;
                unsafe { defaults.setObject_forKey(Some(one), &key) };
            }
            None => defaults.removeObjectForKey(&key),
        }
    }
}

/// The system's preferred languages, in its order, apart from any the app
/// was given of its own.
#[cfg(target_os = "macos")]
pub fn system_languages() -> Vec<String> {
    let all = languages::system();
    if all.is_empty() { sys_locale::get_locales().collect() } else { all }
}
#[cfg(not(target_os = "macos"))]
pub fn system_languages() -> Vec<String> {
    sys_locale::get_locales().collect()
}

/// The language the app was given of its own, if any: by its setting or
/// by System Settings, Language & Region, Applications, which write the
/// same key. Elsewhere the settings file keeps it.
#[cfg(target_os = "macos")]
pub fn own_language() -> Option<String> {
    languages::own()
}
#[cfg(not(target_os = "macos"))]
pub fn own_language() -> Option<String> {
    None
}

/// Give the app a language of its own (a bundle's code, `zh-Hans`), or
/// none, to follow the system's.
#[cfg(target_os = "macos")]
pub fn set_own_language(code: Option<&str>) {
    languages::set_own(code);
}
#[cfg(not(target_os = "macos"))]
pub fn set_own_language(_code: Option<&str>) {}

#[cfg(target_os = "macos")]
fn ns_view(window: &gpui::Window) -> Option<&objc2_app_kit::NSView> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    let handle = HasWindowHandle::window_handle(window).ok()?;
    let RawWindowHandle::AppKit(handle) = handle.as_raw() else { return None };
    unsafe { (handle.ns_view.as_ptr() as *const objc2_app_kit::NSView).as_ref() }
}

/// A picture of the app's own window, written to `path` as a PNG, for
/// looking at a state from a script (`ONYX_SHOT`). A process may capture
/// its own windows without Screen Recording access, which the terminal
/// that runs a probe usually lacks. `CGWindowListCreateImage` is looked
/// up by name: newer SDKs mark it unavailable in favor of
/// ScreenCaptureKit, which wants that access, while the function itself
/// is still there and still captures a window of the caller's own.
#[cfg(target_os = "macos")]
pub fn shoot_window(window: &gpui::Window, path: &std::path::Path) -> Result<(), String> {
    use std::ffi::{c_char, c_void, CString};
    use std::os::unix::ffi::OsStrExt;

    use objc2_app_kit::NSView;
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    #[repr(C)]
    struct Rect {
        x: f64,
        y: f64,
        w: f64,
        h: f64,
    }
    type Capture = unsafe extern "C" fn(Rect, u32, u32, u32) -> *mut c_void;
    #[link(name = "ImageIO", kind = "framework")]
    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
        fn CFURLCreateFromFileSystemRepresentation(alloc: *const c_void, path: *const u8, len: isize, is_dir: u8) -> *mut c_void;
        fn CFStringCreateWithCString(alloc: *const c_void, text: *const c_char, encoding: u32) -> *mut c_void;
        fn CGImageDestinationCreateWithURL(url: *mut c_void, kind: *mut c_void, count: usize, options: *const c_void) -> *mut c_void;
        fn CGImageDestinationAddImage(dest: *mut c_void, image: *mut c_void, properties: *const c_void);
        fn CGImageDestinationFinalize(dest: *mut c_void) -> u8;
        fn CFRelease(obj: *mut c_void);
    }

    let handle = HasWindowHandle::window_handle(window).map_err(|e| e.to_string())?;
    let RawWindowHandle::AppKit(handle) = handle.as_raw() else { return Err("not an AppKit window".into()) };
    let view = unsafe { (handle.ns_view.as_ptr() as *const NSView).as_ref() }.ok_or("no view")?;
    let number = view.window().ok_or("the view has no window")?.windowNumber();
    let bytes = path.as_os_str().as_bytes();
    unsafe {
        // RTLD_DEFAULT, then: no rectangle (the window's own bounds),
        // only this window, without its shadow.
        let capture = dlsym(-2isize as *mut c_void, c"CGWindowListCreateImage".as_ptr());
        if capture.is_null() {
            return Err("this system has no CGWindowListCreateImage".into());
        }
        let capture: Capture = std::mem::transmute(capture);
        let null = Rect { x: f64::INFINITY, y: f64::INFINITY, w: 0.0, h: 0.0 };
        // `ONYX_SHOT_FRAME=1` keeps what the system draws around the
        // window, its shadow and its rim.
        let framing = if std::env::var_os("ONYX_SHOT_FRAME").is_some() { 0 } else { 1 };
        let image = capture(null, 1 << 3, number as u32, framing);
        if image.is_null() {
            return Err("the window could not be captured".into());
        }
        let url = CFURLCreateFromFileSystemRepresentation(std::ptr::null(), bytes.as_ptr(), bytes.len() as isize, 0);
        let png = CString::new("public.png").unwrap();
        let kind = CFStringCreateWithCString(std::ptr::null(), png.as_ptr(), 0x0800_0100);
        let dest = CGImageDestinationCreateWithURL(url, kind, 1, std::ptr::null());
        let written = if dest.is_null() {
            false
        } else {
            CGImageDestinationAddImage(dest, image, std::ptr::null());
            let ok = CGImageDestinationFinalize(dest) != 0;
            CFRelease(dest);
            ok
        };
        CFRelease(kind);
        CFRelease(url);
        CFRelease(image);
        if written { Ok(()) } else { Err(format!("could not write {}", path.display())) }
    }
}
#[cfg(not(target_os = "macos"))]
pub fn shoot_window(_window: &gpui::Window, _path: &std::path::Path) -> Result<(), String> {
    Err("a picture of the window is only taken on macOS".into())
}

/// The window kept over every other app's, for a probe.
#[cfg(target_os = "macos")]
pub fn keep_on_top(window: &gpui::Window) {
    if let Some(native) = ns_view(window).and_then(|v| v.window()) {
        native.setLevel(objc2_app_kit::NSFloatingWindowLevel);
    }
}
#[cfg(not(target_os = "macos"))]
pub fn keep_on_top(_window: &gpui::Window) {}

/// The account's first name, else the login name, capitalized: whom the
/// vault's page greets. The same crate asks every system.
pub fn user_first_name() -> String {
    let full = whoami::fallible::realname().unwrap_or_default();
    let name = full.split_whitespace().next().map(str::to_string).filter(|s| !s.is_empty()).or_else(|| whoami::fallible::username().ok()).unwrap_or_default();
    let mut chars = name.chars();
    match chars.next() {
        Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}
