use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Tabs};
use std::io;
use std::time::Duration;

use super::ipc_client::TuiIpc;
use super::views::{approvals, config as config_view, runner_status, runs};
use crate::util::paths::Paths;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    /// Daemon health + start/stop controls. First tab so `pidash tui` lands
    /// here and the user immediately sees whether the runner is up.
    RunnerStatus,
    /// Config editor, now the primary reason a user opens the TUI. Moved to
    /// position 2 because config editing is the most common flow once the
    /// service is running.
    Config,
    Runs,
    Approvals,
}

impl Tab {
    pub fn all() -> [Tab; 4] {
        [Tab::RunnerStatus, Tab::Config, Tab::Runs, Tab::Approvals]
    }

    pub fn label(&self) -> &'static str {
        match self {
            Tab::RunnerStatus => "Runner",
            Tab::Config => "Config",
            Tab::Runs => "Runs",
            Tab::Approvals => "Approvals",
        }
    }

    /// Parse `--tab` values: accepts the canonical name (`runner`, `config`,
    /// `runs`, `approvals`) or a 1-based index (`1`–`4`). Unknown input
    /// yields `None` so the caller can surface a clap-style error.
    pub fn parse_cli(raw: &str) -> Option<Tab> {
        let s = raw.trim().to_ascii_lowercase();
        match s.as_str() {
            "runner" | "runner-status" | "runner_status" | "status" | "1" => {
                Some(Tab::RunnerStatus)
            }
            "config" | "2" => Some(Tab::Config),
            "runs" | "3" => Some(Tab::Runs),
            "approvals" | "4" => Some(Tab::Approvals),
            _ => None,
        }
    }
}

pub struct AppState {
    pub tab: Tab,
    pub ipc: TuiIpc,
    pub status: Option<crate::ipc::protocol::StatusSnapshot>,
    pub runs: Vec<crate::history::index::RunSummary>,
    pub approvals: Vec<crate::approval::router::ApprovalRecord>,
    pub config_blob: Option<serde_json::Value>,
    pub config_error: Option<String>,
    pub daemon_offline: bool,
    pub error: Option<String>,
    pub quit: bool,
    pub selected: usize,
    pub onboarding_needed: bool,
    pub show_help: bool,
    pub confirm_stop: bool,
    pub confirm_exit: bool,
    pub confirm_exit_yes: bool,
    pub last_approval_count: usize,
}

pub async fn run(paths: Paths, initial_tab: Tab) -> Result<()> {
    let onboarding_needed = !paths.config_path().exists();
    let ipc = TuiIpc {
        socket: paths.ipc_socket_path(),
    };
    let mut state = AppState {
        tab: initial_tab,
        ipc,
        status: None,
        runs: Vec::new(),
        approvals: Vec::new(),
        config_blob: None,
        config_error: None,
        daemon_offline: false,
        error: None,
        quit: false,
        selected: 0,
        onboarding_needed,
        show_help: false,
        confirm_stop: false,
        confirm_exit: false,
        confirm_exit_yes: true,
        last_approval_count: 0,
    };

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    crossterm::execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = loop_ui(&mut terminal, &mut state).await;

    disable_raw_mode()?;
    crossterm::execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    result
}

async fn loop_ui(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    state: &mut AppState,
) -> Result<()> {
    refresh(state).await;
    let mut ticker = tokio::time::interval(Duration::from_millis(500));
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        terminal.draw(|f| draw(f, state))?;
        tokio::select! {
            _ = ticker.tick() => refresh(state).await,
            maybe = poll_event() => {
                if let Some(ev) = maybe {
                    handle_event(ev, state).await;
                }
            }
        }
        if state.quit {
            break;
        }
    }
    Ok(())
}

fn is_daemon_offline(err: &anyhow::Error) -> bool {
    err.chain().any(|c| {
        c.downcast_ref::<std::io::Error>().is_some_and(|io| {
            matches!(
                io.kind(),
                std::io::ErrorKind::NotFound | std::io::ErrorKind::ConnectionRefused
            )
        })
    })
}

async fn poll_event() -> Option<Event> {
    // Off-thread crossterm poll.
    tokio::task::spawn_blocking(|| {
        if event::poll(Duration::from_millis(200)).ok()? {
            event::read().ok()
        } else {
            None
        }
    })
    .await
    .ok()
    .flatten()
}

async fn refresh(state: &mut AppState) {
    match state.ipc.status().await {
        Ok(s) => state.status = Some(s),
        Err(e) => {
            state.status = None;
            state.error = Some(format!("status: {e}"));
        }
    }
    // Approvals are polled even off-tab so we can fire the bell + focus jump.
    if let Ok(v) = state.ipc.approvals().await {
        let was = state.last_approval_count;
        state.last_approval_count = v.len();
        if v.len() > was && state.tab != Tab::Approvals {
            // Bell + auto-focus.
            print!("\x07");
            state.tab = Tab::Approvals;
            state.selected = 0;
        }
        state.approvals = v;
    }
    match state.tab {
        Tab::Runs => {
            if let Ok(v) = state.ipc.runs().await {
                state.runs = v;
            }
        }
        Tab::Config => match state.ipc.config().await {
            Ok(v) => {
                state.config_blob = Some(v);
                state.config_error = None;
                state.daemon_offline = false;
            }
            Err(e) => {
                state.config_blob = None;
                if is_daemon_offline(&e) {
                    state.daemon_offline = true;
                    state.config_error = None;
                } else {
                    state.daemon_offline = false;
                    state.config_error = Some(format!("{e:#}"));
                }
            }
        },
        _ => {}
    }
}

async fn handle_event(ev: Event, state: &mut AppState) {
    if let Event::Key(key) = ev {
        if key.kind != KeyEventKind::Press {
            return;
        }
        // Help overlay consumes keys until dismissed.
        if state.show_help {
            if matches!(
                key.code,
                KeyCode::Esc | KeyCode::Char('?') | KeyCode::Char('q')
            ) {
                state.show_help = false;
            }
            return;
        }
        if state.confirm_exit {
            match (key.code, key.modifiers) {
                (KeyCode::Char('c'), KeyModifiers::CONTROL) => state.quit = true,
                (KeyCode::Enter, _) => {
                    if state.confirm_exit_yes {
                        state.quit = true;
                    } else {
                        state.confirm_exit = false;
                    }
                }
                (KeyCode::Char('y') | KeyCode::Char('Y'), _) => state.quit = true,
                (KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc, _) => {
                    state.confirm_exit = false;
                }
                (
                    KeyCode::Left | KeyCode::Right | KeyCode::Char('h') | KeyCode::Char('l'),
                    _,
                ) => {
                    state.confirm_exit_yes = !state.confirm_exit_yes;
                }
                _ => {}
            }
            return;
        }
        if state.confirm_stop {
            match key.code {
                KeyCode::Char('y') | KeyCode::Char('Y') => {
                    // Ask daemon to disconnect; then quit the TUI.
                    let _ = state
                        .ipc
                        .decide("__stop__", crate::cloud::protocol::ApprovalDecision::Accept)
                        .await
                        .ok();
                    state.quit = true;
                }
                _ => state.confirm_stop = false,
            }
            return;
        }
        match (key.code, key.modifiers) {
            (KeyCode::Char('q'), _) | (KeyCode::Char('c'), KeyModifiers::CONTROL) => {
                state.confirm_exit = true;
                state.confirm_exit_yes = true;
            }
            (KeyCode::Char('Q'), _) => state.confirm_stop = true,
            (KeyCode::Char('?'), _) => state.show_help = true,
            (KeyCode::Char('1'), _) => {
                state.tab = Tab::RunnerStatus;
                state.selected = 0;
                refresh(state).await;
            }
            (KeyCode::Char('2'), _) => {
                state.tab = Tab::Config;
                state.selected = 0;
                refresh(state).await;
            }
            (KeyCode::Char('3'), _) => {
                state.tab = Tab::Runs;
                state.selected = 0;
                refresh(state).await;
            }
            (KeyCode::Char('4'), _) => {
                state.tab = Tab::Approvals;
                state.selected = 0;
                refresh(state).await;
            }
            (KeyCode::Char('j') | KeyCode::Down, _) => {
                state.selected = state.selected.saturating_add(1);
            }
            (KeyCode::Char('k') | KeyCode::Up, _) => {
                state.selected = state.selected.saturating_sub(1);
            }
            (KeyCode::Char('h') | KeyCode::Left, _) => {
                state.tab = match state.tab {
                    Tab::RunnerStatus => Tab::Approvals,
                    Tab::Config => Tab::RunnerStatus,
                    Tab::Runs => Tab::Config,
                    Tab::Approvals => Tab::Runs,
                };
                state.selected = 0;
                refresh(state).await;
            }
            (KeyCode::Char('l') | KeyCode::Right, _) => {
                state.tab = match state.tab {
                    Tab::RunnerStatus => Tab::Config,
                    Tab::Config => Tab::Runs,
                    Tab::Runs => Tab::Approvals,
                    Tab::Approvals => Tab::RunnerStatus,
                };
                state.selected = 0;
                refresh(state).await;
            }
            (KeyCode::Char('r'), _) => refresh(state).await,
            (KeyCode::Char('a'), _) if state.tab == Tab::Approvals => {
                accept_selected(state, crate::cloud::protocol::ApprovalDecision::Accept).await;
            }
            (KeyCode::Char('A'), KeyModifiers::SHIFT) if state.tab == Tab::Approvals => {
                accept_selected(
                    state,
                    crate::cloud::protocol::ApprovalDecision::AcceptForSession,
                )
                .await;
            }
            (KeyCode::Char('d'), _) if state.tab == Tab::Approvals => {
                accept_selected(state, crate::cloud::protocol::ApprovalDecision::Decline).await;
            }
            _ => {}
        }
    }
}

async fn accept_selected(state: &mut AppState, decision: crate::cloud::protocol::ApprovalDecision) {
    if let Some(rec) = state.approvals.get(state.selected).cloned() {
        let _ = state.ipc.decide(&rec.approval_id, decision).await;
        if let Ok(v) = state.ipc.approvals().await {
            state.approvals = v;
            state.selected = 0;
        }
    }
}

fn draw(f: &mut ratatui::Frame<'_>, state: &AppState) {
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(0),
            Constraint::Length(1),
        ])
        .split(f.area());

    let titles: Vec<Line<'_>> = Tab::all()
        .iter()
        .map(|t| Line::from(Span::styled(t.label(), Style::default())))
        .collect();
    let idx = match state.tab {
        Tab::RunnerStatus => 0,
        Tab::Config => 1,
        Tab::Runs => 2,
        Tab::Approvals => 3,
    };
    let tabs = Tabs::new(titles)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Pi Dash Runner "),
        )
        .select(idx)
        .highlight_style(Style::default().add_modifier(Modifier::BOLD | Modifier::REVERSED));
    f.render_widget(tabs, layout[0]);

    match state.tab {
        Tab::RunnerStatus => runner_status::render(f, layout[1], state),
        Tab::Config => config_view::render(f, layout[1], state),
        Tab::Runs => runs::render(f, layout[1], state),
        Tab::Approvals => approvals::render(f, layout[1], state),
    }

    let hint = Line::from(Span::styled(
        " [1]Runner [2]Config [3]Runs [4]Approvals  h/l switch  j/k move  r refresh  ?help  q exit ",
        Style::default().add_modifier(Modifier::DIM),
    ));
    f.render_widget(Paragraph::new(hint), layout[2]);

    if state.show_help {
        render_help(f);
    } else if state.confirm_exit {
        render_confirm_exit(f, state.confirm_exit_yes);
    } else if state.confirm_stop {
        render_confirm_stop(f);
    }
}

fn render_help(f: &mut ratatui::Frame<'_>) {
    use ratatui::layout::Alignment;
    use ratatui::widgets::Clear;

    let area = centered_rect(60, 60, f.area());
    f.render_widget(Clear, area);
    let body = Paragraph::new(vec![
        Line::from("Pi Dash Runner — TUI help"),
        Line::raw(""),
        Line::from("1–4       jump to view"),
        Line::from("h/l ←/→   prev/next view"),
        Line::from("j/k ↑/↓   move selection"),
        Line::from("↵     open detail"),
        Line::from("r     force refresh"),
        Line::from("a     accept approval (once)"),
        Line::from("A     accept for session"),
        Line::from("d     decline"),
        Line::from("q / Ctrl+C  quit TUI (asks for confirmation)"),
        Line::from("Q           stop daemon (asks for confirmation)"),
        Line::from("?     toggle this help"),
    ])
    .alignment(Alignment::Left)
    .block(Block::default().borders(Borders::ALL).title(" Help "));
    f.render_widget(body, area);
}

fn render_confirm_exit(f: &mut ratatui::Frame<'_>, yes_selected: bool) {
    use ratatui::widgets::Clear;

    let area = centered_rect(40, 20, f.area());
    f.render_widget(Clear, area);
    let sel = Style::default().add_modifier(Modifier::REVERSED);
    let unsel = Style::default().add_modifier(Modifier::DIM);
    let (yes_style, no_style) = if yes_selected {
        (sel, unsel)
    } else {
        (unsel, sel)
    };
    let body = Paragraph::new(vec![
        Line::from("Are you sure to exit?"),
        Line::raw(""),
        Line::from(vec![
            Span::raw("  "),
            Span::styled(" Yes ", yes_style),
            Span::raw("    "),
            Span::styled(" No ", no_style),
        ]),
        Line::raw(""),
        Line::from("↵ confirm   ←/→ switch   y / n / Esc"),
    ])
    .block(Block::default().borders(Borders::ALL).title(" Exit "));
    f.render_widget(body, area);
}

fn render_confirm_stop(f: &mut ratatui::Frame<'_>) {
    use ratatui::widgets::Clear;

    let area = centered_rect(40, 20, f.area());
    f.render_widget(Clear, area);
    let body = Paragraph::new(vec![
        Line::from("Stop the runner daemon?"),
        Line::raw(""),
        Line::from("Any active run will be cancelled."),
        Line::raw(""),
        Line::from("[y] yes     [any other key] cancel"),
    ])
    .block(Block::default().borders(Borders::ALL).title(" Confirm "));
    f.render_widget(body, area);
}

fn centered_rect(
    percent_x: u16,
    percent_y: u16,
    r: ratatui::layout::Rect,
) -> ratatui::layout::Rect {
    use ratatui::layout::{Constraint, Direction, Layout};
    let popup = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r)[1];
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup)[1]
}
