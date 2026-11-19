//! A Markdown file in the preview, formatted (0.10's `MarkdownView`). The blocks and their
//! styled ranges are `sshee_app::files::markdown`'s; here they're drawn. Nothing in the file is
//! trusted: images aren't fetched (their alt text stands in, in a quiet chip), and only web and
//! mail links open, in the system's browser. Opened at some lines ("notes.md:12-40"), the
//! blocks they make up are marked by one band behind them, keyed for the preview to scroll to.

use rune::text::Link;
use rune::{col, el, rich, row, text, El, ScrollSpec, Span, Tx};
use sshee_app::files::markdown::{self, Block, Bullet, Style, Styled};

use crate::shell::Shell;
use crate::kit::scroll_bar::ScrollBars;
use crate::theme::{self, Theme};

const BODY: rune::tok::TextStyle = rune::tok::TextStyle::new(13.0, 400.0).lh(20.0);

/// A run of styled text as Rune draws it: spans for the styles, links for the links.
fn styled(s: &Shell, block: &Styled, base: rune::tok::TextStyle, t: &'static Theme) -> El {
    let shown = s.text_shown(&block.text).into_owned();
    // Streamer mode changed the text: draw it plain, the ranges no longer fit.
    if shown != block.text {
        return text(shown).font(base).color(t.l1).wrap().w_full();
    }
    let mut spans = Vec::new();
    let mut links = Vec::new();
    for (i, m) in block.marks.iter().enumerate() {
        let span = Span::new(m.start, m.end);
        spans.push(match &m.style {
            Style::Strong => span.bold(),
            Style::Emph => span.italic(),
            Style::Code => span.mono().size(base.size * 0.9).bg(t.hov2, 2.0),
            Style::Strike => span.color(t.l2),
            Style::Image(_) => span.color(t.l2).bg(t.hov, 2.0),
            Style::Link(url) if markdown::is_external(url) => {
                links.push(Link::new(m.start..m.end, url.clone()));
                span.color(t.accent_text)
            }
            Style::Link(_) => span.underline(t.hair),
        });
        let _ = i;
    }
    let mut out = rich(block.text.clone(), spans).font(base).color(t.l1).w_full();
    for link in links {
        out = out.link(link);
    }
    out.on_link(|_: &mut Shell, url: &str, tx: &mut Tx| {
        // Never anything but the web and mail.
        if markdown::is_external(url) {
            let _ = tx.open_url(url);
        }
    })
}

/// The formatted file, top to bottom, in a scroll view. `lines`: the lines a path named (first
/// and last), whose blocks get one band, keyed `band_key`.
pub fn view(s: &Shell, source: &str, lines: Option<(u32, u32)>, band_key: &str, t: &'static Theme) -> El {
    let blocks = markdown::parse_lines(source);
    let marked = lines.map_or(0..0, |(first, last)| markdown::blocks_at(&blocks, first, last));
    let mut doc = col().w_full().px(18.0).py(14.0).gap(8.0);
    // The marked blocks, gathered in the band as they come.
    let mut band: Option<El> = None;
    for (i, (block, _)) in blocks.iter().enumerate() {
        if i == marked.end {
            doc = doc.children(band.take());
        }
        let key = format!("md-{i}");
        let drawn = match block {
            Block::Heading { level, text: words } => {
                let size = match level {
                    1 => 20.0,
                    2 => 16.5,
                    3 => 14.5,
                    4 => 13.5,
                    _ => 13.0,
                };
                let style = rune::tok::TextStyle::new(size, 600.0).lh(size * 1.35);
                let mut h = col().key(key).w_full().mt(if *level <= 2 { 10.0 } else { 6.0 }).child(styled(s, words, style, t));
                if *level <= 2 {
                    h = h.pb(4.0).border_sides([0.0, 0.0, 1.0, 0.0], t.hair2);
                }
                h
            }
            Block::Paragraph(words) => el().key(key).w_full().child(styled(s, words, BODY, t)),
            Block::Item { depth, bullet, text: words } => {
                let mark = match bullet {
                    Bullet::Dot => "•".to_owned(),
                    Bullet::Number(n) => format!("{n}."),
                    Bullet::Task(true) => "☑".to_owned(),
                    Bullet::Task(false) => "☐".to_owned(),
                };
                row().key(key).w_full().pl(4.0 + f32::from(*depth) * 18.0).gap(6.0).child(el().w(18.0).child(text(mark).font(BODY).color(t.l2).single_line())).child(col().grow(1.0).min_w(0.0).child(styled(s, words, BODY, t)))
            }
            Block::Quote(words) => row().key(key).w_full().gap(10.0).child(el().w(3.0).h_full().radius(1.5).fill(t.hair)).child(col().grow(1.0).min_w(0.0).child(styled(s, words, BODY, t))),
            Block::Code { text: code, .. } => col().key(key).w_full().padding(12.0).radius(8.0).fill(t.code).scroll_x().child(text(s.text_shown(code).into_owned()).font(theme::CODE.lh(19.0)).color(t.l1)).scroll_bars(t),
            Block::Rule => el().key(key).w_full().h(1.0).my(8.0).fill(t.hair),
            Block::Table { head, rows } => {
                let cell = |words: &Styled, bold: bool| {
                    el().w(160.0).px(8.0).py(5.0).border(1.0, t.hair).child(styled(s, words, if bold { rune::tok::TextStyle::new(12.5, 600.0).lh(18.0) } else { rune::tok::TextStyle::new(12.5, 400.0).lh(18.0) }, t))
                };
                let mut table = col().key(key).scroll_x();
                table = table.child(row().fill(t.hov).children(head.iter().map(|h| cell(h, true)).collect::<Vec<_>>()));
                for r in rows {
                    table = table.child(row().children(r.iter().map(|c| cell(c, false)).collect::<Vec<_>>()));
                }
                table.scroll_bars(t)
            }
        };
        if marked.contains(&i) {
            // Bleeding past the text a little, so the band holds the blocks with room.
            let open = band.take().unwrap_or_else(|| {
                col().key(band_key).w_full().gap(8.0).px(8.0).py(6.0).mx(-8.0).my(-6.0).radius(6.0).fill(t.blue.alpha(0.10)).border_sides([0.0, 0.0, 0.0, 2.0], t.blue.alpha(0.7))
            });
            band = Some(open.child(drawn));
        } else {
            doc = doc.child(drawn);
        }
    }
    doc = doc.children(band);
    col().key("preview-markdown").w_full().grow(1.0).min_h(0.0).scroll(ScrollSpec { stick_bottom: false }).child(doc).scroll_bars(t)
}
