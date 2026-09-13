use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use super::super::state::State;

pub fn render(f: &mut Frame, state: &mut State, area: Rect) {
    let inner = super::popup(f, area, 70, 12, "server jar");
    if state.flow.jar_loading {
        f.render_widget(Paragraph::new(" checking builds…"), inner);
        return;
    }
    let Some(st) = &state.flow.jar else {
        f.render_widget(Paragraph::new(" unavailable"), inner);
        return;
    };
    let id = state.current().map(|v| v.server.id.clone()).unwrap_or_default();
    let installed = match &st.installed {
        Some(b) => format!(
            "build {}{}",
            b.build,
            b.time.map(|t| format!(" ({})", t.format("%Y-%m-%d"))).unwrap_or_default()
        ),
        None => "build not recognised".into(),
    };
    let latest = match &st.latest_same_mc {
        Some(l) if st.installed.as_ref().map(|b| b.build) == Some(l.build) => Span::styled("up to date".to_string(), Style::default().fg(Color::Green)),
        Some(l) => Span::styled(
            format!(
                "build {} available{} — press u",
                l.build,
                l.time.map(|t| format!(" ({})", t.format("%Y-%m-%d"))).unwrap_or_default()
            ),
            Style::default().fg(Color::Yellow),
        ),
        None => Span::styled("unknown".to_string(), super::dim()),
    };
    let newer_mc = match &st.newest_mc {
        Some(n) if n > &st.mc => Line::from(vec![
            Span::raw(format!(" minecraft   {n} available — ")),
            Span::styled(format!("sudo mcplug jar {id} --mc {n} --check-only"), Style::default().fg(Color::Cyan)),
        ]),
        _ => Line::from(Span::styled(" minecraft   newest available", super::dim())),
    };
    let lines = vec![
        Line::from(vec![
            Span::styled(format!(" {} ", st.file), Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(format!("{}", st.mc)),
        ]),
        Line::from(format!(" installed   {installed}")),
        Line::from(vec![Span::raw(" newest      "), latest]),
        newer_mc,
        Line::from(format!(" java        {}", st.java_major.map(|j| j.to_string()).unwrap_or_else(|| "?".into()))),
        Line::from(""),
        Line::from(Span::styled(
            " A build update keeps the old jar next to the new one and rewrites the start command through the panel.",
            super::dim(),
        )),
        Line::from(Span::styled(
            " Minecraft upgrades go through the CLI wizard, which checks every plugin's compatibility and your Java first.",
            super::dim(),
        )),
    ];
    f.render_widget(Paragraph::new(lines), inner);
}
