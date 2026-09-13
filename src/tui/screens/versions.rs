use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Span;
use ratatui::widgets::{Paragraph, Row, Table, TableState, Wrap};
use ratatui::Frame;

use super::super::state::{State, VersionTarget};
use crate::sources::Compat;

pub fn render(f: &mut Frame, state: &mut State, area: Rect) {
    let title = match &state.flow.version_target {
        Some(VersionTarget::Plugin(n)) => format!("versions of {n}"),
        Some(VersionTarget::Install(_, n)) => format!("install {n} — pick a version"),
        Some(VersionTarget::PlanItem(i)) => state
            .flow
            .plan
            .as_ref()
            .and_then(|p| p.items.get(*i))
            .map(|it| format!("versions of {}", it.name))
            .unwrap_or_default(),
        None => "versions".into(),
    };
    let inner = super::popup(f, area, 90, 24, &title);
    if state.flow.versions_loading {
        f.render_widget(Paragraph::new(" loading…"), inner);
        return;
    }
    let cctx = state.current().and_then(|v| crate::ops::server_platform(&v.server).ok()).map(|(_, c)| c);
    let [table_area, foot] = Layout::vertical([Constraint::Min(1), Constraint::Length(6)]).areas(inner);
    let header = Row::new(["version", "channel", "published", "compat", "file"]).style(Style::default().add_modifier(Modifier::BOLD));
    let rows: Vec<Row> = state
        .flow
        .versions
        .iter()
        .map(|v| {
            let compat = cctx.as_ref().map(|c| v.compat(c));
            let (ct, cs) = match compat {
                Some(Compat::Exact) => ("✓ exact", Style::default().fg(Color::Green)),
                Some(Compat::Lenient) | Some(Compat::SameLineOnly) => ("~ same line", Style::default().fg(Color::Yellow)),
                Some(Compat::Unknown) | None => ("? unknown", super::dim()),
                Some(Compat::Incompatible) => ("✗", Style::default().fg(Color::Red)),
            };
            Row::new(vec![
                Span::raw(v.version_number.clone()),
                Span::raw(format!("{:?}", v.channel).to_lowercase()),
                Span::raw(v.published.format("%Y-%m-%d").to_string()),
                Span::styled(ct, cs),
                Span::styled(v.primary_file().map(|f| f.name.clone()).unwrap_or_default(), super::dim()),
            ])
        })
        .collect();
    let widths = [
        Constraint::Length(26),
        Constraint::Length(9),
        Constraint::Length(12),
        Constraint::Length(13),
        Constraint::Min(20),
    ];
    let table = Table::new(rows, widths)
        .header(header)
        .row_highlight_style(Style::default().add_modifier(Modifier::REVERSED));
    let mut ts = TableState::default().with_selected(Some(state.flow.version_selected));
    f.render_stateful_widget(table, table_area, &mut ts);
    let changelog = state
        .flow
        .versions
        .get(state.flow.version_selected)
        .and_then(|v| v.changelog.clone())
        .unwrap_or_default();
    let mc = changelog.lines().take(4).collect::<Vec<_>>().join("\n");
    f.render_widget(
        Paragraph::new(format!("{mc}\nEnter choose   Esc back"))
            .wrap(Wrap { trim: true })
            .style(super::dim()),
        foot,
    );
}
