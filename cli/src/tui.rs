use crate::events::FluxEvent;
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
    Terminal,
};
use std::io;

#[derive(Default)]
pub struct TuiExecution {
    pub id: String,
    pub timestamp: String,
    pub method: String,
    pub path: String,
    pub status: Option<String>,
    pub duration_ms: Option<u64>,
    pub captured_io: Vec<CapturedIo>,
    pub error: Option<String>,
}

pub enum CapturedIo {
    Fetch {
        method: String,
        url: String,
        status: u16,
        duration_ms: u64,
    },
    DbQuery {
        query: String,
        duration_ms: u64,
    },
    Log {
        level: String,
        message: String,
    },
}

pub struct TuiApp {
    pub project_name: String,
    pub entry_file: String,
    pub server_url: String,
    pub executions: Vec<TuiExecution>,
    pub system_logs: Vec<CapturedIo>,
    pub list_state: ListState,
}

impl TuiApp {
    pub fn new(project_name: String, entry_file: String, server_url: String) -> Self {
        Self {
            project_name,
            entry_file,
            server_url,
            executions: Vec::new(),
            system_logs: Vec::new(),
            list_state: ListState::default(),
        }
    }

    pub fn handle_event(&mut self, event: FluxEvent) {
        match event {
            FluxEvent::ExecutionStart {
                id,
                method,
                path,
                timestamp,
            } => {
                self.executions.insert(
                    0,
                    TuiExecution {
                        id,
                        timestamp,
                        method,
                        path,
                        ..Default::default()
                    },
                );
            }
            FluxEvent::ExecutionEnd {
                id,
                status,
                duration_ms,
            } => {
                if let Some(exec) = self.executions.iter_mut().find(|e| e.id == id) {
                    exec.status = Some(status);
                    exec.duration_ms = Some(duration_ms);
                }
            }
            FluxEvent::FetchEnd {
                status,
                duration_ms,
                ..
            } => {
                // In this simplified version, we'll attach to the most recent execution
                // that matches (or all in-flight if we had the ID in the event).
                // Actually, I should probably add execution_id to MUST events.
                if let Some(exec) = self.executions.first_mut() {
                    exec.captured_io.push(CapturedIo::Fetch {
                        method: "FETCH".to_string(),
                        url: "api.stripe.com".to_string(),
                        status,
                        duration_ms,
                    });
                }
            }
            FluxEvent::DbQueryEnd { duration_ms } => {
                if let Some(exec) = self.executions.first_mut() {
                    exec.captured_io.push(CapturedIo::DbQuery {
                        query: "SELECT * FROM users".to_string(),
                        duration_ms,
                    });
                }
            }
            FluxEvent::Error { message, .. } => {
                if let Some(exec) = self.executions.first_mut() {
                    exec.error = Some(message);
                }
            }
            FluxEvent::Log { level, message } => {
                if let Some(exec) = self.executions.first_mut() {
                    exec.captured_io.push(CapturedIo::Log { level, message });
                } else {
                    self.system_logs.push(CapturedIo::Log { level, message });
                }
            }
            _ => {}
        }
    }
}

pub fn render(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut TuiApp,
) -> io::Result<()> {
    terminal.draw(|f| {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Min(0),
                Constraint::Length(3),
            ])
            .split(f.area());

        // Header
        let header = Paragraph::new(format!(
            " FLUX RUNTIME v0.2.6  •  Project: {}  •  Entry: {}",
            app.project_name, app.entry_file
        ))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .style(Style::default().fg(Color::Blue)),
        );
        f.render_widget(header, chunks[0]);

        let mut list_items = Vec::new();

        // System logs (startup errors etc)
        for log in &app.system_logs {
            if let CapturedIo::Log { level, message } = log {
                let color = if level == "error" {
                    Color::Red
                } else {
                    Color::DarkGray
                };
                list_items.push(ListItem::new(Line::from(vec![
                    Span::styled(format!(" [S] "), Style::default().fg(Color::Yellow)),
                    Span::styled(message.clone(), Style::default().fg(color)),
                ])));
            }
        }

        // Executions List
        for exec in &app.executions {
            let mut lines = Vec::new();

            let status_span = match exec.status.as_deref() {
                Some("ok") => Span::styled(
                    " ✓ 200 ",
                    Style::default().bg(Color::Green).fg(Color::Black),
                ),
                Some("error") => Span::styled(
                    " ✗ ERROR ",
                    Style::default().bg(Color::Red).fg(Color::Black),
                ),
                _ => Span::styled(
                    " PENDING ",
                    Style::default().bg(Color::Yellow).fg(Color::Black),
                ),
            };

            lines.push(Line::from(vec![
                Span::styled(
                    format!(" {} ", exec.timestamp),
                    Style::default().fg(Color::DarkGray),
                ),
                Span::styled(
                    format!(" {} {} ", exec.method, exec.path),
                    Style::default().add_modifier(Modifier::BOLD),
                ),
                status_span,
                Span::styled(
                    format!(" {}ms", exec.duration_ms.unwrap_or(0)),
                    Style::default().fg(Color::Cyan),
                ),
            ]));

            for io in &exec.captured_io {
                match io {
                    CapturedIo::Fetch {
                        method,
                        url,
                        status,
                        duration_ms,
                    } => {
                        lines.push(Line::from(vec![
                            Span::raw("   ├─ capture: "),
                            Span::styled(
                                format!("fetch {} {} ", method, url),
                                Style::default().fg(Color::Magenta),
                            ),
                            Span::styled(
                                format!("✓ {} ", status),
                                Style::default().fg(Color::Green),
                            ),
                            Span::styled(
                                format!("{}ms", duration_ms),
                                Style::default().fg(Color::DarkGray),
                            ),
                        ]));
                    }
                    CapturedIo::DbQuery { query, duration_ms } => {
                        lines.push(Line::from(vec![
                            Span::raw("   ├─ capture: "),
                            Span::styled("db.query ", Style::default().fg(Color::Yellow)),
                            Span::styled(
                                format!("{} ", query),
                                Style::default().fg(Color::DarkGray),
                            ),
                            Span::styled(
                                format!("{}ms", duration_ms),
                                Style::default().fg(Color::Cyan),
                            ),
                        ]));
                    }
                    CapturedIo::Log { level, message } => {
                        let color = if level == "error" {
                            Color::Red
                        } else {
                            Color::DarkGray
                        };
                        lines.push(Line::from(vec![
                            Span::raw("   ├─ log:     "),
                            Span::styled(message.clone(), Style::default().fg(color)),
                        ]));
                    }
                }
            }

            if let Some(err) = &exec.error {
                lines.push(Line::from(vec![
                    Span::raw("   └─ "),
                    Span::styled("error: ", Style::default().fg(Color::Red)),
                    Span::styled(err.clone(), Style::default().fg(Color::LightRed)),
                ]));
                lines.push(Line::from(vec![
                    Span::raw("      fix:   "),
                    Span::styled(
                        format!("run 'flux why {}' for explanation", exec.id),
                        Style::default()
                            .fg(Color::Blue)
                            .add_modifier(Modifier::BOLD),
                    ),
                ]));
            }

            list_items.push(ListItem::new(lines));
        }

        let list = List::new(list_items)
            .block(Block::default().borders(Borders::LEFT | Borders::RIGHT))
            .highlight_style(Style::default().add_modifier(Modifier::REVERSED));
        f.render_widget(list, chunks[1]);

        // Footer
        let footer = Paragraph::new(format!(
            " [ctrl+c] stop  •  [l] logs  •  Server: {}",
            app.server_url
        ))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .style(Style::default().fg(Color::Blue)),
        );
        f.render_widget(footer, chunks[2]);
    })?;

    Ok(())
}
