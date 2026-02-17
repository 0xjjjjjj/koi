use std::collections::HashMap;
use std::sync::Arc;

use alacritty_terminal::event::WindowSize;
use alacritty_terminal::event_loop::{EventLoop as PtyEventLoop, Msg};
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::{Config as TermConfig, Term};
use alacritty_terminal::tty;

use crate::event::{EventProxy, Notifier};
use crate::panes::{PaneLayout, PaneTree, Split};
use crate::terminal::TerminalSize;

/// A terminal pane with its own Term + PTY.
pub struct Pane {
    pub term: Arc<FairMutex<Term<EventProxy>>>,
    pub notifier: Notifier,
}

/// A tab containing a tree of panes.
pub struct Tab {
    pub title: String,
    pub pane_tree: PaneTree,
    pub panes: HashMap<usize, Pane>,
}

pub struct TabManager {
    tabs: Vec<Tab>,
    active: usize,
    next_pane_id: usize,
}

impl TabManager {
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
            next_pane_id: 0,
        };
        mgr.add_tab(cols, rows, cell_width, cell_height, event_proxy);
        mgr
    }

    fn spawn_pane(
        &mut self,
        cols: usize,
        rows: usize,
        cell_width: f32,
        cell_height: f32,
        event_proxy: &EventProxy,
    ) -> (usize, Pane) {
        let id = self.next_pane_id;
        self.next_pane_id += 1;

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

        (id, Pane { term, notifier })
    }

    /// Add a new tab with one pane.
    pub fn add_tab(
        &mut self,
        cols: usize,
        rows: usize,
        cell_width: f32,
        cell_height: f32,
        event_proxy: &EventProxy,
    ) -> usize {
        let (pane_id, pane) = self.spawn_pane(cols, rows, cell_width, cell_height, event_proxy);

        let mut panes = HashMap::new();
        panes.insert(pane_id, pane);

        let tab = Tab {
            title: format!("Tab {}", self.tabs.len() + 1),
            pane_tree: PaneTree::new(pane_id),
            panes,
        };

        self.tabs.push(tab);
        let idx = self.tabs.len() - 1;
        self.active = idx;
        idx
    }

    /// Close the active tab.
    pub fn close_active(&mut self) -> bool {
        if self.tabs.len() <= 1 {
            // Shutdown all panes in the last tab
            if let Some(tab) = self.tabs.first() {
                for pane in tab.panes.values() {
                    let _ = pane.notifier.0.send(Msg::Shutdown);
                }
            }
            return true;
        }

        let tab = self.tabs.remove(self.active);
        for pane in tab.panes.values() {
            let _ = pane.notifier.0.send(Msg::Shutdown);
        }

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

    pub fn active_tab_mut(&mut self) -> &mut Tab {
        &mut self.tabs[self.active]
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

    /// Get the active pane (in the active tab).
    pub fn active_pane(&self) -> Option<&Pane> {
        let tab = self.active_tab();
        let pane_id = tab.pane_tree.active_pane_id();
        tab.panes.get(&pane_id)
    }

    /// Split the active pane in the active tab.
    pub fn split_active(
        &mut self,
        split: Split,
        cols: usize,
        rows: usize,
        cell_width: f32,
        cell_height: f32,
        event_proxy: &EventProxy,
    ) {
        let (new_id, pane) = self.spawn_pane(cols, rows, cell_width, cell_height, event_proxy);
        let tab = &mut self.tabs[self.active];
        tab.pane_tree.split_active(split, new_id);
        tab.panes.insert(new_id, pane);
    }

    /// Close the active pane in the active tab. Returns true if the whole tab should close.
    pub fn close_active_pane(&mut self) -> bool {
        let tab = &mut self.tabs[self.active];
        let pane_id = tab.pane_tree.active_pane_id();

        if tab.pane_tree.close_active() {
            // Last pane in tab - close the tab
            return self.close_active();
        }

        // Shutdown the closed pane's PTY
        if let Some(pane) = tab.panes.remove(&pane_id) {
            let _ = pane.notifier.0.send(Msg::Shutdown);
        }
        false
    }

    pub fn toggle_zoom(&mut self) {
        self.tabs[self.active].pane_tree.toggle_zoom();
    }

    pub fn focus_next_pane(&mut self) {
        self.tabs[self.active].pane_tree.focus_next();
    }

    pub fn focus_prev_pane(&mut self) {
        self.tabs[self.active].pane_tree.focus_prev();
    }

    /// Get pane layouts for the active tab.
    pub fn active_layouts(&self, width: f32, height: f32) -> Vec<PaneLayout> {
        self.active_tab().pane_tree.calculate_layouts(width, height)
    }

    /// Resize all panes in all tabs.
    pub fn resize_all(&self, cols: usize, rows: usize, cell_width: f32, cell_height: f32) {
        let window_size = WindowSize {
            num_lines: rows as u16,
            num_cols: cols as u16,
            cell_width: cell_width as u16,
            cell_height: cell_height as u16,
        };

        for tab in &self.tabs {
            for pane in tab.panes.values() {
                pane.term.lock().resize(TerminalSize::new(cols, rows));
                pane.notifier.send_resize(window_size);
            }
        }
    }
}
