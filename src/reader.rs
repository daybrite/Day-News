//! The article pane: a native web view showing the feed's own HTML, wrapped in a readable
//! document of our own so text is legible and matches the app's appearance.

use crate::format::full_date;
use crate::theme::palette;
use day::prelude::*;
use day_piece_webview::web_view;
use daynews_core::StoredArticle;

/// Build the article document and hand the web view a `file://` URL for it.
///
/// A `data:` URL would avoid the temp file, but Android's WebView refuses top-level `data:`
/// navigations (API 30+) and every platform caps their length, so a file is the portable choice.
/// The scratch directory is the one the backend reports as app-writable, which is the only
/// writable location on iOS and Android.
pub fn render_to_file(article: &StoredArticle) -> Option<String> {
    let dir = app_temp_dir().join("news-reader");
    std::fs::create_dir_all(&dir).ok()?;
    // One file per article id: revisiting an article reuses its path, and the set stays bounded
    // by how many distinct articles were opened this session.
    let path = dir.join(format!("article-{}.html", article.id));
    std::fs::write(&path, document(article)).ok()?;
    Some(format!("file://{}", path.to_string_lossy()))
}

fn document(a: &StoredArticle) -> String {
    let p = palette();
    let body = a
        .content_html
        .as_deref()
        .or(a.summary.as_deref())
        .unwrap_or(
            "<p><em>This article has no content. Open it in your browser to read it.</em></p>",
        );
    let title = escape(a.title.as_deref().unwrap_or("Untitled"));
    let byline = match &a.author {
        Some(author) => format!(r#" <span class="by">{}</span>"#, escape(author)),
        None => String::new(),
    };
    let when = escape(&full_date(a.published_at));
    let link = a
        .url
        .as_deref()
        .map(|u| {
            format!(
                r#"<a class="src" href="{}">{}</a>"#,
                escape(u),
                escape(&a.feed_title)
            )
        })
        .unwrap_or_else(|| escape(&a.feed_title));

    // Deliberately a self-contained document with no external assets: the reader must render
    // the same offline, and pulling remote CSS would leak the reader's activity to third parties.
    format!(
        r#"<!doctype html>
<html><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<style>
  :root {{ color-scheme: {scheme}; }}
  html, body {{ margin: 0; padding: 0; background: {bg}; color: {fg}; }}
  body {{
    font: 15px/1.65 -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto,
          "Helvetica Neue", system-ui, sans-serif;
    padding: 26px 32px 72px; max-width: 42em; margin: 0 auto;
    overflow-wrap: break-word; word-break: break-word;
  }}
  /* Masthead: source and byline, a rule, then the headline — the order a reader's eye wants,
     and the one NetNewsWire uses. */
  .src {{ color: {accent}; text-decoration: none; font-weight: 600; font-size: 0.87em; }}
  .by {{ color: {muted}; font-size: 0.87em; }}
  .mast {{ margin: 0 0 12px; }}
  .rule {{ border: none; border-top: 1px solid {rule}; margin: 0 0 16px; }}
  h1.t {{ font-size: 1.85em; line-height: 1.2; letter-spacing: -0.012em; margin: 0 0 8px;
          font-weight: 700; }}
  .when {{ color: {muted}; font-size: 0.74em; letter-spacing: 0.06em; text-transform: uppercase;
           margin: 0 0 22px; }}
  a {{ color: {accent}; }}
  /* Feed HTML is arbitrary: keep media inside the pane rather than forcing a sideways scroll. */
  img, video, iframe, table {{ max-width: 100%; height: auto; }}
  figure {{ margin: 1em 0; }}
  pre {{ background: {alt}; padding: 12px; overflow-x: auto; border-radius: 8px; }}
  code {{ font-size: 0.9em; }}
  blockquote {{ margin: 1em 0; padding-left: 1em; border-left: 3px solid {rule}; color: {muted}; }}
  hr {{ border: none; border-top: 1px solid {rule}; }}
</style></head>
<body>
<p class="mast">{link}{byline}</p>
<hr class="rule">
<h1 class="t" id="reader-title">{title}</h1>
<p class="when">{when}</p>
{body}
</body></html>"#,
        scheme = if day::dark_mode() { "dark" } else { "light" },
        bg = css(p.bg),
        fg = css(p.text),
        muted = css(p.text_muted),
        accent = css(p.accent),
        alt = css(p.bg_alt),
        rule = css(p.rule),
    )
}

fn css(c: Color) -> String {
    format!(
        "#{:02x}{:02x}{:02x}",
        (c.r * 255.0).round() as u8,
        (c.g * 255.0).round() as u8,
        (c.b * 255.0).round() as u8
    )
}

/// Escape for HTML text and quoted attributes.
fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}

/// The reader pane. Empty state until an article is open.
pub fn reader_pane() -> impl Piece {
    let st = daynews_core::state();
    let url = Signal::new(String::new());
    // The web view's bound URL is imperative BY DESIGN: it loads on creation and thereafter only
    // when a `go` trigger fires (navigation writes the signal back, so auto-loading on every
    // change would loop). Writing the URL alone left the pane showing the first article forever.
    let go = Trigger::new();
    // Re-render whenever the open article changes (and when the theme flips, so the document's
    // colors follow the system appearance).
    bind(
        move || (st.article.get().map(|a| a.id), day::dark_mode()),
        move |_: &(Option<u64>, bool)| {
            let doc = st.article.get_untracked().and_then(|a| render_to_file(&a));
            url.set(doc.unwrap_or_default());
            go.notify();
        },
    );

    column((
        when(
            move || st.article.get().is_none(),
            || {
                column((
                    spacer(),
                    label(crate::res::str::reader_empty())
                        .font(Font::Title3)
                        .color(move || palette().text_muted)
                        .id("reader-empty"),
                    spacer(),
                ))
                .align(HAlign::Center)
                .grow()
            },
        ),
        when(
            move || st.article.get().is_some(),
            move || web_view(url).go(go).id("reader-web").grow(),
        ),
    ))
    .background(move || palette().bg)
    .grow()
}
