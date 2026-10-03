//! Persisted reader defaults and per-feed overrides, shared by every window.
use day::prelude::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Options {
    pub inline: bool,
    pub auto: bool,
}
impl Default for Options {
    fn default() -> Self {
        Self {
            inline: true,
            auto: false,
        }
    }
}
thread_local! {
    static REVISION: std::cell::OnceCell<Signal<u64>> = const { std::cell::OnceCell::new() };
}
fn revision() -> Signal<u64> {
    REVISION.with(|slot| *slot.get_or_init(|| Signal::new_in(day::reactive::Scope::root(), 0)))
}
fn key(feed: Option<u64>, auto: bool) -> String {
    let option = if auto { "auto" } else { "inline" };
    match feed {
        Some(id) => format!("news.reader.feed.{id}.{option}"),
        None => format!("news.reader.{option}"),
    }
}
pub fn options(feed: Option<u64>) -> Options {
    revision().get();
    read_options(feed)
}
fn preference(default: bool, global: Option<&str>, local: Option<&str>) -> bool {
    local
        .and_then(|s| s.parse().ok())
        .or_else(|| global.and_then(|s| s.parse().ok()))
        .unwrap_or(default)
}
fn read_options(feed: Option<u64>) -> Options {
    let defaults = Options::default();
    let value = |auto, fallback| {
        let global = day::prefs::get(&key(None, auto));
        let local = feed.and_then(|id| day::prefs::get(&key(Some(id), auto)));
        preference(fallback, global.as_deref(), local.as_deref())
    };
    Options {
        inline: value(false, defaults.inline),
        auto: value(true, defaults.auto),
    }
}
#[derive(Clone, Copy)]
pub struct OptionBinding {
    pub feed: Option<u64>,
    pub auto: bool,
}
impl Binding<bool> for OptionBinding {
    fn read(&self) -> bool {
        let options = options(self.feed);
        if self.auto {
            options.auto
        } else {
            options.inline
        }
    }
    fn peek(&self) -> bool {
        let options = read_options(self.feed);
        if self.auto {
            options.auto
        } else {
            options.inline
        }
    }
    fn write(&self, value: bool) {
        day::prefs::set(&key(self.feed, self.auto), &value.to_string());
        let signal = revision();
        signal.set(signal.get_untracked().wrapping_add(1));
    }
}
pub fn controls(feed: Option<u64>) -> impl Piece {
    column((
        labeled(
            crate::res::str::reader_inline_mode(),
            toggle(OptionBinding { feed, auto: false })
                .enabled(|| !crate::site_browser::incognito())
                .id(if feed.is_some() {
                    "feed-inline-reader"
                } else {
                    "inline-reader-picker"
                }),
        ),
        labeled(
            crate::res::str::reader_auto_mode(),
            toggle(OptionBinding { feed, auto: true })
                .enabled(|| !crate::site_browser::incognito())
                .id(if feed.is_some() {
                    "feed-auto-reader"
                } else {
                    "auto-reader-picker"
                }),
        ),
    ))
    .spacing(5.0)
    .grow_w()
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn overrides_inherit_defaults_and_ignore_invalid_persistence() {
        let defaults = Options::default();
        assert!(preference(defaults.inline, None, None));
        assert!(!preference(defaults.auto, None, None));
        assert!(preference(defaults.auto, Some("true"), None));
        assert!(!preference(defaults.auto, Some("true"), Some("false")));
        assert!(preference(defaults.auto, Some("true"), Some("invalid")));
        assert!(preference(
            defaults.inline,
            Some("invalid"),
            Some("invalid")
        ));
        assert!(!preference(defaults.auto, Some("invalid"), Some("invalid")));
    }
}
