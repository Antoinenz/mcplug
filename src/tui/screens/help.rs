use ratatui::layout::Rect;
use ratatui::widgets::Paragraph;
use ratatui::Frame;

const HELP: &str = "\
 servers      ↑↓ / j k  move      Enter  open server
              s  scan plugins     c  check for updates    C  check every server
              r  refresh status   q  quit

 plugins      s  scan (identify new jars)      c  check for updates
              u  update…                        i  identify (choose the project for a ? jar)
              m  toggle unmanaged (leave it alone)
              p  toggle pin (never update)      x  ignore the offered version
              Esc  back

 glyphs       ↑ update available   ⇡ update outside declared compatibility   ✓ up to date
              P pinned   ? unidentified   – unmanaged";

pub fn render(f: &mut Frame, area: Rect) {
    let inner = super::popup(f, area, 80, 16, "help");
    f.render_widget(Paragraph::new(HELP), inner);
}
