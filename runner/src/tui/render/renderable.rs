//! Renderable trait — the lowest layer of the render stack.
//!
//! Shape ported from `codex-rs/tui/src/render/renderable.rs` (Apache-2.0)
//! but trimmed to what the runner needs (no width-aware desired_height
//! caching, no combinators yet — we add them when we need them).

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

/// Render a widget into `buf` covering `area`. Widgets compose by
/// recursively calling `render` on their children with sub-rects.
pub trait Renderable {
    fn render(&self, area: Rect, buf: &mut Buffer);

    /// Width-aware preferred height. Defaults to 1 — widgets that want
    /// the layout layer to size them based on content override this.
    fn desired_height(&self, _width: u16) -> u16 {
        1
    }

    /// Cursor position the terminal should park at this frame, in
    /// absolute screen coordinates. `None` means "I don't claim the
    /// cursor; let a sibling or parent place it." Exactly one widget
    /// per frame should return `Some` (claudy's `useDeclaredCursor`
    /// rule). The runner today only ever has one focused text input,
    /// so there's no node-identity check.
    fn cursor_pos(&self, _area: Rect) -> Option<(u16, u16)> {
        None
    }
}

impl Renderable for () {
    fn render(&self, _area: Rect, _buf: &mut Buffer) {}
    fn desired_height(&self, _width: u16) -> u16 {
        0
    }
}

impl<R: Renderable + ?Sized> Renderable for Box<R> {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        (**self).render(area, buf);
    }
    fn desired_height(&self, width: u16) -> u16 {
        (**self).desired_height(width)
    }
    fn cursor_pos(&self, area: Rect) -> Option<(u16, u16)> {
        (**self).cursor_pos(area)
    }
}
