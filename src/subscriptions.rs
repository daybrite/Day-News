//! Managing subscriptions: add by URL, import/export OPML, and per-feed actions.

use crate::theme::palette;
use day::prelude::*;
use daynews_core::FeedRow;

/// Focus for the "add a subscription" field, so File ▸ New Feed can put the cursor there.
/// PER WINDOW (docs/state.md): the command should focus the field in the window the user is
/// looking at, not in whichever window happened to build first.
#[derive(Clone, Copy)]
pub(crate) struct UrlFocus(Signal<bool>);

impl Ambient for UrlFocus {
    fn create() -> Self {
        UrlFocus(Signal::new(false))
    }
}

fn url_focus() -> Signal<bool> {
    UrlFocus::try_ambient()
        .or_else(UrlFocus::focused)
        .expect("no window is open")
        .0
}

/// File ▸ New Feed: put the cursor in the URL field (the page may have just mounted).
pub fn focus_url_field() {
    url_focus().set(true);
}

/// File ▸ New Folder: ask for a name, then create it.
pub fn begin_new_folder() {
    day::task(async {
        if let Some(name) = prompt(crate::res::str::new_folder_title())
            .placeholder(crate::res::str::new_folder_placeholder().format())
            .await
        {
            let name = name.trim().to_string();
            if !name.is_empty() {
                daynews_core::create_folder(&name);
            }
        }
    });
}

pub fn subscriptions_page() -> impl Piece {
    let st = daynews_core::state();
    let entry = Signal::new(String::new());
    let add = move || {
        let url = entry.get_untracked();
        if !url.trim().is_empty() {
            daynews_core::subscribe(&url);
            entry.set(String::new());
        }
    };

    scroll(
        column((
            label(crate::res::str::subscribe_heading())
                .font(Font::Headline)
                .color(move || palette().text),
            row((
                text_field(entry)
                    .placeholder(crate::res::str::subscribe_placeholder())
                    .focused(url_focus())
                    .id("subscribe-url")
                    .grow_w(),
                button(crate::res::str::subscribe_action())
                    .action(add)
                    .prominent()
                    .id("subscribe-add"),
            ))
            .spacing(8.0)
            .align(VAlign::Center)
            .grow_w(),
            label(crate::res::str::opml_heading())
                .font(Font::Headline)
                .color(move || palette().text)
                .padding(Insets {
                    top: 18.0,
                    ..Default::default()
                }),
            row((
                button(crate::res::str::opml_import())
                    .action(import_opml)
                    .id("opml-import"),
                button(crate::res::str::opml_export())
                    .action(export_opml)
                    .id("opml-export"),
            ))
            .spacing(8.0)
            .align(VAlign::Center),
            label(move || st.status.get())
                .font(Font::Footnote)
                .color(move || palette().text_muted)
                .id("status"),
            label(move || crate::res::str::feeds_count(st.feeds.with(|f| f.len()) as f64).format())
                .font(Font::Headline)
                .color(move || palette().text)
                // `.id()` BEFORE `.padding()`: a decorator returns a wrapper node, so an id applied
                // after one lands on the wrapper — which has no text for assertions to read.
                .id("feeds-count")
                .padding(Insets {
                    top: 18.0,
                    ..Default::default()
                }),
            each(
                items(move || st.feeds.get(), |f: &FeedRow| f.id.to_string()),
                |slot| feed_row(slot.get()),
            ),
        ))
        .spacing(8.0)
        .align(HAlign::Leading)
        .padding(18.0)
        .grow_w(),
    )
    .background(move || palette().bg)
    .grow()
}

fn feed_row(f: FeedRow) -> impl Piece {
    let id = f.id;
    let has_error = f.has_error;
    row((
        column((
            label(f.title.clone())
                .font(Font::Body)
                .color(move || palette().text),
            label(f.feed_url.clone())
                .font(Font::Caption2)
                .color(move || {
                    if has_error {
                        palette().error
                    } else {
                        palette().text_muted
                    }
                }),
        ))
        .spacing(1.0)
        .align(HAlign::Leading)
        .grow_w(),
        button(crate::res::str::unsubscribe())
            .action(move || daynews_core::unsubscribe(id))
            .id_of(move || format!("unsub-{id}")),
    ))
    .spacing(10.0)
    .align(VAlign::Center)
    .padding(Insets::symmetric(0.0, 6.0))
    .grow_w()
}

/// Import: pick an `.opml` file and merge its subscriptions in.
pub fn import_opml() {
    day::task(async {
        let Some(url) = open_file()
            .filter("Subscription lists", &["opml", "xml"])
            .await
        else {
            return;
        };
        match url.read() {
            Ok(bytes) => {
                let text = String::from_utf8_lossy(&bytes).to_string();
                if let Err(e) = daynews_core::import_opml(&text) {
                    daynews_core::state()
                        .status
                        .set(format!("Import failed: {e}"));
                } else {
                    // Newly imported feeds have no articles yet; fetch them straight away so the
                    // app is useful immediately after an import.
                    daynews_core::refresh_all();
                }
            }
            Err(e) => daynews_core::state()
                .status
                .set(format!("Could not read the file: {e}")),
        }
    });
}

/// Export: write every subscription out as OPML.
pub fn export_opml() {
    day::task(async {
        let text = daynews_core::export_opml();
        // `save_file` takes the bytes up front: the picker writes them itself, which is the only
        // shape that works on the sandboxed platforms (the app never gets a writable path).
        let saved = save_file(text.into_bytes())
            .suggested_name("Day-News-Subscriptions.opml")
            .filter("Subscription lists", &["opml"])
            .await;
        daynews_core::state().status.set(match saved {
            Some(_) => "Exported subscriptions".into(),
            None => String::new(),
        });
    });
}
