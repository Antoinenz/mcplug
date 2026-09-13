use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use super::super::state::State;

pub fn render(f: &mut Frame, state: &mut State, area: Rect) {
    let inner = super::popup(f, area, 60, 11, "apply");
    let sel = |i: usize| {
        if state.flow.restart_choice == i {
            Style::default().add_modifier(Modifier::REVERSED)
        } else {
            Style::default()
        }
    };
    let control = state
        .current()
        .map(|v| crate::control::for_server(&v.server, state.mcsm.as_deref().cloned(), &state.loaded.secrets).name())
        .unwrap_or("?");
    let lines = vec![
        Line::from(Span::styled(" restart the server", Style::default().add_modifier(Modifier::BOLD))),
        Line::from(Span::styled(format!("   now, after a {}s countdown in chat", state.flow.countdown), sel(0))),
        Line::from(Span::styled("   when nobody is online (waits, then a short countdown)", sel(1))),
        Line::from(Span::styled("   never — stage the jars, restart yourself later", sel(2))),
        Line::from(""),
        Line::from(vec![
            Span::raw(" ["),
            Span::styled(if state.flow.backup { "x" } else { " " }, Style::default().fg(Color::Green)),
            Span::raw("] mcbackup checkpoint before + snapshot after (b)"),
        ]),
        Line::from(Span::styled(format!(" control: {control}    ←/→ countdown ±15s"), super::dim())),
        Line::from(Span::styled(" Enter apply   Esc back", super::dim())),
    ];
    f.render_widget(Paragraph::new(lines), inner);
}
