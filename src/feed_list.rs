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
