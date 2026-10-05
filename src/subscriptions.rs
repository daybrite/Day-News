//! Managing subscriptions: add by URL, import/export OPML, and per-feed actions.

use day::prelude::*;

fn subscription_url(input: &str) -> Option<String> {
    let input = input.trim();
    // A bare hostname with a port can parse as an opaque URL scheme. Preserve that input
    // convenience while rejecting actual non-web protocols and embedded credentials.
    let bare_port = input.split_once(':').is_some_and(|(_, rest)| {
        rest.split('/')
            .next()
            .is_some_and(|port| port.parse::<u16>().is_ok())
    });
    if url::Url::parse(input).is_ok()
        && daynews_feed::discovery::web_url(input).is_none()
        && !bare_port
    {
        return None;
    }
    daynews_feed::discovery::web_url(&daynews_core::normalize_feed_url(input))
        .map(|u| u.to_string())
}

/// Window-owned subscription sheet shared by File > New Feed and URL activation.
#[derive(Clone, Copy)]
pub struct SubscriptionSheet {
    open: Signal<Option<String>>,
}
impl Ambient for SubscriptionSheet {
    fn create() -> Self {
        Self {
            open: Signal::new(None),
        }
    }
}
pub fn begin_new_feed() {
    if daynews_core::try_scene().is_none() {
        return;
    }
    let Some(sheet) = SubscriptionSheet::try_ambient().or_else(SubscriptionSheet::focused) else {
        return;
    };
    let clipboard = day::clipboard::read(&["text/plain", "text/uri-list"]);
    day::task(async move {
        let initial = clipboard
            .await
            .ok()
            .flatten()
            .and_then(|r| String::from_utf8(r.bytes.as_ref().clone()).ok())
            .and_then(|s| {
                incoming_url(&s)
                    .or_else(|| daynews_feed::discovery::web_url(s.trim()).map(|u| u.to_string()))
            })
            .unwrap_or_default();
        sheet.open.set(Some(initial));
    });
}
fn is_feed_scheme(input: &str) -> bool {
    input.split_once(':').is_some_and(|(scheme, _)| {
        ["feed", "feeds", "rss", "atom", "web+feed"]
            .iter()
            .any(|s| s.eq_ignore_ascii_case(scheme))
    })
}
fn incoming_url(input: &str) -> Option<String> {
    let (scheme, rest) = input.split_once(':')?;
    if !["feed", "feeds", "rss", "atom", "web+feed"]
        .iter()
        .any(|s| scheme.eq_ignore_ascii_case(s))
    {
        return None;
    }
    let candidate = if rest.starts_with("//") {
        format!(
            "{}:{rest}",
            if scheme.eq_ignore_ascii_case("feeds") {
                "https"
            } else {
                "http"
            }
        )
    } else {
        rest.to_owned()
    };
    subscription_url(&candidate)
}
async fn verify_url(input: &str) -> Result<(String, daynews_feed::FeedUpdate), ()> {
    if input.starts_with("asset:") {
        let feed = daynews_core::preview_feed(input).await.map_err(|_| ())?;
        return Ok((
            input.into(),
            daynews_feed::FeedUpdate::Modified(feed, Default::default(), None),
        ));
    }
    let normalized = incoming_url(input)
        .or_else(|| subscription_url(input))
        .ok_or(())?;
    let discovered = daynews_feed::discovery::discover(&normalized)
        .await
        .map_err(|_| ())?;
    match discovered {
        daynews_feed::discovery::Discovery::Feed { url, update } => Ok((url, *update)),
        daynews_feed::discovery::Discovery::Candidates(options) => {
            let url = if options.len() == 1 {
                options[0].url.clone()
            } else {
                if options.is_empty() {
                    return Err(());
                }
                let mut dialog = Alert::new(crate::res::str::subscribe_choose())
                    .message(crate::res::str::subscribe_choose_note());
                for option in options {
                    let label = option
                        .title
                        .as_deref()
                        .map(|title| {
                            crate::res::str::subscribe_candidate(title, option.url.as_str())
                                .format()
                        })
                        .unwrap_or_else(|| option.url.clone());
                    dialog = dialog.button(label, option.url);
                }
                dialog
                    .cancel(crate::res::str::cancel_action())
                    .await
                    .ok_or(())?
            };
            match daynews_feed::discovery::discover(&url)
                .await
                .map_err(|_| ())?
            {
                daynews_feed::discovery::Discovery::Feed { url, update } => Ok((url, *update)),
                _ => Err(()),
            }
        }
    }
}
pub fn subscription_sheet() -> impl Piece {
    let sheet = SubscriptionSheet::ambient();
    day::on_open_url(move |url| {
        if let Some(url) = incoming_url(url)
            .or_else(|| daynews_feed::discovery::web_url(url).map(|u| u.to_string()))
        {
            sheet.open.set(Some(url));
            true
        } else if is_feed_scheme(url) {
            sheet.open.set(Some(url.to_owned()));
            true
        } else {
            false
        }
    });
    day::on_open_files(move |files| {
        if let Some(locator) = files.into_iter().next() {
            day::task(async move {
                // Downloaded feeds usually omit their source URL. Discover from their site;
                // temporary file paths must never become refreshable subscriptions.
                let site = FileUrl::new(locator.clone())
                    .read_limited(16 * 1024 * 1024)
                    .await
                    .ok()
                    .and_then(|bytes| daynews_feed::parse(&bytes, &locator).ok())
                    .and_then(|feed| {
                        feed.source_url
                            .and_then(|s| daynews_feed::discovery::web_url(&s))
                            .or_else(|| {
                                feed.site_url
                                    .and_then(|s| daynews_feed::discovery::web_url(&s))
                            })
                            .map(|u| u.to_string())
                    });
                sheet.open.set(Some(site.unwrap_or_default()));
            });
        }
    });
    cover(sheet.open, move |initial: &String| {
        let address = Signal::new(initial.clone());
        let busy = Signal::new(false);
        let status = Signal::new(String::new());
        let articles = Signal::new(Vec::<daynews_feed::ParsedItem>::new());
        let title = Signal::new(String::new());
        let verified_input = Signal::new(None::<String>);
        let verified = std::rc::Rc::new(std::cell::RefCell::new(None));
        let verify_result = verified.clone();
        let subscribe_result = verified.clone();
        column((
            label(crate::res::str::menu_new_feed())
                .font(Font::Title)
                .id("subscription-title"),
            label(crate::res::str::subscribe_preview_note()).font(Font::Footnote),
            row((
                text_field(address)
                    .placeholder(crate::res::str::subscribe_placeholder())
                    .id("feed-url")
                    .grow_w(),
                button(crate::res::str::subscribe_verify())
                    .enabled(move || !busy.get() && !address.get().trim().is_empty())
                    .action(move || {
                        busy.set(true);
                        status.set(String::new());
                        articles.set(Vec::new());
                        verified_input.set(None);
                        verify_result.borrow_mut().take();
                        let input = address.get_untracked().trim().to_owned();
                        let result = verify_result.clone();
                        day::task(async move {
                            match verify_url(&input).await {
                                Ok((url, update)) if address.get_untracked().trim() == input => {
                                    if let daynews_feed::FeedUpdate::Modified(feed, _, _) = &update
                                    {
                                        title
                                            .set(feed.title.clone().unwrap_or_else(|| url.clone()));
                                        articles.set(feed.items.clone());
                                    }
                                    *result.borrow_mut() = Some((url, update));
                                    verified_input.set(Some(input));
                                    status.set(
                                        crate::res::str::subscribe_preview_ready(
                                            articles.with_untracked(|a| a.len()) as f64,
                                        )
                                        .format(),
                                    );
                                }
                                Ok(_) => (),
                                Err(()) => status.set(crate::res::str::subscribe_failed().format()),
                            }
                            busy.set(false);
                        });
                    })
                    .id("feed-verify"),
            ))
            .spacing(8.0),
            when(
                move || busy.get(),
                || label(crate::res::str::subscribe_verifying()).id("feed-verifying"),
            ),
            label(move || status.get())
                .font(Font::Footnote)
                .id("feed-verification-status"),
            label(move || title.get())
                .font(Font::Headline)
                .id("feed-preview-title"),
            list(items(move || articles.get(), |a| a.guid.clone()), |slot| {
                column((
                    label(move || slot.with(|a| a.display_title()))
                        .font(Font::Headline)
                        .max_lines(2),
                    label(move || {
                        slot.with(|a| {
                            a.summary
                                .as_deref()
                                .map(daynews_feed::clean)
                                .unwrap_or_default()
                        })
                    })
                    .font(Font::Footnote)
                    .max_lines(2),
                ))
                .align(HAlign::Leading)
                .padding(8.0)
            })
            .row_height(RowHeight::Uniform(100.0))
            .id("feed-preview")
            .grow(),
            row((
                button(crate::res::str::cancel_action())
                    .action(move || sheet.open.set(None))
                    .id("feed-cancel"),
                spacer().grow_w(),
                button(crate::res::str::subscribe_action())
                    .enabled(move || {
                        !busy.get() && verified_input.get().as_deref() == Some(address.get().trim())
                    })
                    .action(move || {
                        let Some((url, update)) = subscribe_result.borrow_mut().take() else {
                            return;
                        };
                        busy.set(true);
                        day::task(async move {
                            match daynews_core::subscribe_discovered(url, update).await {
                                Ok(feed) => {
                                    busy.set(false);
                                    sheet.open.set(None);
                                    select_subscribed_feed(feed).await;
                                    return;
                                }
                                Err(_) => {
                                    verified_input.set(None);
                                    status.set(crate::res::str::subscribe_failed().format());
                                }
                            }
                            busy.set(false);
                        });
                    })
                    .id("feed-subscribe"),
            ))
            .spacing(8.0),
        ))
        .spacing(12.0)
        .align(HAlign::Leading)
        .padding(16.0)
        .grow()
        .id("subscription-sheet")
    })
    .sheet()
    .unrouted()
}

/// Wait for the committed feed projection and its native sidebar bindings to settle.
/// This also makes an empty source visible when the unread-only filter was active.
async fn select_subscribed_feed(feed: u64) {
    let st = daynews_core::state();
    let scene = daynews_core::scene();
    scene.search.set(String::new());
    let (tx, rx) = day_async::oneshot();
    let sender = std::cell::RefCell::new(Some(tx));
    let ready = day::reactive::Scope::child();
    ready.enter(|| {
        day::reactive::batch(|| {
            day::reactive::Effect::new(move || {
                if let Some(unread) = st
                    .feeds
                    .with(|feeds| feeds.iter().find(|f| f.id == feed).map(|f| f.unread))
                {
                    if unread == 0
                        && crate::feed_list::FeedList::app()
                            .unread_only
                            .get_untracked()
                    {
                        crate::feed_list::toggle();
                    }
                    if let Some(tx) = sender.borrow_mut().take() {
                        day::reactive::at_turn_end(move || tx.send(()));
                    }
                }
            })
        })
    });
    let visible = rx.await.is_ok();
    ready.dispose();
    if visible {
        navigate(&format!("feed:{feed}"));
        daynews_core::select_scope(daynews_db::Scope::Feed(feed));
    }
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

/// Import: pick an `.opml` file and merge its subscriptions in.
pub fn import_opml() {
    day::task(async {
        let Some(url) = open_file()
            .filter(crate::res::str::opml_filter().format(), &["opml", "xml"])
            .await
        else {
            return;
        };
        match url.read() {
            Ok(bytes) => {
                let text = String::from_utf8_lossy(&bytes).to_string();
                if let Err(e) = daynews_core::import_opml(&text).await {
                    daynews_core::state()
                        .status
                        .set(crate::res::str::opml_import_failed(e.as_str()).format());
                } else {
                    // Newly imported feeds have no articles yet; fetch them straight away so the
                    // app is useful immediately after an import.
                    daynews_core::refresh_all();
                }
            }
            Err(e) => daynews_core::state()
                .status
                .set(crate::res::str::opml_read_failed(e.to_string()).format()),
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
            .filter(crate::res::str::opml_filter().format(), &["opml"])
            .await;
        daynews_core::state().status.set(match saved {
            Some(_) => crate::res::str::opml_exported().format(),
            None => String::new(),
        });
    });
}

#[cfg(test)]
mod tests {
    #[test]
    fn incoming_feed_schemes_preserve_url_data() {
        for (input, expected) in [
            (
                "feed:https://fixture.example/rss?q=a%26b",
                "https://fixture.example/rss?q=a%26b",
            ),
            ("feed://fixture.example/rss", "http://fixture.example/rss"),
            (
                "feeds://fixture.example/atom",
                "https://fixture.example/atom",
            ),
            (
                "ATOM:https://fixture.example/feed",
                "https://fixture.example/feed",
            ),
            (
                "web+feed:https://fixture.example/feed",
                "https://fixture.example/feed",
            ),
        ] {
            assert_eq!(super::incoming_url(input).as_deref(), Some(expected));
        }
        for bad in [
            "feed:javascript:alert(1)",
            "feed:https://user:pass@fixture.example/",
            "unknown:https://fixture.example/",
        ] {
            assert!(super::incoming_url(bad).is_none());
        }
    }

    #[test]
    fn subscription_input_accepts_bare_hosts_and_rejects_non_web_urls() {
        for input in [
            "https://fixture.example/feed",
            "fixture.example/feed",
            "localhost:28763/feed",
            "http://127.0.0.1:28763/feed",
        ] {
            assert!(super::subscription_url(input).is_some(), "{input}");
        }
        for input in [
            "file:///tmp/feed",
            "file:/tmp/feed",
            "ftp://fixture.example/feed",
            "javascript:alert(1)",
            "https://user:pass@fixture.example/feed",
            "not a URL",
            "",
        ] {
            assert!(super::subscription_url(input).is_none(), "{input}");
        }
    }
}
