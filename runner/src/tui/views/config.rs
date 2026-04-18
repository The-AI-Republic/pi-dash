use ratatui::layout::Rect;
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

use crate::tui::app::AppState;

pub fn render(f: &mut ratatui::Frame<'_>, area: Rect, state: &AppState) {
    let text = state
        .config_blob
        .as_ref()
        .map(|v| serde_json::to_string_pretty(v).unwrap_or_else(|e| e.to_string()))
        .unwrap_or_else(|| "Config not loaded.".to_string());
    let p = Paragraph::new(text)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Configuration "),
        )
        .wrap(Wrap { trim: false });
    f.render_widget(p, area);
}
