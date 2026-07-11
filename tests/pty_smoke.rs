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

    // Pty::drop SIGHUPs the bare pid; on headless CI that leaves pgroup
    // children alive and wait4 blocks. K0-style pgroup kill instead.
    #[cfg(unix)]
    {
        let pid = pty.child().id() as i32;
        if pid > 0 {
            unsafe { libc::kill(-pid, libc::SIGKILL); }
        }
    }

    drop(pty);
}
