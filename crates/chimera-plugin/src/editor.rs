//! The editor: a WKWebView attached as a subview of the host's NSView (macOS), showing the React UI
//! embedded in the plugin. [`crate::bridge`] does all the talking; this only connects it.
//!
//! - The page and the state it polls come from a custom protocol (`chimera://localhost/…`), so no
//!   files or ports are involved and the UI pulls state at its own frame rate.
//! - Commands go the other way through `window.ipc.postMessage` (JSON).
//! - Both handlers run on the host's main thread.

use std::ffi::c_void;
use std::ptr::NonNull;
use std::sync::Arc;

use nih_plug::prelude::*;
use raw_window_handle::{AppKitWindowHandle, HandleError, HasWindowHandle, RawWindowHandle, WindowHandle};
use wry::dpi::{LogicalPosition, LogicalSize};
use wry::http::{Request, Response};
use wry::{Rect, WebView, WebViewBuilder};

use crate::bridge::{Bridge, GuiHost};
use crate::core::Shared;
use crate::ChimeraParams;

const SIZE: (u32, u32) = (1280, 760);

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

        let host = GuiHost::new(self.params.clone(), context);
        let bridge = Arc::new(Bridge::new(self.params.clone(), self.shared.clone(), Box::new(host)));

        let serving = bridge.clone();
        let protocol = move |_id: wry::WebViewId, req: Request<Vec<u8>>| {
            let reply = serving.request(req.uri().path());
            Response::builder().status(reply.status).header("Content-Type", reply.mime).header("Cache-Control", "no-store").body(reply.body).unwrap()
        };
        let commands = bridge.clone();
        let ipc = move |req: Request<String>| commands.command(req.body());

        let built = WebViewBuilder::new()
            .with_bounds(Rect { position: LogicalPosition::new(0, 0).into(), size: LogicalSize::new(SIZE.0, SIZE.1).into() })
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
            Ok(webview) => Box::new(Open { bridge, _webview: webview }),
            Err(err) => {
                nih_error!("CHIMERA: could not create the webview: {err}");
                Box::new(())
            }
        }
    }

    fn size(&self) -> (u32, u32) {
        SIZE
    }

    fn set_scale_factor(&self, _factor: f32) -> bool {
        false // macOS scales for us
    }

    // the page polls the parameters, so host-side changes need no push
    fn param_value_changed(&self, _id: &str, _normalized_value: f32) {}
    fn param_modulation_changed(&self, _id: &str, _modulation_offset: f32) {}
    fn param_values_changed(&self) {}
}
