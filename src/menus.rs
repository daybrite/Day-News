//! The desktop menu bar (File, Go, Feed and Article, modeled on NetNewsWire's) and the sidebar
//! feed rows' context menu.
//!
//! The menu bar is installed only where the backend has one (macOS, GTK, Qt, XAML); on a phone
//! the capability is absent and `app_menu` is a no-op, so the same call is safe everywhere. The
//! row menus show wherever the backend has per-row menus (docs/menus.md).

use crate::res;
use day::prelude::*;
use daynews_db::Scope;

/// Route keys the sidebar uses, so a menu command can move the selection the same way a click
/// does (deep links and dayscript address these same keys).
pub const ROUTE_TODAY: &str = "today";
pub const ROUTE_UNREAD: &str = "unread";
pub const ROUTE_STARRED: &str = "starred";

/// Install the app menu. Reactive so the Go menu's enablement follows the unread count.
pub fn install() {
    app_menu_reactive(|| {
        vec![
            // Claim the File slot: day fills the standard slots an app leaves open (Edit, View,
            // Window, Help) and puts this one where File belongs.
            sub_menu(
                res::str::menu_file().format(),
                vec![
                    menu_item(res::str::menu_new_feed().format())
                        .key("n")
                        .action(crate::subscriptions::begin_new_feed),
                    menu_item(res::str::menu_new_folder().format())
                        .shortcut(Shortcut::new("n").shift())
                        .action(crate::subscriptions::begin_new_folder),
                    // No platform has a native "new window" nav, so this lowers to the
                    // builder registered with `register_new_window` (see `root`).
                    menu_role(MenuRole::NewWindow),
                    menu_separator(),
                    crate::commands::refresh().menu_item(),
                    menu_separator(),
                    menu_item(res::str::menu_import().format())
                        .shortcut(Shortcut::new("i").shift())
                        .action(crate::subscriptions::import_opml),
                    menu_item(res::str::menu_export().format())
                        .shortcut(Shortcut::new("e").shift())
                        .action(crate::subscriptions::export_opml),
                    menu_separator(),
                    menu_role(MenuRole::CloseWindow),
                ],
            )
            .bar_role(MenuBarRole::File),
            sub_menu(
                res::str::menu_go().format(),
                vec![
                    crate::commands::navigate_article(false).menu_item(),
                    crate::commands::navigate_article(true).menu_item(),
                    crate::commands::next_unread().menu_item(),
                    menu_separator(),
                    menu_item(res::str::nav_today().format())
                        .key("1")
                        .action(|| go(ROUTE_TODAY, Scope::Today)),
                    menu_item(res::str::nav_all_unread().format())
                        .key("2")
                        .action(|| go(ROUTE_UNREAD, Scope::Unread)),
                    menu_item(res::str::nav_starred().format())
                        .key("3")
                        .action(|| go(ROUTE_STARRED, Scope::Starred)),
                ],
            ),
            // The feed the sidebar has selected: the same three commands its row's context menu
            // offers. With no feed selected they do nothing, like the Article commands with no
            // open article.
            sub_menu(
                res::str::menu_feed().format(),
                vec![
                    crate::commands::unread_feeds_only().menu_item(),
                    crate::commands::group_by_feeds().menu_item(),
                    menu_separator(),
                    menu_item(res::str::menu_refresh_feed().format())
                        .action(|| with_selected_feed(daynews_core::refresh_feed)),
                    menu_item(res::str::mark_all_read().format()).action(|| {
                        with_selected_feed(|feed| daynews_core::mark_feed_read(feed, true))
                    }),
                    menu_separator(),
                    menu_item(res::str::unsubscribe().format())
                        .action(|| with_selected_feed(daynews_core::unsubscribe)),
                ],
            ),
            sub_menu(
                res::str::menu_article().format(),
                vec![
                    // NetNewsWire's Article menu and its shortcuts, read off its own menu bar.
                    menu_item(res::str::menu_mark_read().format())
                        .shortcut(Shortcut::new("u").shift())
                        .action(|| set_open_read(true)),
                    menu_item(res::str::menu_mark_unread().format())
                        .key("u")
                        .action(|| set_open_read(false)),
                    menu_item(res::str::toggle_read().format()).action(|| {
                        if let Some(id) =
                            daynews_core::try_scene().and_then(|sc| sc.selected.get_untracked())
                        {
                            daynews_core::toggle_read(id);
                        }
                    }),
                    crate::commands::mark_all_read().menu_item(),
                    menu_separator(),
                    menu_item(res::str::menu_star().format())
                        .shortcut(Shortcut::new("l").shift())
                        .action(|| set_open_starred(true)),
                    menu_item(res::str::menu_unstar().format())
                        .key("l")
                        .action(|| set_open_starred(false)),
                    menu_item(res::str::menu_tag().format())
                        .key("t")
                        .action(|| {
                            if let Some(id) =
                                daynews_core::try_scene().and_then(|sc| sc.selected.get_untracked())
                            {
                                crate::timeline::begin_tag(id);
                            }
                        }),
                    menu_separator(),
                    crate::commands::open_in_browser().menu_item(),
                    crate::commands::copy_article_link().menu_item(),
                    crate::commands::reader_view().menu_item(),
                    crate::commands::find().menu_item(),
                    menu_separator(),
                    menu_item(res::str::menu_increase_text_size().format())
                        .id("reader-increase-text-size")
                        .key("+")
                        .enabled(
                            crate::reader_styles::state().get().scale
                                < crate::reader_styles::MAX_SCALE,
                        )
                        .action(|| crate::reader_styles::adjust_size(10.0)),
                    menu_item(res::str::menu_decrease_text_size().format())
                        .id("reader-decrease-text-size")
                        .key("-")
                        .enabled(
                            crate::reader_styles::state().get().scale
                                > crate::reader_styles::MIN_SCALE,
                        )
                        .action(|| crate::reader_styles::adjust_size(-10.0)),
                ],
            ),
        ]
    });
}

/// Read/star the article the reader currently shows. No open article means nothing to do:
/// the commands stay harmless rather than acting on some other row.
fn set_open_read(read: bool) {
    if let Some(id) = daynews_core::try_scene().and_then(|sc| sc.selected.get_untracked()) {
        daynews_core::set_read(id, read);
    }
}

fn set_open_starred(starred: bool) {
    if let Some(id) = daynews_core::try_scene().and_then(|sc| sc.selected.get_untracked()) {
        daynews_core::set_starred(id, starred);
    }
}

/// Move both the sidebar selection and the timeline filter. `navigate` alone would move the
/// nav; the scope watch in `root` picks it up. Preferences and a closed last window have
/// no news scene, so their global menu callbacks must tolerate that state.
fn go(route: &str, scope: Scope) {
    if daynews_core::try_scene().is_some() {
        navigate(route);
        daynews_core::select_scope(scope);
    }
}

/// Run `f` on the feed the sidebar has selected. A smart feed, a tag or a page selected instead
/// leaves no feed to act on, and the command does nothing.
fn with_selected_feed(f: impl FnOnce(u64)) {
    if let Some(Scope::Feed(feed)) = daynews_core::try_scene().map(|sc| sc.scope.get_untracked()) {
        f(feed);
    }
}

/// A sidebar feed row's context menu: the Feed menu's three commands, aimed at the row that was
/// right-clicked or long-pressed rather than at the selection (docs/menus.md).
pub fn feed_context_menu(feed: u64, unread: i64) -> Vec<MenuEntry> {
    vec![
        menu_item(res::str::dashboard_show().format())
            .id("feed-dashboard")
            .action(move || crate::site_browser::show_feed_dashboard(feed)),
        menu_item(res::str::refresh_action().format())
            .action(move || daynews_core::refresh_feed(feed)),
        menu_item(res::str::mark_all_read().format())
            .enabled(unread > 0)
            .action(move || daynews_core::mark_feed_read(feed, true)),
        menu_separator(),
        menu_item(res::str::move_feed_up().format())
            .id("feed-move-up")
            .action(move || crate::feed_list::move_relative(feed, -1)),
        menu_item(res::str::move_feed_down().format())
            .id("feed-move-down")
            .action(move || crate::feed_list::move_relative(feed, 1)),
        menu_item(res::str::unsubscribe().format()).action(move || daynews_core::unsubscribe(feed)),
    ]
}

#[cfg(test)]
mod tests {
    #[test]
    fn navigation_commands_tolerate_no_news_window() {
        super::go(super::ROUTE_UNREAD, daynews_db::Scope::Unread);
        super::set_open_read(true);
        super::set_open_starred(true);
        crate::subscriptions::begin_new_feed();
    }
}
