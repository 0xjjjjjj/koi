//! Cross-platform PTY spawn smoke test.
//!
//! Exercises alacritty_terminal's `tty::new` — the same call koi uses to
//! spawn a shell — on the real host tty layer:
//!   - Windows: ConPTY spawning `powershell.exe` (the crate's default).
//!   - macOS/Linux: `posix_openpt` + fork/exec of the user's shell
//!     (falling back to `/bin/sh`).
//!
//! Passing means the child was created without an OS error. This is the
//! narrow contract the K5 acceptance criteria call for: prove headless
//! shell-spawn works on both platforms. The test intentionally does not
//! drive an event loop or read/write the pipes — that path is covered by
//! real usage and adds flake surface for CI (Windows GHA images vary in
//! what conpty.dll gets loaded).

use alacritty_terminal::event::WindowSize;
use alacritty_terminal::tty::{self, Options};

fn window_size() -> WindowSize {
    WindowSize { num_lines: 24, num_cols: 80, cell_width: 8, cell_height: 16 }
}

#[test]
fn spawns_default_shell() {
    tty::setup_env();

    let opts = Options::default();
    let pty = tty::new(&opts, window_size(), 0)
        .expect("tty::new should spawn the default shell on this platform");

    // Drop immediately — we only care that spawn succeeded. On Windows this
    // triggers ClosePseudoConsole; on Unix, Pty::drop SIGHUPs the child.
    drop(pty);
}
