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
pub const ROUTE_SUBSCRIPTIONS: &str = "subscriptions";

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
                        .action(|| {
                            // The subscriptions page owns the URL field; focus follows the user.
                            navigate(ROUTE_SUBSCRIPTIONS);
                            crate::subscriptions::focus_url_field();
                        }),
                    menu_item(res::str::menu_new_folder().format())
                        .shortcut(Shortcut::new("n").shift())
                        .action(|| {
                            navigate(ROUTE_SUBSCRIPTIONS);
                            crate::subscriptions::begin_new_folder();
                        }),
                    // No platform has a native "new window" nav, so this lowers to the
                    // builder registered with `register_new_window` (see `root`).
                    menu_role(MenuRole::NewWindow),
                    menu_separator(),
                    menu_item(res::str::menu_refresh().format())
                        .key("r")
                        .action(daynews_core::refresh_all),
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
                    // ⌘/ — NetNewsWire's shortcut for the single most-used command in a reader.
                    menu_item(res::str::menu_next_unread().format())
                        .key("/")
                        .action(|| {
                            daynews_core::open_next_unread();
                        }),
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
                    menu_item(res::str::menu_refresh_feed().format())
                        .shortcut(Shortcut::new("r").shift())
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
                        if let Some(id) = daynews_core::scene().selected.get_untracked() {
                            daynews_core::toggle_read(id);
                        }
                    }),
                    menu_item(res::str::mark_all_read().format())
                        .key("k")
                        .action(|| daynews_core::mark_scope_read(true)),
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
                            if let Some(id) = daynews_core::scene().selected.get_untracked() {
                                crate::timeline::begin_tag(id);
                            }
                        }),
                    menu_separator(),
                    menu_item(res::str::menu_open_in_browser().format())
                        .shortcut(Shortcut::new("Return"))
                        .action(open_in_browser),
                ],
            ),
        ]
    });
}

/// Read/star the article the reader currently shows. No open article means nothing to do —
/// the commands stay harmless rather than acting on some other row.
fn set_open_read(read: bool) {
    if let Some(id) = daynews_core::scene().selected.get_untracked() {
        daynews_core::set_read(id, read);
    }
}

fn set_open_starred(starred: bool) {
    if let Some(id) = daynews_core::scene().selected.get_untracked() {
        daynews_core::set_starred(id, starred);
    }
}

/// Hand the article's own link to the platform browser.
fn open_in_browser() {
    if let Some(url) = daynews_core::scene()
        .article
        .get_untracked()
        .and_then(|a| a.url.clone())
    {
        open_url(&url);
    }
}

/// Move both the sidebar selection and the timeline filter. `navigate` alone would move the
/// nav; the scope watch in `root` picks it up, but setting it here too means the menu works
/// even before the nav has mounted.
fn go(route: &str, scope: Scope) {
    navigate(route);
    daynews_core::select_scope(scope);
}

/// Run `f` on the feed the sidebar has selected. A smart feed, a tag or a page selected instead
/// leaves no feed to act on, and the command does nothing.
fn with_selected_feed(f: impl FnOnce(u64)) {
    if let Scope::Feed(feed) = daynews_core::scene().scope.get_untracked() {
        f(feed);
    }
}

/// A sidebar feed row's context menu: the Feed menu's three commands, aimed at the row that was
/// right-clicked or long-pressed rather than at the selection (docs/menus.md).
pub fn feed_context_menu(feed: u64, unread: i64) -> Vec<MenuEntry> {
    vec![
        menu_item(res::str::refresh_action().format())
            .action(move || daynews_core::refresh_feed(feed)),
        menu_item(res::str::mark_all_read().format())
            .enabled(unread > 0)
            .action(move || daynews_core::mark_feed_read(feed, true)),
        menu_separator(),
        menu_item(res::str::unsubscribe().format()).action(move || daynews_core::unsubscribe(feed)),
    ]
}
