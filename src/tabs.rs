use std::sync::Arc;

use alacritty_terminal::event::WindowSize;
use alacritty_terminal::event_loop::{EventLoop as PtyEventLoop, Msg};
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::{Config as TermConfig, Term};
use alacritty_terminal::tty;

use crate::event::{EventProxy, Notifier};
use crate::terminal::TerminalSize;

pub struct Tab {
    pub title: String,
    pub term: Arc<FairMutex<Term<EventProxy>>>,
    pub notifier: Notifier,
}

pub struct TabManager {
    tabs: Vec<Tab>,
    active: usize,
}

impl TabManager {
    /// Create a new tab manager with one initial tab.
    pub fn new(
        cols: usize,
        rows: usize,
        cell_width: f32,
        cell_height: f32,
        event_proxy: &EventProxy,
    ) -> Self {
        let mut mgr = TabManager {
            tabs: Vec::new(),
            active: 0,
        };
        mgr.add_tab(cols, rows, cell_width, cell_height, event_proxy);
        mgr
    }

    /// Add a new tab, spawning a fresh terminal + PTY. Returns the new tab index.
    pub fn add_tab(
        &mut self,
        cols: usize,
        rows: usize,
        cell_width: f32,
        cell_height: f32,
        event_proxy: &EventProxy,
    ) -> usize {
        let term_size = TerminalSize::new(cols, rows);
        let term = Term::new(TermConfig::default(), &term_size, event_proxy.clone());
        let term = Arc::new(FairMutex::new(term));

        let window_size = WindowSize {
            num_lines: rows as u16,
            num_cols: cols as u16,
            cell_width: cell_width as u16,
            cell_height: cell_height as u16,
        };
        let pty = tty::new(&tty::Options::default(), window_size, 0).expect("create PTY");

        let pty_event_loop = PtyEventLoop::new(
            term.clone(),
            event_proxy.clone(),
            pty,
            false,
            false,
        )
        .expect("create PTY event loop");

        let notifier = Notifier(pty_event_loop.channel());
        let _pty_thread = pty_event_loop.spawn();

        let tab = Tab {
            title: format!("Tab {}", self.tabs.len() + 1),
            term,
            notifier,
        };

        self.tabs.push(tab);
        let idx = self.tabs.len() - 1;
        self.active = idx;
        idx
    }

    /// Close the active tab. Returns true if it was the last tab (app should quit).
    pub fn close_active(&mut self) -> bool {
        if self.tabs.len() <= 1 {
            // Shutdown the last tab's PTY
            if let Some(tab) = self.tabs.first() {
                let _ = tab.notifier.0.send(Msg::Shutdown);
            }
            return true;
        }

        // Shutdown this tab's PTY
        let _ = self.tabs[self.active].notifier.0.send(Msg::Shutdown);
        self.tabs.remove(self.active);

        if self.active >= self.tabs.len() {
            self.active = self.tabs.len() - 1;
        }
        false
    }

    pub fn next_tab(&mut self) {
        if self.tabs.len() > 1 {
            self.active = (self.active + 1) % self.tabs.len();
        }
    }

    pub fn prev_tab(&mut self) {
        if self.tabs.len() > 1 {
            self.active = if self.active == 0 {
                self.tabs.len() - 1
            } else {
                self.active - 1
            };
        }
    }

    pub fn goto_tab(&mut self, index: usize) {
        if index < self.tabs.len() {
            self.active = index;
        }
    }

    pub fn active_tab(&self) -> &Tab {
        &self.tabs[self.active]
    }

    pub fn active_index(&self) -> usize {
        self.active
    }

    pub fn count(&self) -> usize {
        self.tabs.len()
    }

    pub fn iter(&self) -> impl Iterator<Item = &Tab> {
        self.tabs.iter()
    }

    /// Resize all tabs' terminals and PTYs.
    pub fn resize_all(&self, cols: usize, rows: usize, cell_width: f32, cell_height: f32) {
        let window_size = WindowSize {
            num_lines: rows as u16,
            num_cols: cols as u16,
            cell_width: cell_width as u16,
            cell_height: cell_height as u16,
        };

        for tab in &self.tabs {
            tab.term.lock().resize(TerminalSize::new(cols, rows));
            tab.notifier.send_resize(window_size);
        }
    }
}
