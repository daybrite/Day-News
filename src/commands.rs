//! Reader operations shared by native chrome and content fallbacks.
use crate::res;
use day::prelude::*;

pub(crate) fn refresh() -> CommandHandle {
    Command {
        id: "refresh",
        label: res::str::refresh_action(),
        action: daynews_core::refresh_all,
    }
    .build()
    .icon(Symbol::Refresh)
    .shortcut(Shortcut::new("r"))
    .enabled(|| {
        let st = daynews_core::state();
        st.refresh_progress.get().is_none() && st.feeds.with(|feeds| !feeds.is_empty())
    })
}

pub(crate) fn mark_all_read() -> CommandHandle {
    Command {
        id: "mark-all-read",
        label: res::str::mark_all_read(),
        action: || {
            if daynews_core::try_scene().is_some() {
                daynews_core::mark_scope_read(true);
            }
        },
    }
    .build()
    .icon(Symbol::Check)
    .enabled(|| daynews_core::try_scene().is_some_and(|sc| sc.scope_unread.get() > 0))
}

/// Shared by the menu and native toolbars, including before any article is open.
pub(crate) fn next_unread() -> CommandHandle {
    Command {
        id: "next-unread",
        label: res::str::menu_next_unread(),
        action: || {
            let Some(sc) = daynews_core::try_scene() else {
                return;
            };
            let before = sc.scope.get_untracked();
            if daynews_core::open_next_unread() {
                if before != sc.scope.get_untracked() {
                    navigate(crate::menus::ROUTE_UNREAD);
                }
                if cfg!(feature = "appkit") {
                    sc.timeline_focused.set(true);
                }
            }
        },
    }
    .build()
    .icon(Symbol::Down)
    .shortcut(Shortcut::new("/"))
    .enabled(|| {
        daynews_core::try_scene().is_some_and(|sc| {
            let current_unread = sc.article.with(|a| a.as_ref().is_some_and(|a| !a.is_read));
            daynews_core::state().total_unread.get() > i64::from(current_unread)
        })
    })
}

pub(crate) fn open_in_browser() -> CommandHandle {
    Command {
        id: "open-in-browser",
        label: res::str::menu_open_in_browser(),
        action: || {
            if let Some(url) = daynews_core::try_scene()
                .and_then(|sc| sc.article.with(|a| a.as_ref().and_then(|a| a.url.clone())))
            {
                crate::settings::open_link(&url);
            }
        },
    }
    .build()
    .image(res::vectors::open_browser)
    .shortcut(Shortcut::new("Return"))
    .enabled(|| {
        daynews_core::try_scene().is_some_and(|sc| {
            sc.article.with(|a| {
                a.as_ref()
                    .and_then(|a| a.url.as_ref())
                    .is_some_and(|url| !url.is_empty())
            })
        })
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn article_commands_are_disabled_before_a_window_exists() {
        assert!(!super::open_in_browser().is_enabled());
        assert!(!super::mark_all_read().is_enabled());
        assert!(!super::next_unread().is_enabled());
    }
}
