//! Full-content loading options: the app defaults are the settings prefs (`READER_INLINE`,
//! `READER_AUTO`); each feed's dashboard can override either, stored under the feed's stable id,
//! and inherits whichever default it leaves alone. Shared by every window.
use crate::settings::{READER_AUTO, READER_INLINE};
use day::prelude::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Options {
    pub inline: bool,
    pub auto: bool,
}

thread_local! {
    static REVISION: std::cell::OnceCell<Signal<u64>> = const { std::cell::OnceCell::new() };
}
/// Bumped on every override write, so readers of `options` re-derive.
fn revision() -> Signal<u64> {
    REVISION.with(|slot| *slot.get_or_init(|| Signal::new_in(day::reactive::Scope::root(), 0)))
}
fn key(feed: u64, auto: bool) -> String {
    let option = if auto { "auto" } else { "inline" };
    format!("news.reader.feed.{feed}.{option}")
}
/// A stored override wins over the default; an absent or unreadable one inherits it.
fn inherit(default: bool, stored: Option<&str>) -> bool {
    stored.and_then(|s| s.parse().ok()).unwrap_or(default)
}
/// The effective options: the app defaults, with `feed`'s overrides applied. Tracked.
pub fn options(feed: Option<u64>) -> Options {
    revision().get();
    let defaults = Options {
        inline: READER_INLINE.get(),
        auto: READER_AUTO.get(),
    };
    match feed {
        None => defaults,
        Some(id) => Options {
            inline: inherit(defaults.inline, day::prefs::get(&key(id, false)).as_deref()),
            auto: inherit(defaults.auto, day::prefs::get(&key(id, true)).as_deref()),
        },
    }
}
/// One feed's override of one option, as the dashboard toggles bind it.
#[derive(Clone, Copy)]
pub struct OptionBinding {
    pub feed: u64,
    pub auto: bool,
}
impl Binding<bool> for OptionBinding {
    fn read(&self) -> bool {
        let options = options(Some(self.feed));
        if self.auto {
            options.auto
        } else {
            options.inline
        }
    }
    fn peek(&self) -> bool {
        day::reactive::untrack(|| self.read())
    }
    fn write(&self, value: bool) {
        day::prefs::set(&key(self.feed, self.auto), &value.to_string());
        revision().update(|n| *n = n.wrapping_add(1));
    }
}
/// The feed dashboard's override controls, with the feed's reader-script editor.
pub fn controls(feed: u64) -> impl Piece {
    column((
        labeled(
            crate::res::str::reader_inline_mode(),
            toggle(OptionBinding { feed, auto: false })
                .enabled(|| !crate::site_browser::incognito())
                .id("feed-inline-reader"),
        ),
        labeled(
            crate::res::str::reader_auto_mode(),
            toggle(OptionBinding { feed, auto: true })
                .enabled(|| !crate::site_browser::incognito())
                .id("feed-auto-reader"),
        ),
        crate::reader_scripts::feed_controls(feed),
    ))
    .spacing(5.0)
    .grow_w()
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn overrides_inherit_defaults_and_ignore_invalid_persistence() {
        assert!(inherit(true, None));
        assert!(!inherit(false, None));
        assert!(inherit(false, Some("true")));
        assert!(!inherit(true, Some("false")));
        assert!(inherit(true, Some("invalid")));
        assert!(!inherit(false, Some("invalid")));
    }
}
