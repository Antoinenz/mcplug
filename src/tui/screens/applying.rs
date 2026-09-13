use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use super::super::state::State;

pub fn render(f: &mut Frame, state: &mut State, area: Rect) {
    let inner = super::popup(f, area, 80, 18, if state.flow.applying { "applying…" } else { "applied" });
    let h = inner.height as usize;
    let start = state.flow.apply_log.len().saturating_sub(h.saturating_sub(2));
    let mut lines: Vec<Line> = state.flow.apply_log[start..]
        .iter()
        .map(|l| {
            let style = if l.starts_with("FAILED") { Style::default().fg(Color::Red) } else if l.starts_with("done") { Style::default().fg(Color::Green) } else { Style::default() };
            Line::from(Span::styled(format!(" {l}"), style))
        })
        .collect();
    if !state.flow.applying {
        lines.push(Line::from(""));
        let hint = if state.flow.last_tx.is_some() { " Enter/Esc close   r revert this transaction" } else { " Enter/Esc close" };
        lines.push(Line::from(Span::styled(hint, Style::default().add_modifier(Modifier::BOLD))));
    }
    f.render_widget(Paragraph::new(lines), inner);
}
