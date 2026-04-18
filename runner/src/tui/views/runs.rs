use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState};

use crate::tui::app::AppState;

pub fn render(f: &mut ratatui::Frame<'_>, area: Rect, state: &AppState) {
    let items: Vec<ListItem<'_>> = state
        .runs
        .iter()
        .map(|r| {
            let title = r.title.as_deref().unwrap_or("(no title)");
            ListItem::new(format!(
                "{}  {:>9}  {}",
                r.started_at.format("%m-%d %H:%M"),
                r.status,
                title
            ))
        })
        .collect();
    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Recent runs "),
        )
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED));
    let mut lstate = ListState::default();
    if !state.runs.is_empty() {
        lstate.select(Some(state.selected.min(state.runs.len().saturating_sub(1))));
    }
    f.render_stateful_widget(list, area, &mut lstate);
}
