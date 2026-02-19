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

#[cfg(test)]
mod tests {
    use super::*;
    use alacritty_terminal::grid::Dimensions;

    #[test]
    fn new_sets_dimensions() {
        let size = TerminalSize::new(80, 24);
        assert_eq!(size.columns(), 80);
        assert_eq!(size.screen_lines(), 24);
    }

    #[test]
    fn total_lines_includes_history() {
        let size = TerminalSize::new(80, 24);
        assert_eq!(size.total_lines(), 24 + 10_000);
    }
}
