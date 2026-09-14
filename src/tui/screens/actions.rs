use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{List, ListItem, ListState};
use ratatui::Frame;

use super::super::state::State;

pub fn render(f: &mut Frame, state: &mut State, area: Rect) {
    let Some(v) = state.servers.get(state.selected) else { return };
    let Some(p) = v.lock.as_ref().and_then(|l| l.plugins.get(state.plugin_selected)) else {
        return;
    };
    let latest = v
        .check
        .as_ref()
        .and_then(|c| c.updates.iter().find(|u| u.name == p.name))
        .map(|u| u.latest.version_number.clone());
    let inner = super::popup(f, area, 50, state.flow.actions.len() as u16 + 2, &p.name);
    let items: Vec<ListItem> = state
        .flow
        .actions
        .iter()
        .map(|a| ListItem::new(Line::from(format!(" {}", a.label(latest.as_deref())))))
        .collect();
    let mut ls = ListState::default().with_selected(Some(state.flow.action_selected));
    f.render_stateful_widget(
        List::new(items).highlight_style(Style::default().add_modifier(Modifier::REVERSED)),
        inner,
        &mut ls,
    );
}
