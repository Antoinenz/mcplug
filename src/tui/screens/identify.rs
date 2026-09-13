use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{List, ListItem, ListState, Paragraph};
use ratatui::Frame;

use super::super::state::State;
use crate::sources::Confidence;

pub fn render(f: &mut Frame, state: &mut State, area: Rect) {
    let Some(v) = state.servers.get(state.selected) else { return };
    let Some(lock) = &v.lock else { return };
    let Some(p) = lock.plugins.get(state.plugin_selected) else { return };
    let Some(u) = v.undecided.iter().find(|u| u.jar.descriptor.as_ref().is_some_and(|d| d.name == p.name)) else { return };
    let inner = super::popup(f, area, 80, (u.candidates.len() as u16 + 4).max(6), &format!("identify {} ({})", p.name, u.jar.file));
    let [top, list_area] = ratatui::layout::Layout::vertical([ratatui::layout::Constraint::Length(1), ratatui::layout::Constraint::Min(1)]).areas(inner);
    f.render_widget(Paragraph::new(Span::styled("which project is this jar from?", super::dim())), top);
    let items: Vec<ListItem> = u
        .candidates
        .iter()
        .map(|c| {
            let (label, style) = match c.confidence {
                Confidence::HashConfirmed => ("hash ✓", Style::default().fg(Color::Green)),
                Confidence::ExactName => ("exact ", Style::default().fg(Color::Yellow)),
                Confidence::NameMatch => ("name  ", Style::default()),
                Confidence::Weak => ("weak  ", super::dim()),
            };
            ListItem::new(Line::from(vec![
                Span::styled(format!("{label} "), style),
                Span::styled(format!("{:<9}", c.project.source), super::dim()),
                Span::styled(format!("{:<28}", c.project.name), Style::default().add_modifier(Modifier::BOLD)),
                Span::raw(c.project.description.chars().take(inner.width.saturating_sub(50) as usize).collect::<String>()),
            ]))
        })
        .collect();
    let mut ls = ListState::default().with_selected(Some(state.candidate_selected.min(u.candidates.len().saturating_sub(1))));
    f.render_stateful_widget(List::new(items).highlight_style(Style::default().add_modifier(Modifier::REVERSED)), list_area, &mut ls);
}
