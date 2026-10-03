//! Shared, persistent sidebar filtering. Article scopes and smart feeds stay independent.
use day::prelude::*;

const UNREAD_ONLY_KEY: &str = "news.feeds.unread_only";

#[derive(Clone, Copy)]
pub struct FeedList {
    pub unread_only: Signal<bool>,
}

impl Ambient for FeedList {
    fn create() -> Self {
        Self {
            unread_only: Signal::new(day::prefs::get(UNREAD_ONLY_KEY).as_deref() == Some("true")),
        }
    }
}

pub fn toggle() {
    let state = FeedList::app();
    let on = !state.unread_only.get_untracked();
    if on {
        day::prefs::set(UNREAD_ONLY_KEY, "true");
    } else {
        day::prefs::remove(UNREAD_ONLY_KEY);
    }
    state.unread_only.set(on);
}

pub fn visible(unread: i64, unread_only: bool) -> bool {
    !unread_only || unread > 0
}

pub fn toggle_grouping() {
    let signal = daynews_core::state().group_by_feed;
    let on = !signal.get_untracked();
    day::prefs::set(
        "news.articles.group_by_feed",
        if on { "true" } else { "false" },
    );
    signal.set(on);
}

pub fn move_relative(feed: u64, delta: isize) {
    let feeds = daynews_core::state().feeds.get_untracked();
    if let Some(from) = feeds.iter().position(|f| f.id == feed) {
        daynews_core::move_feed(
            feed,
            from.saturating_add_signed(delta)
                .min(feeds.len().saturating_sub(1)),
        );
    }
}
