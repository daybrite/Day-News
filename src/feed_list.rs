//! Shared, persistent sidebar filtering. Article scopes and smart feeds stay independent.
use day::prelude::*;

const UNREAD_ONLY_KEY: &str = "news.feeds.unread_only";

#[derive(Clone, Copy)]
pub struct FeedList {
    pub unread_only: Signal<bool>,
}

impl Ambient for FeedList {
    fn create() -> Self {
        let unread_only = Signal::new(false);
        // Restore before any window reads it, and persist every change for the app's
        // lifetime, including changes made while revealing a subscribed publisher.
        day::prefs::bind(UNREAD_ONLY_KEY, unread_only);
        Self { unread_only }
    }
}

pub fn toggle() {
    let state = FeedList::app();
    let on = !state.unread_only.get_untracked();
    state.unread_only.set(on);
}

pub fn visible(unread: i64, unread_only: bool) -> bool {
    !unread_only || unread > 0
}

/// Feed ▸ Group by Feeds: flips the setting; `settings::install` mirrors it into
/// daynews-core's flag for the timeline.
pub fn toggle_grouping() {
    let pref = &crate::settings::GROUP_BY_FEED;
    pref.set(!pref.get_untracked());
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

#[cfg(all(
    test,
    any(target_os = "macos", target_os = "linux", target_os = "windows")
))]
mod tests {
    use super::*;

    #[test]
    fn unread_filter_survives_process_restart() {
        const CHILD: &str = "DAY_NEWS_TEST_UNREAD_FILTER";
        if let Ok(mode) = std::env::var(CHILD) {
            let state = FeedList::app();
            match mode.as_str() {
                "enable" => {
                    assert!(!state.unread_only.get_untracked());
                    toggle();
                }
                "disable" => {
                    assert!(state.unread_only.get_untracked());
                    toggle();
                }
                "enabled" => assert!(state.unread_only.get_untracked()),
                "disabled" => assert!(!state.unread_only.get_untracked()),
                _ => panic!("unknown synthetic test mode"),
            }
            day::reactive::flush_sync();
            return;
        }

        let preferences = crate::test_prefs::PreferenceTest::new(UNREAD_ONLY_KEY);
        for mode in ["disabled", "enable", "enabled", "disable", "disabled"] {
            let output = preferences
                .child("feed_list::tests::unread_filter_survives_process_restart")
                .env(CHILD, mode)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{mode}: {}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }
}

/// Count subscriptions, not articles, before applying the unread-only filter.
pub fn section_title() -> String {
    daynews_core::state().feeds.with(|feeds| {
        let unread = feeds.iter().filter(|feed| feed.unread > 0).count();
        crate::res::str::nav_feeds_section_counts(
            day::format_decimal(feeds.len() as f64, 0),
            day::format_decimal(unread as f64, 0),
        )
        .format()
    })
}
