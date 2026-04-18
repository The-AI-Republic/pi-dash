use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap};

use crate::tui::app::AppState;

pub fn render(f: &mut ratatui::Frame<'_>, area: Rect, state: &AppState) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
        .split(area);

    let items: Vec<ListItem<'_>> = state
        .approvals
        .iter()
        .map(|r| {
            ListItem::new(format!(
                "{}  {:?}",
                r.requested_at.format("%H:%M:%S"),
                r.kind
            ))
        })
        .collect();
    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title(" Pending "))
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED));
    let mut lstate = ListState::default();
    if !state.approvals.is_empty() {
        lstate.select(Some(
            state.selected.min(state.approvals.len().saturating_sub(1)),
        ));
    }
    f.render_stateful_widget(list, chunks[0], &mut lstate);

    let detail_text = state
        .approvals
        .get(state.selected)
        .map(|r| {
            let pretty = serde_json::to_string_pretty(&r.payload).unwrap_or_default();
            let reason = r.reason.clone().unwrap_or_default();
            format!(
                "approval_id: {}\nrun_id: {}\nkind: {:?}\nreason: {}\n\npayload:\n{}",
                r.approval_id, r.run_id, r.kind, reason, pretty
            )
        })
        .unwrap_or_else(|| "Select an approval (j/k) to see details.".to_string());

    let lines = vec![
        Line::from(detail_text),
        Line::raw(""),
        Line::from("[a] Accept   [A] Accept for session   [d] Decline"),
    ];
    let detail = Paragraph::new(lines)
        .block(Block::default().borders(Borders::ALL).title(" Detail "))
        .wrap(Wrap { trim: false });
    f.render_widget(detail, chunks[1]);
}
