//! The window's toolbar items and the search signal they share (docs/toolbars.md).
//!
//! Modeled on NetNewsWire's. The commands split three ways by what they act on, and each is
//! declared where that thing is: refresh and mark-all-read on the feed list (`lib.rs`'s
//! nav), next-unread and the star and read toggles on the article (`reader.rs`), and search
//! on the surface it filters.
//!
//! What used to be here as well was the bookkeeping that a window-wide bar needed: two mirror
//! signals and two `watch`es copying the open article's starred and read state into them, so the
//! bar could read what it could not see. The reader has the article in hand, so none of it is
//! needed any more.

use crate::res;
use day::prelude::*;

/// What this window's toolbar shows, per window (docs/state.md), like everything else about
/// what a window is looking at. The search text is shared with that window's timeline field on
/// a phone.
///
/// Owned by the window's scope through `Ambient::scoped`, which is what the old `Signal::global`
/// was standing in for: the field outlives any page scope, so a page-owned signal would be
/// disposed under it the first time the reader navigated.
#[derive(Clone, Copy)]
pub struct Bar {
    pub search: Signal<String>,
}

impl Ambient for Bar {
    fn create() -> Self {
        Bar {
            search: Signal::new(String::new()),
        }
    }
}

/// This window's bar state: ambient while a piece builds, the focused window's from a handler.
fn bar() -> Bar {
    Bar::try_ambient()
        .or_else(Bar::focused)
        .expect("no window is open, so there is no toolbar state to act on")
}

pub fn search() -> Signal<String> {
    bar().search
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
    let st = daynews_core::state();
    vec![
        toolbar_button("refresh", res::str::refresh_action())
            .icon(Symbol::Refresh)
            .action(daynews_core::refresh_all),
        toolbar_button("mark-all-read", res::str::mark_all_read())
            .icon(Symbol::Check)
            .action(|| daynews_core::mark_scope_read(true))
            .enabled_when(move || st.total_unread.get() > 0),
    ]
}

/// The open article's commands. Declared on the reader, so they arrive with the article and
/// leave with it, and each one reads the article it was built for rather than a mirror of it.
pub fn article_items(article: daynews_core::StoredArticle) -> Vec<ToolbarEntry> {
    let st = daynews_core::state();
    let (id, starred, read) = (article.id, article.is_starred, article.is_read);
    vec![
        toolbar_button("next-unread", res::str::menu_next_unread())
            .icon(Symbol::Down)
            .enabled_when(move || st.total_unread.get() > 0)
            .action(|| {
                daynews_core::open_next_unread();
            }),
        toolbar_toggle("star", res::str::menu_star(), Signal::new(starred))
            .icon(Symbol::Star)
            .action(move || daynews_core::set_starred(id, !starred)),
        toolbar_toggle("read", res::str::toggle_read(), Signal::new(read))
            .icon(Symbol::CircleFilled)
            .placement(ToolbarPlacement::Primary)
            .action(move || daynews_core::set_read(id, !read)),
    ]
}
