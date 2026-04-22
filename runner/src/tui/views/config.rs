use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

use crate::tui::app::AppState;

pub fn render(f: &mut ratatui::Frame<'_>, area: Rect, state: &AppState) {
    let (text, style) = if let Some(v) = state.config_blob.as_ref() {
        (
            serde_json::to_string_pretty(v).unwrap_or_else(|e| e.to_string()),
            Style::default(),
        )
    } else if state.onboarding_needed {
        (
            "No config file yet.\n\nRun `pidash configure --url ... --token ...` to create one."
                .to_string(),
            Style::default().fg(Color::Yellow),
        )
    } else if state.daemon_offline {
        (
            "Runner daemon not running.\n\nStart it with `pidash service start`\n(or run `pidash run` in a terminal for foreground debugging)."
                .to_string(),
            Style::default().fg(Color::Yellow),
        )
    } else if let Some(err) = state.config_error.as_ref() {
        (
            format!("Failed to load config:\n{err}"),
            Style::default().fg(Color::Red),
        )
    } else {
        ("Loading config…".to_string(), Style::default())
    };
    let p = Paragraph::new(text)
        .style(style)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Configuration "),
        )
        .wrap(Wrap { trim: false });
    f.render_widget(p, area);
}
