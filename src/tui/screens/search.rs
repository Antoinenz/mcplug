use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph};
use ratatui::Frame;

use super::super::state::State;

pub fn render(f: &mut Frame, state: &mut State, area: Rect) {
    let inner = super::popup(f, area, 90, 22, "install a plugin");
    let [input, hint, list_area] = Layout::vertical([Constraint::Length(3), Constraint::Length(1), Constraint::Min(1)]).areas(inner);
    f.render_widget(Paragraph::new(format!("{}▏", state.flow.search_query)).block(Block::default().borders(Borders::ALL).title(" search Modrinth + Hangar, or paste a URL / owner/repo ")), input);
    let status = if state.flow.searching { "searching…" } else if state.flow.collections_mode { "↑↓ choose a collection   Enter open   Esc back" } else if state.flow.search_results.is_empty() { "Enter to search  ·  Ctrl-L your Modrinth collections  ·  Esc to close" } else { "↑↓ choose   Enter pick a version   Tab search again   Ctrl-L collections" };
    f.render_widget(Paragraph::new(Span::styled(format!(" {status}"), super::dim())), hint);
    let width = list_area.width as usize;
    if state.flow.collections_mode {
        let items: Vec<ListItem> = state
            .flow
            .collections
            .iter()
            .map(|c| ListItem::new(Line::from(vec![Span::styled(format!("{:<30}", c.name), Style::default().add_modifier(Modifier::BOLD)), Span::styled(format!("{:>4} projects  ", c.project_ids.len()), Style::default().fg(Color::Cyan)), Span::raw(c.description.clone().unwrap_or_default().chars().take(width.saturating_sub(48)).collect::<String>())])))
            .collect();
        let mut ls = ListState::default().with_selected(Some(state.flow.search_selected));
        f.render_stateful_widget(List::new(items).highlight_style(Style::default().add_modifier(Modifier::REVERSED)), list_area, &mut ls);
        return;
    }
    let items: Vec<ListItem> = state
        .flow
        .search_results
        .iter()
        .map(|c| {
            let dl = c.project.downloads.map(|d| format!("{}", humansize::format_size(d, humansize::DECIMAL).trim_end_matches('B').trim())).unwrap_or_default();
            ListItem::new(Line::from(vec![
                Span::styled(format!("{:<9}", c.project.source), super::dim()),
                Span::styled(format!("{:<28}", c.project.name.chars().take(27).collect::<String>()), Style::default().add_modifier(Modifier::BOLD)),
                Span::styled(format!("{:>7} ", dl), Style::default().fg(Color::Cyan)),
                Span::raw(c.project.description.chars().take(width.saturating_sub(48)).collect::<String>()),
            ]))
        })
        .collect();
    let mut ls = ListState::default().with_selected(Some(state.flow.search_selected));
    f.render_stateful_widget(List::new(items).highlight_style(Style::default().add_modifier(Modifier::REVERSED)), list_area, &mut ls);
}
