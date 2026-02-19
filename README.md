# Koi

A GPU-accelerated terminal emulator for macOS, built with Rust.

![Koi Icon](bundle/koi-icon.png)

## Features

- **GPU-rendered text** — OpenGL instanced glyph atlas with subpixel LCD anti-aliasing
- **Tabs** — Cmd+T new tab, Cmd+W close, Shift+[ / Shift+] switch
- **Pane splitting** — binary tree layout with click-to-focus
- **Scrollback** — 10,000 line history with trackpad and mouse wheel support
- **Mouse reporting** — SGR mouse protocol for vim, tmux, etc.
- **Selection** — click and drag to select, Cmd+C to copy, Cmd+V to paste
- **Image paste** — OSC 1337 protocol (iTerm2-compatible)
- **Font zoom** — Cmd+Plus / Cmd+Minus
- **HiDPI** — Retina display support with proper DPI scaling
- **Terminal emulation** — powered by alacritty_terminal

## Keybindings

| Key | Action |
|-----|--------|
| Cmd+T | New tab |
| Cmd+W | Close pane/tab |
| Shift+[ / ] | Previous/next tab |
| Cmd+D | Split pane vertically |
| Cmd+Shift+D | Split pane horizontally |
| Cmd+Opt+Arrow | Focus pane |
| Ctrl+Tab | Next pane |
| Cmd+Plus/Minus | Zoom font |
| Cmd+C | Copy selection |
| Cmd+V | Paste |

## Build

Requires Rust toolchain.

```bash
make build     # release build
make app       # build + assemble Koi.app bundle
make install   # build + install to /Applications
make clean     # cargo clean
```

## License

MIT
