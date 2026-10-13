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

        // Test binaries have their own Apple preferences domain. Restore the key on
        // other desktop stores too, including when a child assertion fails.
        struct Restore(Option<String>);
        impl Drop for Restore {
            fn drop(&mut self) {
                if let Some(value) = &self.0 {
                    day::prefs::set(UNREAD_ONLY_KEY, value);
                } else {
                    day::prefs::remove(UNREAD_ONLY_KEY);
                }
            }
        }
        let _restore = Restore(day::prefs::get(UNREAD_ONLY_KEY));
        day::prefs::remove(UNREAD_ONLY_KEY);
        for mode in ["disabled", "enable", "enabled", "disable", "disabled"] {
            let output = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "feed_list::tests::unread_filter_survives_process_restart",
                ])
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
