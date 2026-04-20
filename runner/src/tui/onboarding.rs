//! First-run onboarding wizard. Shown by `pi-dash-runner tui` when no
//! configuration exists on disk. Mirrors the 4-step flow from tui-design.md.

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use std::io;
use std::time::Duration;

use crate::cli::doctor;
use crate::cloud::register::{RegisterRequest, register};
use crate::config::schema::{Config, Credentials, RunnerSection, WorkspaceSection};
use crate::util::paths::Paths;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Step {
    Cloud,
    Token,
    Verify,
    Service,
    Done,
}

struct Wizard {
    paths: Paths,
    step: Step,
    cloud_url: String,
    token: String,
    name: String,
    cursor_field: u8,
    status_line: Option<String>,
    doctor_report: Option<doctor::Report>,
    install_service: bool,
    error: Option<String>,
    busy: bool,
    done_message: Option<String>,
    quit: bool,
}

/// Entry point. Returns when the wizard exits (either the user completed setup
/// or pressed Esc). Callers should inspect `paths.config_path().exists()` to
/// decide whether to launch the dashboard.
pub async fn run(paths: Paths) -> Result<()> {
    let default_name = default_hostname().unwrap_or_else(|| "runner".to_string());
    let mut state = Wizard {
        paths,
        step: Step::Cloud,
        cloud_url: "https://cloud.pi-dash.so".to_string(),
        token: String::new(),
        name: default_name,
        cursor_field: 0,
        status_line: None,
        doctor_report: None,
        install_service: true,
        error: None,
        busy: false,
        done_message: None,
        quit: false,
    };

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    crossterm::execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut term = Terminal::new(backend)?;

    let result = event_loop(&mut term, &mut state).await;

    disable_raw_mode()?;
    crossterm::execute!(term.backend_mut(), LeaveAlternateScreen)?;
    term.show_cursor()?;
    result
}

async fn event_loop(
    term: &mut Terminal<CrosstermBackend<io::Stdout>>,
    state: &mut Wizard,
) -> Result<()> {
    loop {
        term.draw(|f| draw(f, state))?;
        if state.quit {
            return Ok(());
        }
        if let Some(ev) = poll_event().await {
            handle_event(ev, state).await;
        }
    }
}

async fn poll_event() -> Option<Event> {
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

async fn handle_event(ev: Event, state: &mut Wizard) {
    let Event::Key(key) = ev else { return };
    if key.kind != KeyEventKind::Press {
        return;
    }
    // Esc anywhere exits the wizard.
    if matches!(key.code, KeyCode::Esc) {
        state.quit = true;
        return;
    }
    match state.step {
        Step::Cloud => handle_cloud_step(key.code, state),
        Step::Token => handle_token_step(key.code, state).await,
        Step::Verify => handle_verify_step(key.code, state).await,
        Step::Service => handle_service_step(key.code, state).await,
        Step::Done => {
            if matches!(key.code, KeyCode::Enter | KeyCode::Char('q')) {
                state.quit = true;
            }
        }
    }
}

fn handle_cloud_step(code: KeyCode, state: &mut Wizard) {
    match code {
        KeyCode::Enter => {
            if state.cloud_url.starts_with("http") {
                state.step = Step::Token;
                state.error = None;
            } else {
                state.error = Some("cloud url must start with https:// or http://".into());
            }
        }
        KeyCode::Backspace => {
            state.cloud_url.pop();
        }
        KeyCode::Char(c) => state.cloud_url.push(c),
        _ => {}
    }
}

async fn handle_token_step(code: KeyCode, state: &mut Wizard) {
    match code {
        KeyCode::Enter => {
            if state.cursor_field == 0 {
                // Move from token → name.
                if state.token.is_empty() {
                    state.error = Some("registration code is required".into());
                    return;
                }
                state.cursor_field = 1;
                state.error = None;
            } else {
                if state.name.is_empty() {
                    state.error = Some("runner name is required".into());
                    return;
                }
                state.error = None;
                register_and_advance(state).await;
            }
        }
        KeyCode::Tab => {
            state.cursor_field = 1 - state.cursor_field;
        }
        KeyCode::Backspace => {
            if state.cursor_field == 0 {
                state.token.pop();
            } else {
                state.name.pop();
            }
        }
        KeyCode::Char(c) => {
            if state.cursor_field == 0 {
                state.token.push(c);
            } else {
                state.name.push(c);
            }
        }
        _ => {}
    }
}

async fn register_and_advance(state: &mut Wizard) {
    state.busy = true;
    state.status_line = Some("contacting cloud...".to_string());
    let req = RegisterRequest {
        runner_name: state.name.clone(),
        os: std::env::consts::OS.to_string(),
        arch: std::env::consts::ARCH.to_string(),
        version: crate::RUNNER_VERSION.to_string(),
        protocol_version: crate::PROTOCOL_VERSION,
    };
    match register(&state.cloud_url, &state.token, &req).await {
        Ok(resp) => {
            let config = Config {
                version: 1,
                runner: RunnerSection {
                    name: state.name.clone(),
                    cloud_url: state.cloud_url.clone(),
                },
                workspace: WorkspaceSection {
                    working_dir: state.paths.default_working_dir(),
                },
                codex: Default::default(),
                approval_policy: Default::default(),
                logging: Default::default(),
            };
            if let Err(e) = crate::config::file::write_config(&state.paths, &config) {
                state.error = Some(format!("writing config: {e}"));
                state.busy = false;
                return;
            }
            let creds = Credentials {
                runner_id: resp.runner_id,
                runner_secret: resp.runner_secret,
                api_token: resp.api_token,
                issued_at: chrono::Utc::now(),
            };
            if let Err(e) = crate::config::file::write_credentials(&state.paths, &creds) {
                state.error = Some(format!("writing credentials: {e}"));
                state.busy = false;
                return;
            }
            state.status_line = Some(format!("registered as {}", resp.runner_id));
            state.busy = false;
            state.step = Step::Verify;
            // Kick off preflight immediately.
            match doctor::execute(&state.paths).await {
                Ok(r) => state.doctor_report = Some(r),
                Err(e) => state.error = Some(format!("doctor failed: {e}")),
            }
        }
        Err(e) => {
            state.error = Some(format!("{e:#}"));
            state.busy = false;
        }
    }
}

async fn handle_verify_step(code: KeyCode, state: &mut Wizard) {
    match code {
        KeyCode::Enter => {
            state.step = Step::Service;
        }
        KeyCode::Char('r') => {
            state.busy = true;
            match doctor::execute(&state.paths).await {
                Ok(r) => state.doctor_report = Some(r),
                Err(e) => state.error = Some(format!("doctor failed: {e}")),
            }
            state.busy = false;
        }
        _ => {}
    }
}

async fn handle_service_step(code: KeyCode, state: &mut Wizard) {
    match code {
        KeyCode::Char(' ') => {
            state.install_service = !state.install_service;
        }
        KeyCode::Enter => {
            state.busy = true;
            if state.install_service {
                let svc = crate::service::detect();
                if let Err(e) = svc.install(&state.paths).await {
                    state.error = Some(format!("service install failed: {e:#}"));
                    state.busy = false;
                    return;
                }
                if let Err(e) = svc.start().await {
                    state.error = Some(format!("service start failed: {e:#}"));
                    // Not fatal — user can start manually.
                }
            }
            state.done_message = Some(
                if state.install_service {
                    "Runner is installed as a service and connected.\nThe TUI dashboard will open next."
                } else {
                    "Configuration saved. Run `pi-dash-runner start` to connect."
                }
                .to_string(),
            );
            state.step = Step::Done;
            state.busy = false;
        }
        _ => {}
    }
}

fn draw(f: &mut ratatui::Frame<'_>, state: &Wizard) {
    let area = centered_rect(70, 80, f.area());
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Pi Dash Runner — Setup ");
    f.render_widget(block.clone(), area);
    let inner = Layout::default()
        .direction(Direction::Vertical)
        .margin(2)
        .constraints([
            Constraint::Length(2),
            Constraint::Min(5),
            Constraint::Length(3),
        ])
        .split(area);

    f.render_widget(header(state), inner[0]);
    f.render_widget(body(state), inner[1]);
    f.render_widget(footer(state), inner[2]);
}

fn header(state: &Wizard) -> Paragraph<'_> {
    let which = match state.step {
        Step::Cloud => "Step 1 of 4 — Cloud endpoint",
        Step::Token => "Step 2 of 4 — Paste registration code",
        Step::Verify => "Step 3 of 4 — Verify Codex / git",
        Step::Service => "Step 4 of 4 — Install as service",
        Step::Done => "All set",
    };
    Paragraph::new(Line::from(Span::styled(
        which,
        Style::default().add_modifier(Modifier::BOLD),
    )))
    .alignment(Alignment::Center)
}

fn body(state: &Wizard) -> Paragraph<'_> {
    let lines = match state.step {
        Step::Cloud => vec![
            Line::from("Where is your Pi Dash cloud deployment?"),
            Line::raw(""),
            Line::from(vec![
                Span::raw("URL: "),
                Span::styled(&state.cloud_url, Style::default().fg(Color::Cyan)),
            ]),
            Line::raw(""),
            Line::from("Press Enter to continue. Esc to cancel."),
        ],
        Step::Token => vec![
            Line::from("Generate a one-time code in the Pi Dash web UI:"),
            Line::from("  Workspace → Runners → Mint registration code"),
            Line::raw(""),
            Line::from(vec![
                Span::styled(
                    if state.cursor_field == 0 { "> " } else { "  " },
                    Style::default(),
                ),
                Span::raw("Code: "),
                Span::styled(mask(&state.token), Style::default().fg(Color::Cyan)),
            ]),
            Line::from(vec![
                Span::styled(
                    if state.cursor_field == 1 { "> " } else { "  " },
                    Style::default(),
                ),
                Span::raw("Runner name: "),
                Span::styled(state.name.as_str(), Style::default().fg(Color::Cyan)),
            ]),
            Line::raw(""),
            Line::from("Tab to switch fields · Enter to confirm · Esc to cancel"),
        ],
        Step::Verify => {
            let mut l = vec![Line::from("Checking your environment..."), Line::raw("")];
            if let Some(report) = &state.doctor_report {
                for c in &report.checks {
                    let mark = if c.ok { "✓" } else { "✗" };
                    let color = if c.ok { Color::Green } else { Color::Red };
                    l.push(Line::from(vec![
                        Span::styled(format!(" {mark} "), Style::default().fg(color)),
                        Span::raw(format!("{:<14}", c.name)),
                        Span::raw(c.detail.clone()),
                    ]));
                }
            } else {
                l.push(Line::from("running checks..."));
            }
            l.push(Line::raw(""));
            l.push(Line::from(
                "Press 'r' to re-run · Enter to continue · Esc to cancel",
            ));
            l
        }
        Step::Service => vec![
            Line::from("Install the runner as a system service so it starts on login?"),
            Line::raw(""),
            Line::from(vec![
                Span::raw(if state.install_service {
                    "[x] "
                } else {
                    "[ ] "
                }),
                Span::raw("Start on login ("),
                Span::raw(if cfg!(target_os = "macos") {
                    "launchd"
                } else {
                    "systemd user unit"
                }),
                Span::raw(")"),
            ]),
            Line::raw(""),
            Line::from("Space toggles · Enter to finish · Esc to cancel"),
        ],
        Step::Done => {
            let msg = state
                .done_message
                .clone()
                .unwrap_or_else(|| "Setup complete.".to_string());
            msg.lines().map(|s| Line::from(s.to_string())).collect()
        }
    };
    Paragraph::new(lines).wrap(Wrap { trim: false })
}

fn footer(state: &Wizard) -> Paragraph<'_> {
    let mut lines = Vec::new();
    if let Some(s) = &state.status_line {
        lines.push(Line::from(Span::styled(
            s.clone(),
            Style::default().fg(Color::Yellow),
        )));
    }
    if let Some(e) = &state.error {
        lines.push(Line::from(Span::styled(
            e.clone(),
            Style::default().fg(Color::Red),
        )));
    }
    if state.busy {
        lines.push(Line::from(Span::styled(
            "working...",
            Style::default().fg(Color::Yellow),
        )));
    }
    Paragraph::new(lines)
}

fn mask(s: &str) -> String {
    if s.len() <= 4 {
        "*".repeat(s.len())
    } else {
        format!("{}...{}", &s[..2], &s[s.len() - 2..])
    }
}

fn default_hostname() -> Option<String> {
    if let Ok(h) = std::env::var("HOSTNAME") {
        if !h.is_empty() {
            return Some(h);
        }
    }
    nix::unistd::gethostname()
        .ok()
        .and_then(|os| os.into_string().ok())
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let v = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(v[1])[1]
}
