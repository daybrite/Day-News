//! Dedicated site profiles shared by the interactive browser and inert reader extraction.
use crate::res;
use day::prelude::*;
use day_piece_webview::{JsHandle, WebProfile, web_view};
/// Tracked read of the Incognito Mode setting (Settings ▸ Browser).
pub fn incognito() -> bool {
    crate::settings::INCOGNITO.get()
}
pub fn site_url(feed: u64) -> Option<String> {
    daynews_core::state()
        .feeds
        .with(|rows| {
            rows.iter()
                .find(|row| row.id == feed)
                .map(|row| row.site_url.clone().unwrap_or_else(|| row.feed_url.clone()))
        })
        .filter(|url| crate::extraction::web_url(url).is_some())
}
pub fn profile(feed: u64) -> WebProfile {
    let name = site_url(feed)
        .and_then(|url| url::Url::parse(&url).ok())
        .map(|url| url.origin().ascii_serialization())
        .unwrap_or_else(|| format!("feed:{feed}"));
    if day_piece_webview::profile_support() != Support::Native {
        return if incognito() {
            WebProfile::private("default")
        } else {
            WebProfile::default()
        };
    }
    if incognito() {
        WebProfile::private(format!("news:{name}"))
    } else {
        WebProfile::persistent(format!("news:{name}"))
    }
}
/// Manual article deselection returns to its publisher rather than the smart-feed scope.
pub fn show_article_feed_dashboard() {
    let scene = daynews_core::scene();
    let Some(selected) = scene.selected.get_untracked() else {
        // Scope changes already clear selection. Do not redirect those navigation events.
        return;
    };
    let feed = scene
        .article
        .with_untracked(|article| {
            article
                .as_ref()
                .filter(|a| a.id == selected)
                .map(|a| a.feed_id)
        })
        .or_else(|| {
            scene.articles.with_untracked(|articles| {
                articles
                    .iter()
                    .find(|a| a.id == selected)
                    .map(|a| a.feed_id)
            })
        });
    let Some(feed) = feed else { return };
    show_feed_dashboard(feed);
}
/// Open a sidebar publisher's overview, regardless of the current article or scope.
pub fn show_feed_dashboard(feed: u64) {
    let scene = daynews_core::scene();
    batch(|| {
        if crate::feed_list::FeedList::app()
            .unread_only
            .get_untracked()
            && daynews_core::state()
                .feeds
                .with_untracked(|feeds| feeds.iter().any(|f| f.id == feed && f.unread == 0))
        {
            crate::feed_list::toggle();
        }
        daynews_core::select_scope(daynews_db::Scope::Feed(feed));
        navigate(&format!("feed:{feed}"));
        scene.article.set(None);
        day::reactive::at_turn_end(move || scene.reader_open.set(true));
    });
}
#[derive(Clone, Copy)]
pub struct FeedSelection {
    pub section: Signal<Option<String>>,
    pub scene: daynews_core::NewsScene,
}
impl Binding<Option<String>> for FeedSelection {
    fn read(&self) -> Option<String> {
        self.section.get()
    }
    fn peek(&self) -> Option<String> {
        self.section.get_untracked()
    }
    fn write(&self, key: Option<String>) {
        if self.section.get_untracked() == key
            && key.as_deref().and_then(crate::scope_for_key).is_some()
        {
            batch(|| {
                self.scene.selected.set(None);
                self.scene.article.set(None);
                self.scene
                    .reader_open
                    .set(day_core::capability(day_spec::Cap::NavContentList) != Support::Emulated);
            });
        }
        self.section.set(key);
    }
}
#[derive(Clone, Copy)]
pub(crate) struct Browser {
    open: Signal<Option<String>>,
}
impl Ambient for Browser {
    fn create() -> Self {
        let state = Self {
            open: Signal::new(None),
        };
        watch(incognito, move |private, _| {
            if *private {
                state.open.set(None);
            }
        });
        state
    }
}
/// The Feed menu follows the focused window's sidebar selection.
pub fn selected_feed() -> Option<u64> {
    match daynews_core::try_scene()?.scope.get() {
        daynews_db::Scope::Feed(feed) => Some(feed),
        _ => None,
    }
}
pub fn can_preview(feed: u64) -> bool {
    !incognito() && day_piece_webview::support() == Support::Native && site_url(feed).is_some()
}
/// Shared entry point for the dashboard, app menu and the context-clicked feed.
pub fn preview(feed: u64) {
    let Some(browser) = Browser::try_ambient().or_else(Browser::focused) else {
        return;
    };
    open_preview(browser, feed);
}
/// Row and button callbacks retain their owning window even if focus changes meanwhile.
pub fn preview_action(feed: u64) -> impl Fn() {
    let browser = Browser::ambient();
    move || open_preview(browser, feed)
}
fn open_preview(browser: Browser, feed: u64) {
    if can_preview(feed) {
        browser.open.set(Some(feed.to_string()));
    }
}
pub fn site_controls(feed: u64) -> impl Piece {
    let clearing = Signal::new(false);
    let clear = Signal::new(false);
    let started = Signal::new(false);
    let status = Signal::new(String::new());
    let engine = JsHandle::new();
    let url = Signal::new("about:blank".to_owned());
    column((
        row((
            button(res::str::site_preview())
                .action(preview_action(feed))
                .enabled(move || !clearing.get() && can_preview(feed))
                .id("dashboard-site-preview"),
            button(res::str::site_forget())
                .action(move || {
                    clearing.set(true);
                    started.set(false);
                    status.set(String::new());
                    clear.set(true);
                })
                .enabled(move || {
                    !incognito()
                        && !clearing.get()
                        && day_piece_webview::clear_data_support() == Support::Native
                        && day_piece_webview::profile_support() == Support::Native
                })
                .id("dashboard-site-forget"),
        ))
        .spacing(Space::S),
        label(move || status.get())
            .font(Font::Caption)
            .id("dashboard-site-status"),
        when(
            move || clear.get(),
            move || {
                web_view(url)
                    .profile(profile(feed))
                    .js(engine)
                    .on_load(move || {
                        if started.get_untracked() {
                            return;
                        }
                        started.set(true);
                        day::task(async move {
                            let result = futures_util::future::select(
                                Box::pin(engine.clear_data()),
                                Box::pin(day::sleep(15_000)),
                            )
                            .await;
                            let succeeded =
                                matches!(result, futures_util::future::Either::Left((Ok(_), _)));
                            status.set(if succeeded {
                                res::str::site_data_cleared().format()
                            } else {
                                res::str::site_data_failed().format()
                            });
                            clear.set(false);
                            clearing.set(false);
                        });
                    })
                    .id("site-data-web")
                    .frame(1.0, 1.0)
            },
        ),
    ))
    .spacing(Space::XS)
}
pub fn browser_cover() -> impl Piece {
    // Created in the window's ambient scope; dashboard controls find this same instance.
    let browser = Browser::ambient();
    cover(browser.open, move |feed: &String| {
        let feed = feed.parse::<u64>().unwrap_or_default();
        let address = Signal::new(site_url(feed).unwrap_or_else(|| "about:blank".into()));
        let back = Trigger::new();
        let forward = Trigger::new();
        let go = Trigger::new();
        let content = column((
            column((
                row((
                    button(res::str::site_browser_back())
                        .action(move || back.notify())
                        .id("site-browser-back"),
                    button(res::str::site_browser_forward())
                        .action(move || forward.notify())
                        .id("site-browser-forward"),
                    spacer().grow_w(),
                    button(res::str::site_browser_close())
                        .action(move || browser.open.set(None))
                        .id("site-browser-close"),
                ))
                .spacing(Space::S),
                row((
                    text_field(address)
                        .placeholder(res::str::site_browser_address())
                        .id("site-browser-address")
                        .grow_w(),
                    button(res::str::site_browser_go())
                        .action(move || {
                            if crate::extraction::web_url(&address.get_untracked()).is_some() {
                                go.notify();
                            }
                        })
                        .id("site-browser-go"),
                ))
                .spacing(Space::S),
            ))
            .spacing(Space::S)
            .padding(Space::S),
            web_view(address)
                .profile(profile(feed))
                .back(back)
                .forward(forward)
                .go(go)
                .on_external_link(|_| day_piece_webview::LinkPolicy::InView)
                .id("site-browser-web")
                .grow(),
        ))
        .id("site-browser-sheet")
        .grow();
        if cfg!(any(feature = "appkit", feature = "uikit")) {
            content.any()
        } else {
            // The cover still owns the modal input boundary on backends without native
            // sheets. Keep its browser panel narrow and let the dimmed window show beside it;
            // on a phone max_width simply accepts the available width.
            zstack((content
                .background(|| crate::theme::palette().bg)
                .max_width(640.0),))
            .grow()
            .any()
        }
    })
    .sheet()
    .background(|_| {
        if cfg!(any(feature = "appkit", feature = "uikit")) {
            crate::theme::palette().bg
        } else {
            Color::rgba(0.0, 0.0, 0.0, 0.35)
        }
    })
    .unrouted()
}
