use alacritty_terminal::grid::Dimensions;

/// Default scrollback history size (lines).
const DEFAULT_HISTORY_SIZE: usize = 10_000;

/// Terminal dimensions implementing alacritty_terminal's Dimensions trait.
pub struct TerminalSize {
    pub columns: usize,
    pub screen_lines: usize,
    pub history_size: usize,
}

impl TerminalSize {
    pub fn new(columns: usize, screen_lines: usize) -> Self {
        Self { columns, screen_lines, history_size: DEFAULT_HISTORY_SIZE }
    }
}

impl Dimensions for TerminalSize {
    fn total_lines(&self) -> usize {
        self.screen_lines + self.history_size
    }

    fn screen_lines(&self) -> usize {
        self.screen_lines
    }

    fn columns(&self) -> usize {
        self.columns
    }
}
