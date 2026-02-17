# Koi v2: Optimizations & Features Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Upgrade Koi's rendering quality (subpixel AA), performance (skip redundant redraws), and usability (mouse reporting, Cmd+W, .app bundle).

**Architecture:** 5 independent features, each touching different subsystems. Subpixel AA is the largest (atlas+shader+glyph_cache). Mouse reporting and Cmd+W are main.rs changes. .app bundle is a build-system addition.

**Tech Stack:** Rust, OpenGL 3.3 (dual-source blending), crossfont (CoreText backend), alacritty_terminal, winit, glutin

---

### Task 1: Subpixel LCD Antialiasing

The biggest visual quality improvement. Changes atlas from RED (1-channel) to RGB (3-channel) and uses dual-source blending for per-subpixel alpha.

**Files:**
- Modify: `src/renderer/atlas.rs` (texture format RED→RGB)
- Modify: `src/renderer/glyph_cache.rs` (keep RGB data instead of averaging)
- Modify: `src/renderer/text.rs` (new shader with dual-source blending)
- Modify: `src/renderer/mod.rs` (pass glyph color differently)

**Step 1: Change atlas texture format from RED to RGB**

In `atlas.rs`, change `gl::RED` to `gl::RGB` in both `TexImage2D` (line 42,46) and `TexSubImage2D` (line 114):

```rust
// atlas.rs new() — change internal format and upload format
gl::TexImage2D(
    gl::TEXTURE_2D, 0,
    gl::RGB8 as i32,  // was: gl::RED as i32
    size, size, 0,
    gl::RGB,          // was: gl::RED
    gl::UNSIGNED_BYTE,
    std::ptr::null(),
);

// atlas.rs insert() — change upload format
gl::TexSubImage2D(
    gl::TEXTURE_2D, 0,
    x, y, glyph_width, glyph_height,
    gl::RGB,          // was: gl::RED
    gl::UNSIGNED_BYTE,
    buffer.as_ptr() as *const _,
);
```

Also set `gl::PixelStorei(gl::UNPACK_ALIGNMENT, 1)` before the SubImage2D call in insert() since RGB rows may not be 4-byte aligned.

**Step 2: Keep RGB glyph data in glyph_cache.rs**

In `glyph_cache.rs`, stop converting to single-channel. The RGB data from crossfont IS the subpixel coverage:

```rust
let buffer: Vec<u8> = match &rasterized.buffer {
    BitmapBuffer::Rgb(data) => {
        // Keep RGB subpixel coverage data as-is
        data.clone()
    }
    BitmapBuffer::Rgba(data) => {
        // Convert RGBA to RGB (drop alpha, use color channels as coverage)
        data.chunks(4).flat_map(|rgba| [rgba[0], rgba[1], rgba[2]]).collect()
    }
};
```

**Step 3: New fragment shader with dual-source blending**

Replace the fragment shader in `text.rs`:

```glsl
#version 330 core

uniform sampler2D uAtlas;

in vec2 vUV;
flat in vec4 vColor;

layout(location = 0, index = 0) out vec4 fragColor;
layout(location = 0, index = 1) out vec4 blendWeights;

void main() {
    vec3 coverage = texture(uAtlas, vUV).rgb;
    // Text color * per-channel coverage
    fragColor = vec4(vColor.rgb * coverage, 1.0);
    // Blend weights = coverage per channel
    blendWeights = vec4(coverage, 1.0);
}
```

**Step 4: Enable dual-source blending in flush()**

In `text.rs` flush(), change the blend function:

```rust
gl::Enable(gl::BLEND);
gl::BlendFunc(gl::SRC1_COLOR, gl::ONE_MINUS_SRC1_COLOR);
// was: gl::BlendFunc(gl::SRC_ALPHA, gl::ONE_MINUS_SRC_ALPHA);
```

**Step 5: Build and test**

Run: `cargo build 2>&1`
Expected: Compiles clean. Text should look noticeably crisper with visible subpixel coloring on LCD screens.

**Step 6: Commit**

```bash
git add src/renderer/atlas.rs src/renderer/glyph_cache.rs src/renderer/text.rs
git commit -m "feat: subpixel LCD antialiasing with dual-source blending"
```

---

### Task 2: Skip Redundant Redraws

Instead of full dirty-rect tracking (complex, Alacritty doesn't do it either), add a frame skip when nothing has changed.

**Files:**
- Modify: `src/main.rs` (track dirty flag, skip identical redraws)

**Step 1: Add dirty tracking**

Add a `needs_redraw: bool` field to the `Koi` struct. Set it to `true` on:
- Any KoiEvent::Wakeup (terminal output changed)
- Any keyboard input
- Any mouse input
- Window resize
- Tab/pane changes

In RedrawRequested, only do the full render if `needs_redraw` is true. After rendering, set it to false.

For cursor blink: request_redraw on a timer but only if blink state changed.

**Step 2: Commit**

```bash
git add src/main.rs
git commit -m "perf: skip redundant redraws with dirty flag"
```

---

### Task 3: Mouse Reporting (DECSET modes)

Forward mouse events to the PTY when the running app has requested mouse tracking (vim, htop, less, etc.).

**Files:**
- Modify: `src/main.rs` (forward mouse events as escape sequences)

**Step 1: Understand the protocol**

alacritty_terminal tracks terminal modes internally. The Term has a `mode()` method returning `TermMode` flags:
- `TermMode::SGR_MOUSE` — SGR extended mouse reporting
- `TermMode::MOUSE_REPORT_CLICK` — basic click reporting
- `TermMode::MOUSE_DRAG` — drag reporting
- `TermMode::MOUSE_MOTION` — all motion reporting

When these modes are active, forward mouse events as escape sequences instead of handling them ourselves (no selection, no click-to-focus while mouse reporting is active).

**Step 2: Implement mouse event forwarding**

In the MouseInput and CursorMoved handlers, check if the active pane's term has mouse reporting enabled. If so, encode the event as:

SGR format: `\x1b[<button;col;row;M` (press) or `\x1b[<button;col;row;m` (release)

Where button is: 0=left, 1=middle, 2=right, 32+button=motion, 64=scroll up, 65=scroll down.

**Step 3: Handle scroll in mouse-reporting apps**

When SGR_MOUSE is active, scroll events should send mouse button 64/65 instead of scroll_display().

**Step 4: Commit**

```bash
git add src/main.rs
git commit -m "feat: mouse reporting (SGR) for vim, htop, less"
```

---

### Task 4: Cmd+W Close Pane/Tab

Standard macOS shortcut.

**Files:**
- Modify: `src/main.rs` (add Cmd+W handler)

**Step 1: Add Cmd+W handler**

In the Cmd+key match block, add:

```rust
Key::Character(ref s) if s == "w" => {
    if let Some(tab_manager) = &mut self.tab_manager {
        if tab_manager.close_active_pane() {
            event_loop.exit();
            return;
        }
    }
    if let Some(w) = &self.window {
        w.request_redraw();
    }
    return;
}
```

This closes the active pane. If it's the last pane in the last tab, it exits the app.

**Step 2: Commit**

```bash
git add src/main.rs
git commit -m "feat: Cmd+W close active pane/tab"
```

---

### Task 5: macOS .app Bundle

Wrap the binary in a proper .app bundle so it shows in Spotlight, Dock, and has an icon.

**Files:**
- Create: `bundle/Info.plist` (app metadata)
- Create: `bundle/build-app.sh` (build script)
- Create: `bundle/koi.icns` (app icon — can use a placeholder)

**Step 1: Create Info.plist**

```xml
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key>
    <string>Koi</string>
    <key>CFBundleDisplayName</key>
    <string>Koi</string>
    <key>CFBundleIdentifier</key>
    <string>com.koi.terminal</string>
    <key>CFBundleVersion</key>
    <string>0.1.0</string>
    <key>CFBundleExecutable</key>
    <string>koi</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>NSHighResolutionCapable</key>
    <true/>
    <key>LSMinimumSystemVersion</key>
    <string>12.0</string>
</dict>
</plist>
```

**Step 2: Create build-app.sh**

```bash
#!/bin/bash
set -e
cargo build --release
APP="target/Koi.app"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS"
mkdir -p "$APP/Contents/Resources"
cp target/release/koi "$APP/Contents/MacOS/koi"
cp bundle/Info.plist "$APP/Contents/Info.plist"
# cp bundle/koi.icns "$APP/Contents/Resources/koi.icns"  # when icon exists
echo "Built $APP"
```

**Step 3: Commit**

```bash
git add bundle/
git commit -m "feat: macOS .app bundle for Spotlight/Dock"
```

---

## Execution Order

Tasks 1-5 are independent and can be executed in parallel by kraken agents:
- Task 1 (subpixel): touches renderer/* only
- Task 2 (dirty flag): touches main.rs only (different section from Task 3/4)
- Task 3 (mouse reporting): touches main.rs mouse handlers
- Task 4 (Cmd+W): touches main.rs key handlers
- Task 5 (.app bundle): touches only new files in bundle/

**Parallel groups:**
- Group A: Task 1 (subpixel) — renderer files only
- Group B: Task 5 (.app bundle) — new files only
- Sequential: Tasks 2, 3, 4 (all touch main.rs — apply in order)
