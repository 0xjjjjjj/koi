use std::borrow::Cow;

use alacritty_terminal::event::{Event as TermEvent, EventListener, Notify, WindowSize};
use alacritty_terminal::event_loop::Msg;
use winit::event_loop::EventLoopProxy;

/// Custom event sent from terminal threads to the winit event loop.
#[derive(Debug)]
pub enum KoiEvent {
    /// Terminal content changed, needs redraw.
    Wakeup,
    /// Terminal title changed.
    Title(String),
    /// Child process exited (pane_id, exit_code).
    ChildExit(usize, i32),
    /// Terminal bell.
    Bell,
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
            TermEvent::Title(title) => KoiEvent::Title(title),
            TermEvent::ChildExit(code) => KoiEvent::ChildExit(self.pane_id, code),
            TermEvent::Bell => KoiEvent::Bell,
            // Security: intentionally block these events.
            // - ClipboardStore/Load: blocks OSC 52 clipboard exfiltration
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

    pub fn send_resize(&self, size: WindowSize) {
        let _ = self.0.send(Msg::Resize(size));
    }
}
