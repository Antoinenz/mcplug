mod applying;
mod detail;
mod help;
mod identify;
mod journal;
mod restart;
mod review;
mod search;
mod servers;
mod versions;

use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;

use super::state::{Screen, State};

pub fn render(f: &mut Frame, state: &mut State) {
    let [body, bar] = Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(f.area());
    match state.screen {
        Screen::Servers => servers::render(f, state, body),
        Screen::Detail => detail::render(f, state, body),
        Screen::Identify => {
            detail::render(f, state, body);
            identify::render(f, state, body);
        }
        Screen::Help => {
            servers::render(f, state, body);
            help::render(f, body);
        }
        Screen::Review => {
            detail::render(f, state, body);
            review::render(f, state, body);
        }
        Screen::Restart => {
            detail::render(f, state, body);
            restart::render(f, state, body);
        }
        Screen::Applying => {
            detail::render(f, state, body);
            applying::render(f, state, body);
        }
        Screen::Versions => {
            detail::render(f, state, body);
            versions::render(f, state, body);
        }
        Screen::Search => {
            detail::render(f, state, body);
            search::render(f, state, body);
        }
        Screen::Journal => {
            detail::render(f, state, body);
            journal::render(f, state, body);
        }
    }
    status_bar(f, state, bar);
}

fn status_bar(f: &mut Frame, state: &State, area: Rect) {
    let hints = match state.screen {
        Screen::Servers => "↑↓ move  Enter open  s scan  c check  C check all  r refresh  ? help  q quit",
        Screen::Detail => "↑↓ move  s scan  c check  u update  i identify  m unmanaged  p pin  x ignore  Esc back",
        Screen::Identify => "↑↓ choose  Enter accept  m mark unmanaged  Esc cancel",
        Screen::Help => "any key to close",
        Screen::Review => "Enter continue  v version  - drop  Esc cancel",
        Screen::Restart => "↑↓ choose  ←→ countdown  b backup  Enter apply  Esc back",
        Screen::Applying => "working…",
        Screen::Versions => "↑↓ choose  Enter select  Esc back",
        Screen::Search => "type to search  Enter search/select  Esc close",
        Screen::Journal => "↑↓ move  r revert  Esc back",
    };
    let daemon = crate::daemon::DaemonState::load(&crate::daemon::state::path()).describe();
    let line = match &state.toast {
        Some((t, _)) => Line::from(vec![Span::styled(format!(" {t} "), Style::default().fg(Color::Black).bg(Color::Yellow))]),
        None => Line::from(vec![Span::styled(format!(" {hints}"), Style::default().fg(Color::DarkGray)), Span::styled(format!("   {daemon}"), Style::default().fg(Color::DarkGray))]),
    };
    f.render_widget(Paragraph::new(line), area);
}

pub fn popup(f: &mut Frame, area: Rect, width_pct: u16, height: u16, title: &str) -> Rect {
    let w = area.width * width_pct / 100;
    let h = height.min(area.height.saturating_sub(2));
    let r = Rect { x: area.x + (area.width - w) / 2, y: area.y + (area.height.saturating_sub(h)) / 2, width: w, height: h };
    f.render_widget(Clear, r);
    let block = Block::default().borders(Borders::ALL).title(Span::styled(format!(" {title} "), Style::default().add_modifier(Modifier::BOLD)));
    let inner = block.inner(r);
    f.render_widget(block, r);
    inner
}

pub fn dim() -> Style {
    Style::default().fg(Color::DarkGray)
}
