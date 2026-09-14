use ratatui::layout::Rect;
use ratatui::widgets::Paragraph;
use ratatui::Frame;

const HELP: &str = "\
 mcplug finds your servers, scans their plugins and checks for updates by itself.
 You mostly read the screen and press Enter.

 servers      Enter open   b install the in-game bridge (/mcplug for ops)   r re-check   q quit
 plugins      ↑↓ choose    Enter actions for that plugin (update, pick a version, pin, ignore, identify…)
              u  update everything that has an update
              a  add a plugin (search Modrinth + Hangar, paste a URL, or Ctrl-L for your collections)
              r  re-check this server        Esc back

 glyphs       ↑ update available   ⇡ update outside declared compatibility   ✓ up to date
              P pinned   ? unidentified (Enter to pick the project)   – unmanaged";

pub fn render(f: &mut Frame, area: Rect) {
    let inner = super::popup(f, area, 84, 15, "help");
    f.render_widget(Paragraph::new(HELP), inner);
}
