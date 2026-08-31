//! Day News — a feed reader built on [Day](https://daybrite.dev), modeled on NetNewsWire.
//!
//! Three panes on a desktop (subscriptions, timeline, article) and a push-navigation stack on a
//! phone, from one `root()`. Everything the UI shows is a reactive signal published by
//! `daynews-core`; the crates underneath own feed parsing, OPML and the SQLite store.

use day::prelude::*;

mod format;
mod menus;
mod reader;
mod settings;
mod subscriptions;
mod theme;
mod timeline;
mod toolbar;

use daynews_db::Scope;

// The mobile / embedded entry point. Expands to the export each platform's shell binds against —
// and to nothing at all on a plain cargo desktop build, where src/main.rs is the entry instead.
// Both entries hand `launch` the same description, so they open the same window.
day::day_start!(options: window(), root);

/// The window every entry point opens — `src/main.rs` on the desktop, the platform shells
/// through the macro above.
///
/// `launch` installs the catalog itself, after the OS's languages have reached day-l10n and
/// before the first localized string is read; installing it here, or in `root`, would resolve
/// against an empty hint list and open an English window on a French device. The same ordering
/// is what lets the TITLE come from the catalog (docs/localization.md).
pub fn window() -> day::WindowOptions {
    day::WindowOptions {
        locales: Some((res::locales::DEFAULT, res::locales::CATALOG)),
        title_fn: Some(|| res::str::app_title().format()),
        // Three panes need room. At 960 the timeline and the article both end up too narrow to
        // read comfortably; this is close to NetNewsWire's own default.
        size: day::prelude::Size::new(1440.0, 900.0),
        min_size: Some(day::prelude::Size::new(720.0, 480.0)),
        ..Default::default()
    }
}

// Typed constants for the files under `resource/`, generated at build time by `day-build`.
day::resources!();

/// The sidebar's selection, as a route key. Smart feeds come first (NetNewsWire's "All Unread"
/// and "Starred"), then one entry per subscription, then the management page.
fn scope_for_key(key: &str) -> Option<Scope> {
    match key {
        "today" => Some(Scope::Today),
        "unread" => Some(Scope::Unread),
        "all" => Some(Scope::All),
        "starred" => Some(Scope::Starred),
        k => {
            if let Some(id) = k
                .strip_prefix("feed:")
                .and_then(|id| id.parse::<u64>().ok())
            {
                Some(Scope::Feed(id))
            } else {
                k.strip_prefix("tag:")
                    .and_then(|id| id.parse::<u64>().ok())
                    .map(Scope::Tag)
            }
        }
    }
}

/// A sidebar count, blank when there is nothing unread — an empty badge draws nothing.
fn count(n: i64) -> String {
    if n > 0 { n.to_string() } else { String::new() }
}

pub fn root() -> impl Piece {
    // Open the store and stand up its live queries before the first build, so the sidebar is
    // populated on the very first frame instead of flashing empty.
    daynews_core::init();
    // The undo history rides the container's change log; the platform bridge gives it ⌘Z,
    // the Edit menu, and the mobile gestures.
    if let Some(stack) = daynews_core::undo_stack() {
        day::install_undo(&stack);
    }
    // Retention: prune per the stored setting (Settings page owns changing it).
    daynews_core::prune(settings::retention_days());
    // Every window shows the same store — the reader is the app, not the window — so a new
    // window is just another shell. Registered once; each window builds its own signals.
    day::register_new_window(build_shell);
    menus::install();
    build_shell()
}

/// The sidebar row a window opens on — NetNewsWire's top smart feed.
const OPENING_SECTION: &str = "today";

/// One window's contents. Called again for each File ▸ New Window.
fn build_shell() -> impl Piece {
    // This window's view and its bar (docs/state.md): `scoped` creates one of each in the
    // window's own scope, so a second window browses its own scope, search and selection while
    // the store, the feeds and the badges below stay shared.
    daynews_core::NewsScene::scoped(|sc| {
        // Each of these belongs to the WINDOW, not to a page: the toolbar outlives any page
        // scope, and File ▸ New Feed focuses the field in the window the user is looking at —
        // so both are provided here, where `focused()` can find them (docs/state.md).
        toolbar::Bar::scoped(move |_bar| {
            subscriptions::UrlFocus::scoped(move |_focus| shell_body(sc))
        })
    })
}

fn shell_body(sc: daynews_core::NewsScene) -> impl Piece {
    let st = daynews_core::state();
    // Per window, so File ▸ New Window gets its own bar (docs/toolbars.md).
    toolbar::install();
    let section: Signal<Option<String>> = Signal::new(Some(OPENING_SECTION.into()));
    // Apply the opening scope by hand: `watch` fires on CHANGE, so without this the sidebar
    // highlights Today while the timeline still shows whatever scope the store opened with.
    if let Some(scope) = scope_for_key(OPENING_SECTION) {
        daynews_core::select_scope(scope);
    }
    // Thereafter the sidebar selection drives the timeline query.
    watch(
        move || section.get(),
        move |key, _| {
            if let Some(scope) = key.as_deref().and_then(scope_for_key) {
                daynews_core::select_scope(scope);
            }
        },
    );

    selector(section)
        .style(SelectorStyle::Sidebar)
        .title(res::str::app_title())
        // Search moved off the toolbar when day replaced `toolbar_search` with `.searchable()`:
        // the selector owns the field now, and the toolkit puts it in the window toolbar on
        // desktop and inline above the list on a phone. The signal is still the shared one the
        // timeline filters on.
        .searchable(toolbar::search())
        .search_prompt(res::str::search_placeholder())
        // The article list is the CONTENT-LIST pane (docs/navigation.md): its own column
        // between the sidebar and the reader where the toolkit has one (a real `contentList`
        // split item on macOS, the supplementary column on iPadOS), the pushed middle layer
        // on a phone, and composed beside the reader elsewhere. Full-page sections keep the
        // whole detail area.
        .content_list(timeline::timeline_pane)
        .content_list_width(400.0)
        .content_list_for(|k: &Option<String>| {
            !matches!(k.as_deref(), Some("subscriptions") | Some("settings"))
        })
        .detail_visible(sc.reader_open)
        // Smart feeds — Today, All Unread and Starred, in NetNewsWire's order, under their own
        // header. Counts are real badges: right-aligned and de-emphasized by the toolkit.
        .section(res::str::nav_smart_feeds())
        .item_icon(
            "today".to_string(),
            res::str::nav_today(),
            res::images::sidebar_today,
            reader_dest,
        )
        // Per-feed identity colors (docs/vectors.md): the sun, the dot, the star and the
        // stack each wear their own tint, NetNewsWire-style.
        .icon_tint(Color::hex(0xFF9F0A))
        .badge(move || count(st.total_today.get()))
        .item_icon(
            "unread".to_string(),
            res::str::nav_all_unread(),
            res::images::sidebar_unread,
            reader_dest,
        )
        .icon_tint(Color::hex(0x0A84FF))
        .badge(move || count(st.total_unread.get()))
        .item_icon(
            "starred".to_string(),
            res::str::nav_starred(),
            res::images::sidebar_starred,
            reader_dest,
        )
        .icon_tint(Color::hex(0xE8940A))
        .badge(move || count(st.total_starred.get()))
        .item_icon(
            "all".to_string(),
            res::str::nav_all_articles(),
            res::images::sidebar_all,
            reader_dest,
        )
        .icon_tint(Color::hex(0x5E5CE6))
        // One row per subscription, re-derived whenever the feed list or its counts change.
        .section(res::str::nav_feeds_section())
        .items(
            move || st.feeds.get(),
            |f: &daynews_core::FeedRow| {
                let name = if f.has_error {
                    format!("⚠ {}", f.title)
                } else {
                    f.title.clone()
                };
                item(format!("feed:{}", f.id), name)
                    .icon(res::images::sidebar_feed)
                    .icon_tint(Color::hex(0x30B0C7))
                    .badge(count(f.unread))
            },
        )
        // User tags, with their article counts — selecting one scopes the timeline to it.
        .section(res::str::nav_tags_section())
        .items(
            move || st.tags.get(),
            |t: &daynews_core::TagRow| {
                item(format!("tag:{}", t.id), t.name.clone())
                    .icon(res::images::sidebar_starred)
                    .icon_tint(Color::hex(0xE8940A))
                    .badge(count(t.count))
            },
        )
        .destination(|key: &Option<String>| match key.as_deref() {
            Some("subscriptions") => {
                Either::Left(Either::Left(subscriptions::subscriptions_page()))
            }
            Some("settings") => Either::Left(Either::Right(settings::settings_page())),
            _ => Either::Right(reader_dest()),
        })
        .item(
            "subscriptions".to_string(),
            res::str::nav_subscriptions(),
            subscriptions::subscriptions_page,
        )
        .item(
            "settings".to_string(),
            res::str::nav_settings(),
            settings::settings_page,
        )
        .id("nav")
}

/// The reader as a destination. The timeline is no longer in here — it is the selector's
/// content-list pane, its own column beside this on the desktops and the pushed middle layer
/// on a phone (docs/navigation.md).
#[cfg(not(target_os = "android"))]
fn reader_dest() -> impl Piece {
    reader::reader_pane().grow()
}

/// Android composes the list-then-reader push flow in the selector, so the reader page carries
/// its own way back (`reader_open` := false); iOS gets the system back chevron from the native
/// stack and macOS shows the reader beside the list, so neither wants the extra row.
#[cfg(target_os = "android")]
fn reader_dest() -> impl Piece {
    column((
        row((
            button(res::str::back())
                .action(|| daynews_core::scene().reader_open.set(false))
                .id("article-back"),
            spacer(),
        ))
        .padding(Insets::symmetric(10.0, 6.0))
        .grow_w(),
        divider(),
        reader::reader_pane().grow(),
    ))
    .grow()
}
