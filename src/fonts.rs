// IBM Plex Mono OFL 1.1 — bundled at build time so the primary koi font
// resolves on every platform regardless of user installation state.
// LICENSE.txt sits next to these bytes in bundle/fonts/ and is included in
// the source tree for OFL compliance.

pub const PLEX_MONO_REGULAR: &[u8] =
    include_bytes!("../bundle/fonts/IBMPlexMono-Regular.otf");
pub const PLEX_MONO_BOLD: &[u8] =
    include_bytes!("../bundle/fonts/IBMPlexMono-Bold.otf");
pub const PLEX_MONO_ITALIC: &[u8] =
    include_bytes!("../bundle/fonts/IBMPlexMono-Italic.otf");
pub const PLEX_MONO_BOLD_ITALIC: &[u8] =
    include_bytes!("../bundle/fonts/IBMPlexMono-BoldItalic.otf");

pub const ALL: [&[u8]; 4] = [
    PLEX_MONO_REGULAR,
    PLEX_MONO_BOLD,
    PLEX_MONO_ITALIC,
    PLEX_MONO_BOLD_ITALIC,
];
