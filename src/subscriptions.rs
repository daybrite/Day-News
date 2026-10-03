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

/// Open the URL prompt in the initiating window. Clipboard access starts in the user gesture.
pub fn begin_new_feed() {
    if daynews_core::try_scene().is_none() {
        return;
    }
    let clipboard = day::clipboard::read(&["text/plain", "text/uri-list"]);
    day::task(async move {
        let initial = clipboard
            .await
            .ok()
            .flatten()
            .and_then(|r| String::from_utf8(r.bytes.as_ref().clone()).ok())
            .and_then(|s| daynews_feed::discovery::web_url(s.trim()).map(|u| u.to_string()))
            .unwrap_or_default();
        let Some(input) = prompt(crate::res::str::menu_new_feed())
            .placeholder(crate::res::str::subscribe_placeholder())
            .initial(initial)
            .ok_label(crate::res::str::subscribe_action())
            .await
        else {
            return;
        };
        let input = input.trim();
        if input.is_empty() {
            return;
        }
        let result = if input.starts_with("asset:") {
            // Deterministic app-owned fixtures used by dayscript, never a clipboard candidate.
            daynews_core::subscribe_asset(input.to_owned()).await
        } else {
            let Some(normalized) = subscription_url(input) else {
                alert(crate::res::str::subscribe_invalid()).await;
                return;
            };
            let resolved = match daynews_feed::discovery::discover(&normalized).await {
                Ok(daynews_feed::discovery::Discovery::Feed { url, update }) => Some((url, update)),
                Ok(daynews_feed::discovery::Discovery::Candidates(options)) => {
                    if options.is_empty() {
                        alert(crate::res::str::subscribe_missing())
                            .message(crate::res::str::subscribe_missing_note())
                            .await;
                        return;
                    }
                    let chosen = if options.len() == 1 {
                        Some(options[0].url.clone())
                    } else {
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
                        dialog.cancel(crate::res::str::cancel_action()).await
                    };
                    let Some(url) = chosen else {
                        return;
                    };
                    // Resolve and validate only the chosen candidate. Other feeds incur no requests.
                    match daynews_feed::discovery::discover(&url).await {
                        Ok(daynews_feed::discovery::Discovery::Feed { url, update }) => {
                            Some((url, update))
                        }
                        _ => {
                            alert(crate::res::str::subscribe_failed()).await;
                            return;
                        }
                    }
                }
                Err(_) => {
                    alert(crate::res::str::subscribe_failed()).await;
                    return;
                }
            };
            let Some((url, update)) = resolved else {
                return;
            };
            daynews_core::subscribe_discovered(url, update).await
        };
        match result {
            Ok(feed) => select_subscribed_feed(feed).await,
            Err(_) => {
                alert(crate::res::str::subscribe_failed()).await;
            }
        }
    });
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
