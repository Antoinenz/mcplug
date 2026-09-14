use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Row, Table, TableState};
use ratatui::Frame;

use super::super::state::State;
use crate::lockfile::SourceRef;
use crate::server::Access;

pub fn render(f: &mut Frame, state: &mut State, area: Rect) {
    let Some(v) = state.servers.get(state.selected) else { return };
    let [head, body, foot] = Layout::vertical([Constraint::Length(4), Constraint::Min(3), Constraint::Length(4)]).areas(area);

    let s = &v.server;
    let mc = s
        .jar
        .as_ref()
        .and_then(|j| j.mc_version.as_ref())
        .map(|m| m.to_string())
        .unwrap_or_else(|| "?".into());
    let build = s.jar.as_ref().and_then(|j| j.build_hint).map(|b| format!(" build {b}")).unwrap_or_default();
    let access = match &s.access {
        Access::ReadWrite => Span::styled("read-write", Style::default().fg(Color::Green)),
        Access::ReadOnly { reason } => Span::styled(format!("read-only: {reason}"), Style::default().fg(Color::Red)),
        Access::Missing => Span::styled("no plugin directory", Style::default().fg(Color::Red)),
    };
    let lines = vec![
        Line::from(vec![
            Span::styled(format!(" {} ", s.name), Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(format!(
                "{} {mc}{build}  ·  {}  ·  {}",
                s.platform,
                v.status,
                v.players.map(|p| format!("{p} online")).unwrap_or_else(|| "–".into())
            )),
        ]),
        Line::from(vec![Span::raw(" "), access, Span::styled(format!("  ·  {}", s.root.display()), super::dim())]),
    ];
    f.render_widget(Paragraph::new(lines).block(Block::default().borders(Borders::ALL)), head);

    let Some(lock) = &v.lock else {
        f.render_widget(
            Paragraph::new("  not scanned yet — press s").block(Block::default().borders(Borders::ALL).title(" plugins ")),
            body,
        );
        return;
    };
    let header = Row::new(["", "plugin", "installed", "latest", "source", "file"]).style(Style::default().add_modifier(Modifier::BOLD));
    let rows: Vec<Row> = lock
        .plugins
        .iter()
        .map(|p| {
            let update = v.check.as_ref().and_then(|c| c.updates.iter().find(|u| u.name == p.name));
            let checked = v.check.is_some();
            let (glyph, style) = match (&p.source, p.pinned, update) {
                (SourceRef::Unidentified, _, _) => ("?", Style::default().fg(Color::Magenta)),
                (SourceRef::Unmanaged, _, _) => ("–", super::dim()),
                (_, true, _) => ("P", Style::default().fg(Color::Blue)),
                (_, _, Some(u)) if u.untested => ("⇡", Style::default().fg(Color::Yellow)),
                (_, _, Some(_)) => ("↑", Style::default().fg(Color::Yellow)),
                _ if checked => ("✓", Style::default().fg(Color::Green)),
                _ => (" ", Style::default()),
            };
            let installed = match &p.source {
                SourceRef::Modrinth { version_number, .. } => version_number.clone(),
                SourceRef::Hangar { version_name, .. } => version_name.clone(),
                SourceRef::GitHub { tag, .. } => tag.clone(),
                SourceRef::GeyserMc { build, .. } => build.map(|b| format!("build {b}")).unwrap_or_else(|| "?".into()),
                _ => p.descriptor_version.clone().unwrap_or_else(|| "?".into()),
            };
            let latest = match update {
                Some(u) => format!("{}{}", u.latest.version_number, if u.unverified { " (unverified)" } else { "" }),
                None if checked && p.source.is_managed() && !p.pinned => "up to date".into(),
                None => String::new(),
            };
            Row::new(vec![
                Span::styled(glyph.to_string(), style),
                Span::raw(p.name.clone()),
                Span::raw(installed),
                Span::styled(latest, if update.is_some() { Style::default().fg(Color::Yellow) } else { super::dim() }),
                Span::raw(p.source.kind_str().to_string()),
                Span::styled(p.file.clone(), super::dim()),
            ])
        })
        .collect();
    let widths = [
        Constraint::Length(2),
        Constraint::Min(18),
        Constraint::Length(22),
        Constraint::Length(24),
        Constraint::Length(10),
        Constraint::Min(20),
    ];
    let n_updates = v.check.as_ref().map(|c| c.updates.len()).unwrap_or(0);
    let title = match (v.busy, n_updates) {
        (Some(b), _) => format!(" plugins ({}) — {b} ", lock.plugins.len()),
        (None, 0) if v.check.is_some() => format!(" plugins ({}) — all up to date ", lock.plugins.len()),
        (None, n) if n > 0 => format!(
            " plugins ({}) — {n} update{} available, u to apply ",
            lock.plugins.len(),
            if n == 1 { "" } else { "s" }
        ),
        _ => format!(" plugins ({}) ", lock.plugins.len()),
    };
    let table = Table::new(rows, widths)
        .header(header)
        .block(Block::default().borders(Borders::ALL).title(title))
        .row_highlight_style(Style::default().add_modifier(Modifier::REVERSED));
    let mut ts = TableState::default().with_selected(Some(state.plugin_selected));
    f.render_stateful_widget(table, body, &mut ts);

    // footer: details of the selected plugin
    let mut foot_lines = Vec::new();
    if let Some(p) = lock.plugins.get(state.plugin_selected) {
        let src = match &p.source {
            SourceRef::Modrinth { project_id, .. } => format!("modrinth project {project_id}"),
            SourceRef::Hangar { slug, .. } => format!("hangar {slug}"),
            SourceRef::GitHub { owner, repo, asset_glob, .. } => format!("github {owner}/{repo} ({asset_glob})"),
            SourceRef::GeyserMc { project, .. } => format!("geysermc {project}"),
            SourceRef::Unidentified => "not found by hash — Enter to pick the project it comes from".into(),
            SourceRef::Unmanaged => "unmanaged — Enter to manage it again".into(),
        };
        foot_lines.push(Line::from(vec![
            Span::styled(format!(" {} ", p.name), Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(src),
        ]));
        let mut flags = vec![format!("channel {:?}", p.channel).to_lowercase()];
        if p.pinned {
            flags.push("pinned".into());
        }
        if !p.ignored_versions.is_empty() {
            flags.push(format!("{} ignored version(s)", p.ignored_versions.len()));
        }
        if let Some(u) = v.check.as_ref().and_then(|c| c.updates.iter().find(|u| u.name == p.name)) {
            flags.push(format!(
                "update {} published {} ({})",
                u.latest.version_number,
                u.latest.published.format("%Y-%m-%d"),
                u.compat
            ));
        }
        foot_lines.push(Line::from(Span::styled(format!(" {}", flags.join("  ·  ")), super::dim())));
    }
    if let Some(e) = &v.last_error {
        foot_lines.push(Line::from(Span::styled(format!(" ! {e}"), Style::default().fg(Color::Red))));
    }
    f.render_widget(Paragraph::new(foot_lines).block(Block::default().borders(Borders::ALL)), foot);
}
