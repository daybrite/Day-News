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
    // A phone's narrow reader bar prioritizes reading actions. Refresh and bulk marking
    // remain one native Back away on the timeline.
    let compact = day::size_class().is_none_or(|size| size.width == WidthClass::Compact);
    if cfg!(target_os = "ios") && compact && sc.reader_open.get() {
        return Vec::new();
    }
    let mut items = vec![
        crate::commands::refresh().toolbar_item(),
        crate::commands::mark_all_read().toolbar_item(),
    ];
    if sc.article.with(|a| a.is_none()) {
        items.push(crate::commands::next_unread().toolbar_item());
    }
    items
}

/// The open article's commands. Declared on the reader, so they arrive with the article and
/// leave with it, and each one reads the article it was built for rather than a mirror of it.
pub fn article_items(article: daynews_core::StoredArticle) -> Vec<ToolbarEntry> {
    let (id, starred, read) = (article.id, article.is_starred, article.is_read);
    vec![
        crate::commands::next_unread().toolbar_item(),
        toolbar_toggle(
            "star",
            if starred {
                res::str::unstar()
            } else {
                res::str::star()
            },
            Signal::new(starred),
        )
        .icon(Symbol::Star)
        .action(move || daynews_core::set_starred(id, !starred)),
        toolbar_toggle(
            "read",
            if read {
                res::str::mark_unread()
            } else {
                res::str::mark_read()
            },
            Signal::new(read),
        )
        .icon(Symbol::CircleFilled)
        .placement(ToolbarPlacement::Primary)
        .action(move || daynews_core::set_read(id, !read)),
        crate::commands::open_in_browser().toolbar_item(),
    ]
}
