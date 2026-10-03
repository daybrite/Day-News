//! The window's toolbar items and the search signal they share (docs/toolbars.md).
//!
//! Feed commands stay with navigation; article commands stay with the reader. Next Unread
//! is available before selection and moves into the article toolbar once reading starts.

use crate::res;
use day::prelude::*;

/// Search has one source of truth, shared by native chrome and the timeline query.
pub fn search() -> Signal<String> {
    daynews_core::scene().search
}

/// Whether this toolkit puts commands in a window bar. Where it does not, the reader's commands
/// still appear (a contribution always lands on some chrome), but the timeline carries the
/// search field itself rather than handing it to a window toolbar.
pub fn available() -> bool {
    capability(Cap::Toolbar) != Support::Unsupported
}

/// The feed list's commands: they act on the scope the sidebar has chosen, which is what the
/// user is looking at while the list is in front of them.
pub fn feed_items() -> Vec<ToolbarEntry> {
    let sc = daynews_core::scene();
    // A phone's narrow reader bar prioritizes reading actions. Sidebar commands
    // remain one native Back away on the timeline.
    let compact = day::size_class().is_none_or(|size| size.width == WidthClass::Compact);
    if cfg!(target_os = "ios") && compact && sc.reader_open.get() {
        return Vec::new();
    }
    let mut items = vec![
        toolbar_button("new-feed", res::str::menu_new_feed())
            .icon(Symbol::Add)
            .action(crate::subscriptions::begin_new_feed)
            .placement(ToolbarPlacement::Secondary),
        crate::commands::refresh()
            .toolbar_item()
            .placement(ToolbarPlacement::Navigation),
        crate::commands::unread_feeds_only()
            .toolbar_item()
            .placement(ToolbarPlacement::Navigation),
    ];
    if sc.article.with(|a| a.is_none()) {
        items.push(
            crate::commands::dashboard()
                .toolbar_item()
                .placement(ToolbarPlacement::Secondary),
        );
        items.push(crate::commands::next_unread().toolbar_item());
    }
    items
}

/// The open article's commands. Declared on the reader, so they arrive with the article and
/// leave with it, and each one reads the article it was built for rather than a mirror of it.
pub fn article_items(article: daynews_core::StoredArticle) -> Vec<ToolbarEntry> {
    let (id, starred, read) = (article.id, article.is_starred, article.is_read);
    let star_state = Signal::new(starred);
    let read_state = Signal::new(read);
    vec![
        crate::commands::navigate_article(false).toolbar_item(),
        crate::commands::navigate_article(true).toolbar_item(),
        crate::commands::next_unread().toolbar_item(),
        toolbar_toggle(
            "star",
            if starred {
                res::str::unstar()
            } else {
                res::str::star()
            },
            star_state,
        )
        .icon(Symbol::Star)
        .action(move || daynews_core::set_starred(id, star_state.get_untracked())),
        toolbar_toggle(
            "read",
            if read {
                res::str::mark_unread()
            } else {
                res::str::mark_read()
            },
            read_state,
        )
        .icon(Symbol::CircleFilled)
        .placement(ToolbarPlacement::Primary)
        .action(move || daynews_core::set_read(id, read_state.get_untracked())),
        crate::commands::open_in_browser().toolbar_item(),
        crate::commands::reader_view().toolbar_item(),
        crate::commands::dashboard()
            .toolbar_item()
            .placement(ToolbarPlacement::Secondary),
        crate::commands::copy_article_link()
            .toolbar_item()
            .placement(ToolbarPlacement::Secondary),
    ]
}

/// Grouping belongs above the article list; feed filtering stays beside Refresh in the sidebar.
pub fn list_items() -> Vec<ToolbarEntry> {
    if cfg!(feature = "appkit") {
        vec![crate::commands::group_by_feeds().toolbar_item()]
    } else {
        Vec::new()
    }
}
