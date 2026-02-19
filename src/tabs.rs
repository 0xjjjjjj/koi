use std::collections::HashMap;
use std::sync::Arc;
use alacritty_terminal::event::WindowSize;
use alacritty_terminal::event_loop::{EventLoop as PtyEventLoop, Msg, State as PtyState};
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::{Config as TermConfig, Term};
use alacritty_terminal::tty;

use crate::event::{EventProxy, Notifier};
use crate::panes::{PaneLayout, PaneTree, Split};
use crate::terminal::TerminalSize;

type PtyJoinHandle = std::thread::JoinHandle<(PtyEventLoop<tty::Pty, EventProxy>, PtyState)>;

/// A terminal pane with its own Term + PTY.
pub struct Pane {
    pub term: Arc<FairMutex<Term<EventProxy>>>,
    pub notifier: Notifier,
    _pty_thread: Option<PtyJoinHandle>,
}

impl Drop for Pane {
    fn drop(&mut self) {
        // Send shutdown, then join the PTY thread to release the Term mutex.
        let _ = self.notifier.0.send(Msg::Shutdown);
        if let Some(handle) = self._pty_thread.take() {
            let _ = handle.join();
        }
    }
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

        let pane_proxy = event_proxy.with_pane_id(id);

        let term_size = TerminalSize::new(cols, rows);
        let term = Term::new(TermConfig::default(), &term_size, pane_proxy.clone());
        let term = Arc::new(FairMutex::new(term));

        let window_size = WindowSize {
            num_lines: rows as u16,
            num_cols: cols as u16,
            cell_width: cell_width as u16,
            cell_height: cell_height as u16,
        };
        let pty_opts = tty::Options {
            working_directory: std::env::var_os("HOME").map(std::path::PathBuf::from),
            ..tty::Options::default()
        };
        let pty = tty::new(&pty_opts, window_size, 0).expect("create PTY");

        let pty_event_loop = PtyEventLoop::new(
            term.clone(),
            pane_proxy,
            pty,
            false,
            false,
        )
        .expect("create PTY event loop");

        let notifier = Notifier(pty_event_loop.channel());
        let pty_thread = pty_event_loop.spawn();

        (id, Pane { term, notifier, _pty_thread: Some(pty_thread) })
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

    pub fn active_tab(&self) -> Option<&Tab> {
        self.tabs.get(self.active)
    }

    /// Set the title of the tab containing the given pane.
    pub fn set_tab_title_by_pane(&mut self, pane_id: usize, title: String) {
        for tab in &mut self.tabs {
            if tab.panes.contains_key(&pane_id) {
                tab.title = title;
                return;
            }
        }
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
        let tab = self.active_tab()?;
        let pane_id = tab.pane_tree.active_pane_id();
        tab.panes.get(&pane_id)
    }

    /// Split the active pane in the active tab, then resize all panes to fit.
    pub fn split_active(
        &mut self,
        split: Split,
        cols: usize,
        rows: usize,
        cell_width: f32,
        cell_height: f32,
        viewport_width: f32,
        viewport_height: f32,
        event_proxy: &EventProxy,
    ) {
        let (new_id, pane) = self.spawn_pane(cols, rows, cell_width, cell_height, event_proxy);
        let tab = &mut self.tabs[self.active];
        tab.pane_tree.split_active(split, new_id);
        tab.panes.insert(new_id, pane);
        // Resize all panes to their actual layout dimensions
        Self::resize_tab_panes(tab, viewport_width, viewport_height, cell_width, cell_height);
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

    pub fn focus_pane(&mut self, pane_id: usize) {
        self.tabs[self.active].pane_tree.set_active(pane_id);
    }

    pub fn focus_next_pane(&mut self) {
        self.tabs[self.active].pane_tree.focus_next();
    }

    pub fn focus_prev_pane(&mut self) {
        self.tabs[self.active].pane_tree.focus_prev();
    }

    /// Get pane layouts for the active tab.
    pub fn active_layouts(&self, width: f32, height: f32) -> Vec<PaneLayout> {
        match self.active_tab() {
            Some(tab) => tab.pane_tree.calculate_layouts(width, height),
            None => Vec::new(),
        }
    }

    /// Close a specific pane by ID (e.g., when its shell exits).
    /// Returns true if the app should quit (last pane in last tab).
    pub fn close_pane_by_id(&mut self, pane_id: usize) -> bool {
        // Find which tab contains this pane
        let tab_idx = self.tabs.iter().position(|tab| tab.panes.contains_key(&pane_id));
        let Some(tab_idx) = tab_idx else {
            return false; // Pane already gone
        };

        let tab = &mut self.tabs[tab_idx];

        // If this pane is active, use close_active logic
        if tab.pane_tree.active_pane_id() == pane_id {
            // Temporarily switch to this tab for close_active_pane
            let prev_active = self.active;
            self.active = tab_idx;
            let result = self.close_active_pane();
            // If tab was removed and we were on a different tab, adjust
            if !result && prev_active < self.tabs.len() && prev_active != tab_idx {
                self.active = if prev_active > tab_idx {
                    prev_active - 1
                } else {
                    prev_active
                };
            }
            return result;
        }

        // Pane is not the active one — remove it from the tree and HashMap
        if tab.pane_tree.pane_count() <= 1 {
            // Last pane — close the tab
            if let Some(pane) = tab.panes.remove(&pane_id) {
                let _ = pane.notifier.0.send(Msg::Shutdown);
            }
            if self.tabs.len() <= 1 {
                return true;
            }
            self.tabs.remove(tab_idx);
            if self.active >= self.tabs.len() {
                self.active = self.tabs.len() - 1;
            }
            return false;
        }

        // Remove from tree (need to manipulate active temporarily)
        let saved_active = tab.pane_tree.active_pane_id();
        // Set active to the target so close_active removes it
        tab.pane_tree.set_active(pane_id);
        tab.pane_tree.close_active();
        // Restore focus
        tab.pane_tree.set_active(saved_active);

        if let Some(pane) = tab.panes.remove(&pane_id) {
            let _ = pane.notifier.0.send(Msg::Shutdown);
        }
        false
    }

    /// Resize panes in a single tab based on their layout dimensions.
    fn resize_tab_panes(tab: &Tab, width: f32, height: f32, cell_width: f32, cell_height: f32) {
        let layouts = tab.pane_tree.calculate_layouts(width, height);
        for layout in &layouts {
            if let Some(pane) = tab.panes.get(&layout.pane_id) {
                let cols = (layout.width / cell_width) as usize;
                let rows = (layout.height / cell_height) as usize;
                let cols = cols.max(2);
                let rows = rows.max(1);
                pane.term.lock().resize(TerminalSize::new(cols, rows));
                let window_size = WindowSize {
                    num_lines: rows as u16,
                    num_cols: cols as u16,
                    cell_width: cell_width as u16,
                    cell_height: cell_height as u16,
                };
                pane.notifier.send_resize(window_size);
            }
        }
    }

    /// Resize all panes in all tabs using per-pane layout dimensions.
    pub fn resize_all(&self, width: f32, height: f32, cell_width: f32, cell_height: f32) {
        for tab in &self.tabs {
            Self::resize_tab_panes(tab, width, height, cell_width, cell_height);
        }
    }
}
