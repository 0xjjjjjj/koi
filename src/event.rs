use std::borrow::Cow;
use std::sync::Arc;

use alacritty_terminal::event::{Event as TermEvent, EventListener, Notify, WindowSize};
use alacritty_terminal::event_loop::Msg;
use winit::event_loop::EventLoopProxy;

/// Custom event sent from terminal threads to the winit event loop.
pub enum KoiEvent {
    /// Terminal content changed, needs redraw.
    Wakeup,
    /// Terminal title changed (title, pane_id).
    Title(String, usize),
    /// Child process exited (pane_id, exit_code).
    ChildExit(usize, i32),
    /// Terminal bell.
    Bell,
    /// OSC 52: remote app wants to set the local clipboard.
    ClipboardStore(String),
    /// OSC 52: remote app wants to read the local clipboard (pane_id, formatter).
    ClipboardLoad(usize, Arc<dyn Fn(&str) -> String + Sync + Send + 'static>),
}

impl std::fmt::Debug for KoiEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Wakeup => write!(f, "Wakeup"),
            Self::Title(t, id) => write!(f, "Title({t}, {id})"),
            Self::ChildExit(id, code) => write!(f, "ChildExit({id}, {code})"),
            Self::Bell => write!(f, "Bell"),
            Self::ClipboardStore(text) => write!(f, "ClipboardStore({text})"),
            Self::ClipboardLoad(id, _) => write!(f, "ClipboardLoad({id})"),
        }
    }
}

/// Bridges alacritty_terminal events to winit's event loop.
#[derive(Clone)]
pub struct EventProxy {
    proxy: EventLoopProxy<KoiEvent>,
    pane_id: usize,
}

impl EventProxy {
    pub fn new(proxy: EventLoopProxy<KoiEvent>) -> Self {
        Self { proxy, pane_id: 0 }
    }

    /// Create a proxy tagged with a specific pane ID.
    pub fn with_pane_id(&self, pane_id: usize) -> Self {
        Self {
            proxy: self.proxy.clone(),
            pane_id,
        }
    }
}

impl EventListener for EventProxy {
    fn send_event(&self, event: TermEvent) {
        let koi_event = match event {
            TermEvent::Wakeup => KoiEvent::Wakeup,
            TermEvent::Title(title) => KoiEvent::Title(title, self.pane_id),
            TermEvent::ChildExit(code) => KoiEvent::ChildExit(self.pane_id, code),
            TermEvent::Bell => KoiEvent::Bell,
            // OSC 52: remote app sets local clipboard (e.g. vim yank over SSH).
            TermEvent::ClipboardStore(_, text) => KoiEvent::ClipboardStore(text),
            // OSC 52: remote app reads local clipboard.
            TermEvent::ClipboardLoad(_, formatter) => KoiEvent::ClipboardLoad(self.pane_id, formatter),
            // Security: intentionally block these events.
            // - PtyWrite: blocks DECRQSS echo-back attacks
            // - ColorRequest: blocks terminal color information leaks
            _ => return,
        };
        let _ = self.proxy.send_event(koi_event);
    }
}

/// Writes input to the PTY via the event loop channel.
pub struct Notifier(pub alacritty_terminal::event_loop::EventLoopSender);

impl Notify for Notifier {
    fn notify<B: Into<Cow<'static, [u8]>>>(&self, bytes: B) {
        let _ = self.0.send(Msg::Input(bytes.into()));
    }
}

impl Notifier {
    pub fn send_input(&self, data: &[u8]) {
        let _ = self.0.send(Msg::Input(Cow::Owned(data.to_vec())));
    }

    /// Send owned bytes without copying — use with format!().into_bytes().
    pub fn send_bytes(&self, data: Vec<u8>) {
        let _ = self.0.send(Msg::Input(Cow::Owned(data)));
    }

    pub fn send_resize(&self, size: WindowSize) {
        let _ = self.0.send(Msg::Resize(size));
    }
}
