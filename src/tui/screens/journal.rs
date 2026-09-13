use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Span;
use ratatui::widgets::{Paragraph, Row, Table, TableState, Wrap};
use ratatui::Frame;

use super::super::state::State;

pub fn render(f: &mut Frame, state: &mut State, area: Rect) {
    let name = state.current().map(|v| v.server.name.clone()).unwrap_or_default();
    let inner = super::popup(f, area, 90, 24, &format!("history — {name}"));
    let [table_area, foot] = Layout::vertical([Constraint::Min(1), Constraint::Length(4)]).areas(inner);
    let header = Row::new(["when", "tx", "action", "outcome", "changes"]).style(Style::default().add_modifier(Modifier::BOLD));
    let rows: Vec<Row> = state
        .flow
        .journal
        .iter()
        .map(|e| {
            let style = match e.action.as_str() {
                "aborted" => Style::default().fg(Color::Red),
                "revert" => Style::default().fg(Color::Yellow),
                "backup" | "restart" => super::dim(),
                _ => Style::default(),
            };
            let changes = e
                .items
                .iter()
                .map(|i| match &i.from {
                    Some(f) => format!("{} {f}→{}", i.name, i.to),
                    None => format!("+{} {}", i.name, i.to),
                })
                .collect::<Vec<_>>()
                .join(", ");
            Row::new(vec![
                Span::raw(e.time.with_timezone(&chrono::Local).format("%Y-%m-%d %H:%M").to_string()),
                Span::styled(e.tx_id.clone(), super::dim()),
                Span::styled(e.action.clone(), style),
                Span::raw(e.outcome.chars().take(30).collect::<String>()),
                Span::raw(changes),
            ])
        })
        .collect();
    let widths = [
        Constraint::Length(17),
        Constraint::Length(16),
        Constraint::Length(9),
        Constraint::Length(24),
        Constraint::Min(20),
    ];
    let table = Table::new(rows, widths)
        .header(header)
        .row_highlight_style(Style::default().add_modifier(Modifier::REVERSED));
    let mut ts = TableState::default().with_selected(Some(state.flow.journal_selected));
    f.render_stateful_widget(table, table_area, &mut ts);
    let detail = state
        .flow
        .journal
        .get(state.flow.journal_selected)
        .map(|e| format!("{}\n{}", e.outcome, e.note.clone().unwrap_or_default()))
        .unwrap_or_default();
    f.render_widget(
        Paragraph::new(format!("{detail}\nr revert selected transaction   Esc back"))
            .wrap(Wrap { trim: true })
            .style(super::dim()),
        foot,
    );
}
