use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Row, Table, TableState};
use ratatui::Frame;

use super::super::state::State;
use crate::transaction::plan::installed_label;

pub fn render(f: &mut Frame, state: &mut State, area: Rect) {
    let Some(plan) = &state.flow.plan else { return };
    let inner = super::popup(f, area, 90, (plan.items.len() as u16 + 7).max(10), "review changes");
    let [table_area, foot] = Layout::vertical([Constraint::Min(1), Constraint::Length(3)]).areas(inner);
    let header = Row::new(["plugin", "from", "to", "source", "file", ""]).style(Style::default().add_modifier(Modifier::BOLD));
    let rows: Vec<Row> = plan
        .items
        .iter()
        .map(|i| {
            let mut flags = Vec::new();
            if let Some(d) = &i.dependency_of {
                flags.push(format!("required by {d}"));
            }
            if i.unverified {
                flags.push("no checksum".into());
            }
            if !matches!(i.to.channel, crate::lockfile::Channel::Release) {
                flags.push(format!("{:?}", i.to.channel).to_lowercase());
            }
            Row::new(vec![
                Span::raw(i.name.clone()),
                Span::raw(i.from.as_ref().map(installed_label).unwrap_or_else(|| "(new)".into())),
                Span::styled(i.to.version_number.clone(), Style::default().fg(Color::Yellow)),
                Span::raw(i.project.source.to_string()),
                Span::styled(i.file.name.clone(), super::dim()),
                Span::styled(flags.join(", "), Style::default().fg(Color::Magenta)),
            ])
        })
        .collect();
    let widths = [Constraint::Min(16), Constraint::Length(18), Constraint::Length(18), Constraint::Length(9), Constraint::Min(20), Constraint::Length(22)];
    let table = Table::new(rows, widths).header(header).row_highlight_style(Style::default().add_modifier(Modifier::REVERSED));
    let mut ts = TableState::default().with_selected(Some(state.flow.plan_selected));
    f.render_stateful_widget(table, table_area, &mut ts);
    let mut lines = vec![Line::from(Span::styled("Enter continue   v pick another version   - drop row   Esc cancel", super::dim()))];
    for n in &plan.notes {
        lines.push(Line::from(Span::styled(format!("· {n}"), super::dim())));
    }
    f.render_widget(Paragraph::new(lines).block(Block::default().borders(Borders::TOP)), foot);
}
