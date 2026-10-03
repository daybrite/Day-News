//! Dedicated site profiles shared by the interactive browser and inert reader extraction.
use crate::res;
use day::prelude::*;
use day_piece_webview::{JsHandle, WebProfile, web_view};
thread_local! {
    static REVISION: std::cell::OnceCell<Signal<u64>> = const { std::cell::OnceCell::new() };
}
fn revision() -> Signal<u64> {
    REVISION.with(|r| *r.get_or_init(|| Signal::new_in(day::reactive::Scope::root(), 0)))
}
pub fn incognito() -> bool {
    revision().get();
    day::prefs::get("news.browser.incognito").as_deref() == Some("true")
}
#[derive(Clone, Copy)]
struct Privacy;
impl Binding<bool> for Privacy {
    fn read(&self) -> bool {
        incognito()
    }
    fn peek(&self) -> bool {
        incognito()
    }
    fn write(&self, value: bool) {
        day::prefs::set("news.browser.incognito", &value.to_string());
        let revision = revision();
        revision.update(|n| *n = n.wrapping_add(1));
    }
}
pub fn privacy_control() -> impl Piece {
    column((
        labeled(
            res::str::incognito_mode(),
            toggle(Privacy).id("incognito-mode-picker"),
        ),
        when(
            || incognito() && day_piece_webview::private_browsing_support() == Support::Unsupported,
            || label(res::str::site_private_unsupported()).font(Font::Footnote),
        ),
    ))
}
pub fn site_url(feed: u64) -> Option<String> {
    daynews_core::state()
        .feeds
        .with_untracked(|rows| {
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
pub fn show_dashboard() {
    let scene = daynews_core::scene();
    batch(|| {
        scene.selected.set(None);
        scene.article.set(None);
        scene.reader_open.set(true);
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
            && key.as_deref().is_some_and(|key| key.starts_with("feed:"))
        {
            batch(|| {
                self.scene.selected.set(None);
                self.scene.article.set(None);
                self.scene.reader_open.set(true);
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
pub fn site_controls(feed: u64) -> impl Piece {
    let browser = Browser::ambient();
    let clearing = Signal::new(false);
    let clear = Signal::new(false);
    let started = Signal::new(false);
    let status = Signal::new(String::new());
    let engine = JsHandle::new();
    let url = Signal::new("about:blank".to_owned());
    column((
        row((
            button(res::str::site_preview())
                .action(move || browser.open.set(Some(feed.to_string())))
                .enabled(move || {
                    !incognito()
                        && !clearing.get()
                        && site_url(feed).is_some()
                        && day_piece_webview::support() == Support::Native
                })
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
        .spacing(8.0),
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
    .spacing(4.0)
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
        column((
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
                .spacing(8.0),
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
                .spacing(8.0),
            ))
            .spacing(8.0)
            .padding(8.0),
            web_view(address)
                .profile(profile(feed))
                .back(back)
                .forward(forward)
                .go(go)
                .on_external_link(|_| day_piece_webview::LinkPolicy::InView)
                .id("site-browser-web")
                .grow(),
        ))
        .grow()
    })
    .unrouted()
}
