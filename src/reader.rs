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
fn render_to_file(path: &std::path::Path, article: &StoredArticle) -> Option<String> {
    std::fs::create_dir_all(path.parent()?).ok()?;
    std::fs::write(path, document(article)).ok()?;
    #[cfg(not(target_arch = "wasm32"))]
    {
        url::Url::from_file_path(path).ok().map(Into::into)
    }
    #[cfg(target_arch = "wasm32")]
    {
        // Day's web filesystem uses POSIX paths, while url's OS-path helper is unavailable
        // on wasm. The webview backend resolves this URL through that virtual filesystem.
        let mut url = url::Url::parse("file:///").ok()?;
        url.set_path(&path.to_string_lossy());
        Some(url.into())
    }
}

fn document(a: &StoredArticle) -> String {
    document_with_style(
        a,
        day::dark_mode(),
        &crate::reader_styles::state().get_untracked(),
    )
}

#[cfg(test)]
fn document_in_appearance(a: &StoredArticle, dark: bool) -> String {
    document_with_style(a, dark, &crate::reader_styles::ReaderStyle::default())
}

fn document_with_style(
    a: &StoredArticle,
    dark: bool,
    style: &crate::reader_styles::ReaderStyle,
) -> String {
    let p = crate::theme::palette_for(dark);
    let empty_body = format!(
        "<p><em>{}</em></p>",
        escape(&crate::res::str::reader_no_content().format())
    );
    // Parser summaries are plain text; only content_html is publication markup.
    let summary = a.summary.as_deref().map(escape);
    let body = a
        .content_html
        .as_deref()
        .filter(|body| !body.trim().is_empty())
        .or_else(|| summary.as_deref().filter(|text| !text.trim().is_empty()))
        .unwrap_or(&empty_body);
    let title = a.title.as_deref().filter(|title| !title.trim().is_empty());
    let title = match title {
        Some(title) => escape(title),
        None => escape(&crate::res::str::untitled().format()),
    };
    // Relative images and links in feed HTML resolve against the publication, not our
    // temporary reader file. Only web URLs are suitable document bases.
    let base = a
        .url
        .as_deref()
        .filter(|url| url.starts_with("https://") || url.starts_with("http://"))
        .map(|url| format!(r#"<base href="{}">"#, escape(url)))
        .unwrap_or_default();
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

    // A self-contained document with no external assets: the reader must render the same
    // offline, and pulling remote CSS would leak the reader's activity to third parties.
    format!(
        r#"<!doctype html>
<html><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
{base}
<style>
  :root {{ color-scheme: {scheme}; }}
  html {{ -webkit-text-size-adjust: 100%; font-size: 17px; }}
  html, body {{ margin: 0; padding: 0; background: {bg}; color: {fg}; }}
  body {{
    font: 17px/1.65 -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto,
          "Helvetica Neue", system-ui, sans-serif;
    padding: 28px 32px 72px; max-width: 40em; margin: 0 auto;
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
  #reader-link-tooltip {{ position: fixed; z-index: 2147483647; pointer-events: none;
    max-width: min(680px, calc(100vw - 16px)); padding: 7px 10px;
    background: {alt}; color: {fg}; border: 1px solid {rule}; border-radius: 6px;
    box-shadow: 0 3px 12px #0003; font: 12px/1.4 system-ui, sans-serif;
    overflow-wrap: anywhere; }}
  /* Feed HTML is arbitrary: keep media inside the pane rather than forcing a sideways scroll. */
  img, video, iframe, table {{ max-width: 100%; }}
  img, video {{ height: auto; }}
  table {{ display: block; overflow-x: auto; }}
  iframe {{ border: 0; }}
  figure {{ margin: 1em 0; }}
  pre {{ background: {alt}; padding: 12px; overflow-x: auto; border-radius: 8px; }}
  code {{ font-size: 0.9em; }}
  blockquote {{ margin: 1em 0; padding-left: 1em; border-left: 3px solid {rule}; color: {muted}; }}
  hr {{ border: none; border-top: 1px solid {rule}; }}
  @supports (font: -apple-system-body) {{
    html {{ font: -apple-system-body; }}
  }}
  @media (max-width: 480px) {{
    body {{ padding: 20px 20px 48px; }}
    h1.t {{ font-size: 1.65em; }}
  }}
</style>
<style id="reader-display">{display_style}</style></head>
<body>
<p class="mast">{link}{byline}</p>
<hr class="rule">
<h1 class="t" id="reader-title">{title}</h1>
<p class="when">{when}</p>
{body}
<div id="reader-link-tooltip" role="tooltip" hidden></div>
<script>
(() => {{
  const tooltip = document.getElementById('reader-link-tooltip');
  const hide = () => {{ tooltip.hidden = true; tooltip.textContent = ''; }};
  document.addEventListener('pointermove', event => {{
    if (event.pointerType && event.pointerType !== 'mouse' && event.pointerType !== 'pen') {{ hide(); return; }}
    const link = event.target.closest && event.target.closest('a[href]');
    if (!link) {{ hide(); return; }}
    tooltip.textContent = link.href;
    tooltip.hidden = false;
    const box = tooltip.getBoundingClientRect();
    const x = Math.max(8, Math.min(event.clientX + 12, innerWidth - box.width - 8));
    let y = event.clientY + 18;
    if (y + box.height > innerHeight - 8) y = Math.max(8, event.clientY - box.height - 10);
    tooltip.style.left = x + 'px'; tooltip.style.top = y + 'px';
  }});
  document.addEventListener('pointerleave', hide);
  document.addEventListener('scroll', hide, true);
  document.addEventListener('click', hide, true);
  document.addEventListener('keydown', hide);
  window.addEventListener('blur', hide);
}})();
</script>
</body></html>"#,
        display_style = crate::reader_styles::stylesheet(style, dark),
        scheme = if dark { "dark" } else { "light" },
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

/// The open article, rendered the way this backend can.
///
/// Where there is a web engine the generated document goes to the web view. A backend
/// without one gets a composed text reader instead of a placeholder leaf.
fn reader_body(url: Signal<String>, go: Trigger, view: crate::reader_view::ReaderView) -> AnyPiece {
    if day_piece_webview::support() == Support::Unsupported {
        article_text().any()
    } else {
        let js = view.engine;
        watch(
            || (crate::reader_styles::state().get(), day::dark_mode()),
            move |_, _| crate::reader_styles::apply_to(js),
        );
        web_view(url)
            .js(js)
            .on_load(move || {
                crate::reader_styles::apply_to(js);
                view.ready.set(true);
            })
            .go(go)
            .on_external_link(|url| {
                crate::settings::open_link(url);
                day_piece_webview::LinkPolicy::Ignore
            })
            .id("reader-web")
            .grow()
            .any()
    }
}

/// The article as pieces: the masthead order the document uses (source, headline, date) and
/// then its text, with the feed's markup reduced to headings and paragraphs.
fn article_text() -> impl Piece {
    let st = daynews_core::scene();
    let view = crate::reader_view::ReaderView::ambient();
    // One label per block, keyed by article and position, so a new article rebuilds the column
    // and a heading keeps its weight instead of reading as one more paragraph.
    let body = move || {
        let Some(a) = view.article(st.article.get()) else {
            return Vec::new();
        };
        let html = a
            .content_html
            .as_deref()
            .or(a.summary.as_deref())
            .unwrap_or_default();
        let mut blocks = crate::format::blocks(html);
        if blocks.is_empty() {
            blocks.push(crate::format::Block {
                text: crate::res::str::reader_no_content().format(),
                heading: false,
            });
        }
        blocks
            .into_iter()
            .enumerate()
            .map(|(i, b)| (format!("{}-{i}", a.id), b))
            .collect::<Vec<_>>()
    };
    let block = |b: crate::format::Block| {
        if b.heading {
            label(b.text)
                .font(Font::Title3)
                .weight(FontWeight::Bold)
                .color(move || palette().text)
                .any()
        } else {
            label(b.text).color(move || palette().text).any()
        }
    };
    scroll(
        column((
            label(move || st.article.get().map(|a| a.feed_title).unwrap_or_default())
                .font(Font::Footnote)
                .color(move || palette().accent),
            label(move || {
                st.article
                    .get()
                    .and_then(|a| a.title)
                    .unwrap_or_else(|| crate::res::str::untitled().format())
            })
            .font(Font::Title2)
            .bold()
            .color(move || palette().text),
            label(move || {
                st.article
                    .get()
                    .map(|a| full_date(a.published_at))
                    .unwrap_or_default()
            })
            .font(Font::Caption)
            .color(move || palette().text_muted),
            each(
                items(body, |(key, _): &(String, crate::format::Block)| {
                    key.clone()
                }),
                move |slot| block(slot.get().1),
            ),
        ))
        .spacing(10.0)
        .align(HAlign::Leading)
        .padding(24.0)
        .grow_w(),
    )
    .id("reader-text")
    .grow()
}

/// The reader pane. Empty state until an article is open.
///
/// The article's commands are declared here (docs/toolbars.md): next-unread, star and mark-read
/// all act on what this pane is showing, so they ride its chrome and leave when it does. The
/// window's bar cannot see the open article, which is why they used to need mirror signals.
pub fn reader_pane() -> impl Piece {
    let st = daynews_core::scene();
    let view = crate::reader_view::ReaderView::ambient();
    // Two windows may show the same article in different modes. Never share their files.
    static NEXT_READER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(1);
    let slot = NEXT_READER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    #[cfg(not(target_arch = "wasm32"))]
    let filename = format!("reader-{}-{slot}.html", std::process::id());
    #[cfg(target_arch = "wasm32")]
    let filename = format!("reader-{slot}.html");
    let path = app_temp_dir().join("news-reader").join(filename);
    let cleanup_path = path.clone();
    day::reactive::Scope::current().on_cleanup(move || {
        let _ = std::fs::remove_file(cleanup_path);
    });
    let url = Signal::new(String::new());
    // The web view's bound URL is imperative: it loads on creation and thereafter only
    // when a `go` trigger fires (navigation writes the signal back, so auto-loading on every
    // change would loop). Writing the URL alone left the pane showing the first article forever.
    let go = Trigger::new();
    // Re-render only for article/locale changes. Typography and colors update the live
    // document through its dedicated stylesheet, preserving reading position.
    bind(
        move || {
            (
                st.article
                    .with(|a| a.as_ref().map(|a| (a.id, a.url.clone()))),
                view.active.get(),
                view.extracted.get(),
                day::locale().get(),
            )
        },
        move |_| {
            view.ready.set(false);
            let doc = view
                .article(st.article.get_untracked())
                .and_then(|a| render_to_file(&path, &a));
            url.set(doc.unwrap_or_default());
            go.notify();
        },
    );

    column((
        when(
            move || view.loading.get() || view.error.get().is_some(),
            move || {
                label(move || view.status())
                    .font(Font::Footnote)
                    .id("reader-view-status")
                    .padding(8.0)
            },
        ),
        when(
            move || st.article.get().is_none(),
            crate::dashboard::dashboard,
        ),
        when(
            move || st.article.get().is_some(),
            move || reader_body(url, go, view),
        ),
    ))
    .background(move || palette().bg)
    .grow()
    // The commands follow the open article: a new one re-derives the list with its own starred
    // and read state, and no article at all contributes nothing, so the bar carries them only
    // while there is something to act on (https://daybrite.dev/docs/toolbars).
    .toolbar(move || match st.article.get() {
        Some(a) => crate::toolbar::article_items(a),
        None => Vec::new(),
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn document_escapes_metadata_and_resolves_publication_links() {
        // Synthetic publication data, not app-owned UI strings.
        let article = daynews_core::StoredArticle {
            id: 1,
            feed_id: 1,
            feed_title: "Fixture & source".into(),
            title: Some("<Fixture title>".into()),
            url: Some("https://fixture.example/posts/story?x=1&y=2".into()),
            author: Some("<Fixture author>".into()),
            published_at: 0,
            summary: None,
            content_html: Some(r#"<p><img src="../image.png"></p>"#.into()),
            is_read: false,
            is_starred: false,
        };
        let html = super::document_in_appearance(&article, false);
        assert!(html.contains("&lt;Fixture title&gt;"));
        assert!(html.contains("&lt;Fixture author&gt;"));
        assert!(html.contains(r#"<base href="https://fixture.example/posts/story?x=1&amp;y=2">"#));
        assert!(html.contains(r#"<img src="../image.png">"#));
    }
}
