use ratatui::layout::{Constraint, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Span;
use ratatui::widgets::{Block, Borders, Row, Table, TableState};
use ratatui::Frame;

use super::super::state::State;
use crate::server::Access;

pub fn render(f: &mut Frame, state: &mut State, area: Rect) {
    let header = Row::new(["server", "platform", "mc", "status", "players", "plugins", "updates", "access", ""]).style(Style::default().add_modifier(Modifier::BOLD));
    let rows: Vec<Row> = state
        .servers
        .iter()
        .map(|v| {
            let s = &v.server;
            let mc = s.jar.as_ref().and_then(|j| j.mc_version.as_ref()).map(|m| m.to_string()).unwrap_or_else(|| "?".into());
            let plugins = v.lock.as_ref().map(|l| l.plugins.len().to_string()).unwrap_or_else(|| "–".into());
            let updates = match &v.check {
                Some(c) if !c.updates.is_empty() => Span::styled(c.updates.len().to_string(), Style::default().fg(Color::Yellow)),
                Some(_) => Span::styled("✓", Style::default().fg(Color::Green)),
                None => Span::raw("–"),
            };
            let access = match &s.access {
                Access::ReadWrite => Span::styled("rw", Style::default().fg(Color::Green)),
                Access::ReadOnly { .. } => Span::styled("ro", Style::default().fg(Color::Red)),
                Access::Missing => Span::styled("–", super::dim()),
            };
            let status = match v.status.as_str() {
                "running" => Span::styled("running", Style::default().fg(Color::Green)),
                other => Span::styled(other.to_string(), super::dim()),
            };
            Row::new(vec![
                Span::raw(s.name.clone()),
                Span::raw(s.platform.to_string()),
                Span::raw(mc),
                status,
                Span::raw(v.players.map(|p| p.to_string()).unwrap_or_default()),
                Span::raw(plugins),
                updates,
                access,
                Span::styled(v.busy.unwrap_or("").to_string(), Style::default().fg(Color::Cyan)),
            ])
        })
        .collect();
    let widths = [
        Constraint::Min(22),
        Constraint::Length(10),
        Constraint::Length(8),
        Constraint::Length(9),
        Constraint::Length(8),
        Constraint::Length(8),
        Constraint::Length(8),
        Constraint::Length(7),
        Constraint::Length(10),
    ];
    let table = Table::new(rows, widths)
        .header(header)
        .block(Block::default().borders(Borders::ALL).title(" mcplug — servers "))
        .row_highlight_style(Style::default().add_modifier(Modifier::REVERSED));
    let mut ts = TableState::default().with_selected(Some(state.selected));
    f.render_stateful_widget(table, area, &mut ts);
}
