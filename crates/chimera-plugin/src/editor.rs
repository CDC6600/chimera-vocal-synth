//! The editor: a WKWebView attached as a subview of the host's NSView (macOS), showing the React UI
//! embedded in the plugin. [`crate::bridge`] does all the talking; this only connects it.
//!
//! - The page and the state it polls come from a custom protocol (`chimera://localhost/…`), so no
//!   files or ports are involved and the UI pulls state at its own frame rate.
//! - Commands go the other way through `window.ipc.postMessage` (JSON).
//! - Both handlers run on the host's main thread.

use std::ffi::c_void;
use std::ptr::{NonNull, null_mut};
use std::sync::atomic::{AtomicPtr, Ordering};
use std::sync::{Arc, Mutex};

use nih_plug::prelude::*;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{class, msg_send};
use objc2_core_foundation::{CGPoint, CGRect};
use raw_window_handle::{AppKitWindowHandle, HandleError, HasWindowHandle, RawWindowHandle, WindowHandle};
use wry::dpi::{LogicalPosition, LogicalSize};
use wry::http::{Request, Response};
use wry::{Rect, WebView, WebViewBuilder, WebViewExtMacOS};

use crate::bridge::{Bridge, GripAction, GuiHost, ParamHost, WINDOW_DEFAULT, WINDOW_MAX, WINDOW_MIN, page_zoom};
use crate::core::Shared;
use crate::ChimeraParams;

/// NSViewWidthSizable | NSViewHeightSizable
const FOLLOW_SUPERVIEW: usize = 2 | 16;
/// Polls after the grip lets go before the host's view is taken as the answer (about half a second:
/// a CLAP host may resize a little after it's asked).
const SETTLE_POLLS: u32 = 30;
/// Room left on the screen for the host's title bar and toolbar above the editor (points).
const HOST_CHROME: f64 = 60.0;

pub struct WebEditor {
    params: Arc<ChimeraParams>,
    shared: Arc<Shared>,
}

impl WebEditor {
    pub fn new(params: Arc<ChimeraParams>, shared: Arc<Shared>) -> Self {
        Self { params, shared }
    }
}

/// The host's NSView, as a window handle wry can attach to.
struct HostView(NonNull<c_void>);

impl HasWindowHandle for HostView {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        let raw = RawWindowHandle::AppKit(AppKitWindowHandle::new(self.0));
        // SAFETY: the host keeps its view alive while the editor is open (we drop the webview first)
        Ok(unsafe { WindowHandle::borrow_raw(raw) })
    }
}

/// The host's connection, with the host's view for resizing.
struct WindowHost {
    host: GuiHost,
    params: Arc<ChimeraParams>,
    view: NonNull<c_void>,
    /// the WKWebView, once built; not owned, because the webview owns this struct
    webview: Arc<AtomicPtr<AnyObject>>,
    /// the window's size and the pointer's place when the drag began
    drag: Mutex<Option<((u32, u32), CGPoint)>>,
    /// the view size the page's zoom was set for
    zoomed_for: Mutex<(u32, u32)>,
    /// polls until the view's size is taken as the window's
    settling: Mutex<u32>,
}

// SAFETY: the bridge calls this from the host's main thread, while the editor is open.
unsafe impl Send for WindowHost {}
unsafe impl Sync for WindowHost {}

impl WindowHost {
    fn store(&self, size: (u32, u32)) {
        *self.params.window.write().unwrap_or_else(|e| e.into_inner()) = size;
    }

    fn size(&self) -> (u32, u32) {
        *self.params.window.read().unwrap_or_else(|e| e.into_inner())
    }

    /// The size of the host's view (points).
    fn view_size(&self) -> (u32, u32) {
        // SAFETY: an NSView, alive while the editor is open, and `frame` returns a CGRect
        let frame: CGRect = unsafe { msg_send![self.view.as_ptr() as *const AnyObject, frame] };
        (frame.size.width.round() as u32, frame.size.height.round() as u32)
    }

    fn resize(&self, asked: (u32, u32)) {
        if asked == self.size() {
            return;
        }
        // `request_resize` reports the editor's `size()`, so that changes first. Its answer means
        // nothing here: clap-wrapper's VST3 layer returns false on success and true on failure, and
        // its AU layer always returns false. `tick` looks at what really happened once the grip is let go.
        self.store(asked);
        let _ = self.host.context.request_resize();
    }

    fn settle(&self) {
        *self.settling.lock().unwrap_or_else(|e| e.into_inner()) = SETTLE_POLLS;
    }
}

/// A size that fits on the screen in use, host's window included. A window hanging off the screen
/// would have its grip there too, with no way to make it smaller; the page zooms out to suit.
fn fit_screen((width, height): (u32, u32)) -> (u32, u32) {
    // SAFETY: class methods on the main thread, where hosts ask for the editor's size
    let screen: *const AnyObject = unsafe { msg_send![class!(NSScreen), mainScreen] };
    if screen.is_null() {
        return (width, height);
    }
    // SAFETY: an NSScreen, and `visibleFrame` returns a CGRect
    let room: CGRect = unsafe { msg_send![screen, visibleFrame] };
    (width.min(room.size.width as u32).max(WINDOW_MIN.0), height.min((room.size.height - HOST_CHROME).max(0.0) as u32).max(WINDOW_MIN.1))
}

/// The pointer on screen (points, y upwards). The page's own coordinates change with its zoom.
fn pointer() -> CGPoint {
    // SAFETY: a class method returning an NSPoint
    unsafe { msg_send![class!(NSEvent), mouseLocation] }
}

impl ParamHost for WindowHost {
    fn begin(&self, id: &str) {
        self.host.begin(id);
    }
    fn set(&self, id: &str, plain: f32) {
        self.host.set(id, plain);
    }
    fn end(&self, id: &str) {
        self.host.end(id);
    }
    fn values(&self) -> Vec<(String, f32)> {
        self.host.values()
    }
    fn grip(&self, action: GripAction) {
        let mut drag = self.drag.lock().unwrap_or_else(|e| e.into_inner());
        match action {
            GripAction::Begin => {
                // the view's size, not the saved one: the window may be smaller to fit the screen
                *drag = Some((self.view_size(), pointer()));
                *self.settling.lock().unwrap_or_else(|e| e.into_inner()) = 0;
            }
            GripAction::Move => {
                let Some(((w, h), from)) = *drag else { return };
                let to = pointer();
                let width = w as f64 + to.x - from.x;
                let height = h as f64 + from.y - to.y;
                self.resize(((width.round().max(0.0) as u32).clamp(WINDOW_MIN.0, WINDOW_MAX.0), (height.round().max(0.0) as u32).clamp(WINDOW_MIN.1, WINDOW_MAX.1)));
            }
            GripAction::End => {
                *drag = None;
                self.settle();
            }
            GripAction::Reset => {
                *drag = None;
                self.resize(WINDOW_DEFAULT);
                self.settle();
            }
        }
    }
    /// The page's zoom follows the host's view, whichever way that came to change, and once the
    /// grip has settled the view's size is the window's: a host may have refused or clamped it.
    fn tick(&self) {
        let now = self.view_size();
        let mut settling = self.settling.lock().unwrap_or_else(|e| e.into_inner());
        if *settling > 0 {
            *settling -= 1;
            if *settling == 0 {
                self.store(now);
            }
        }
        drop(settling);
        let mut zoomed_for = self.zoomed_for.lock().unwrap_or_else(|e| e.into_inner());
        if *zoomed_for != now {
            let webview = self.webview.load(Ordering::Relaxed);
            if !webview.is_null() {
                *zoomed_for = now;
                // SAFETY: the WKWebView is alive while its handlers run, and this is the main thread
                let _: () = unsafe { msg_send![webview as *const AnyObject, setPageZoom: page_zoom(now)] };
            }
        }
    }
}

/// The open editor. Dropped (on the main thread) when the host closes the window; wry then removes
/// the webview from the host's view.
struct Open {
    bridge: Arc<Bridge>,
    _webview: WebView,
}

// SAFETY: nih-plug creates and drops the editor handle on the GUI thread only.
unsafe impl Send for Open {}

impl Drop for Open {
    fn drop(&mut self) {
        self.bridge.close();
    }
}

impl Editor for WebEditor {
    fn spawn(&self, parent: ParentWindowHandle, context: Arc<dyn GuiContext>) -> Box<dyn std::any::Any + Send> {
        let ParentWindowHandle::AppKitNsView(view) = parent else { return Box::new(()) };
        let Some(view) = NonNull::new(view) else { return Box::new(()) };

        let size @ (width, height) = self.size();
        let webview_ptr = Arc::new(AtomicPtr::new(null_mut()));
        let host = WindowHost { host: GuiHost::new(self.params.clone(), context), params: self.params.clone(), view, webview: webview_ptr.clone(), drag: Mutex::new(None), zoomed_for: Mutex::new(size), settling: Mutex::new(0) };
        let bridge = Arc::new(Bridge::new(self.params.clone(), self.shared.clone(), Box::new(host)));

        let serving = bridge.clone();
        let protocol = move |_id: wry::WebViewId, req: Request<Vec<u8>>| {
            let reply = serving.request(req.uri().path());
            Response::builder().status(reply.status).header("Content-Type", reply.mime).header("Cache-Control", "no-store").body(reply.body).unwrap()
        };
        let commands = bridge.clone();
        let ipc = move |req: Request<String>| commands.command(req.body());

        let built = WebViewBuilder::new()
            .with_bounds(Rect { position: LogicalPosition::new(0, 0).into(), size: LogicalSize::new(width, height).into() })
            .with_custom_protocol("chimera".into(), protocol)
            .with_ipc_handler(ipc)
            // a link must not replace the plugin's own page
            .with_navigation_handler(|url| url.starts_with("chimera://"))
            // the first click on a window the host hasn't focused still works the control under it
            .with_accept_first_mouse(true)
            .with_devtools(true)
            .with_url("chimera://localhost/")
            .build_as_child(&HostView(view));
        match built {
            Ok(webview) => {
                // wry fixes a child's size; this one follows the host's view whenever that changes
                let wk = webview.webview();
                // SAFETY: `setAutoresizingMask:` takes an NSUInteger, on the main thread we're on
                let _: () = unsafe { msg_send![&*wk, setAutoresizingMask: FOLLOW_SUPERVIEW] };
                webview_ptr.store(Retained::as_ptr(&wk) as *mut AnyObject, Ordering::Relaxed);
                let _ = webview.zoom(page_zoom(size));
                Box::new(Open { bridge, _webview: webview })
            }
            Err(err) => {
                nih_error!("CHIMERA: could not create the webview: {err}");
                Box::new(())
            }
        }
    }

    fn size(&self) -> (u32, u32) {
        fit_screen(*self.params.window.read().unwrap_or_else(|e| e.into_inner()))
    }

    fn set_scale_factor(&self, _factor: f32) -> bool {
        false // macOS scales for us
    }

    // the page polls the parameters, so host-side changes need no push
    fn param_value_changed(&self, _id: &str, _normalized_value: f32) {}
    fn param_modulation_changed(&self, _id: &str, _modulation_offset: f32) {}
    fn param_values_changed(&self) {}
}
