//! The window toolbar (docs/toolbars.md), modeled on NetNewsWire's: refresh and mark-all-read
//! at the leading edge, next-unread and the star toggle after them, and search pinned to the
//! trailing edge.
//!
//! Installed per window, so File ▸ New Window gets its own bar. Where the toolkit has no
//! toolbar — every phone — nothing installs and the timeline keeps its own search field
//! instead (see `timeline::timeline_pane`).

use crate::res;
use day::prelude::*;
use std::cell::OnceCell;

thread_local! {
    /// The search text, shared by the toolbar's search field and (on a phone) the timeline's.
    /// `global`, NOT `new`: the toolbar outlives any page scope, so a scope-owned signal would
    /// be disposed under it the first time the reader navigated.
    static SEARCH: OnceCell<Signal<String>> = const { OnceCell::new() };
    /// Whether the open article is starred — what the toolbar's star toggle shows.
    static STARRED: OnceCell<Signal<bool>> = const { OnceCell::new() };
    /// Whether the open article is read — what the toolbar's read toggle shows.
    static READ: OnceCell<Signal<bool>> = const { OnceCell::new() };
}

pub fn search() -> Signal<String> {
    SEARCH.with(|c| *c.get_or_init(|| Signal::global(String::new())))
}

fn starred() -> Signal<bool> {
    STARRED.with(|c| *c.get_or_init(|| Signal::global(false)))
}

fn read() -> Signal<bool> {
    READ.with(|c| *c.get_or_init(|| Signal::global(false)))
}

/// Does this toolkit put commands in a bar? Where it does not, the reader's commands have to
/// live in the content instead — there is no drawn stand-in.
///
/// `!= Unsupported`, not `== Native` (docs/toolbars.md): web-dom answers `Emulated` — a strip
/// the shim docks above the app root, with working buttons, toggles and menus — and gating on
/// `Native` hid the web build's toolbar entirely, pushing Refresh and Mark All as Read into
/// the timeline as if a browser were a phone.
pub fn available() -> bool {
    capability(Cap::Toolbar) != Support::Unsupported
}

/// Install the window's toolbar. Called once per window, from that window's builder.
pub fn install() {
    if !available() {
        return;
    }
    let st = daynews_core::state();
    let search = search();
    let starred = starred();
    let read = read();

    // Typing filters through the full-text index; the store quotes the text so punctuation is
    // searched for rather than parsed as query syntax.
    watch(move || search.get(), |q, _| daynews_core::set_search(q));
    // The star toggle shows the OPEN article's state, so it has to follow the selection as
    // well as the reader's own clicks.
    watch(
        move || st.article.get().map(|a| a.is_starred).unwrap_or(false),
        move |on, _| starred.set(*on),
    );
    // The read toggle likewise follows the open article — a swipe or menu toggle elsewhere
    // repaints this button without it doing anything.
    watch(
        move || st.article.get().map(|a| a.is_read).unwrap_or(false),
        move |on, _| read.set(*on),
    );

    // Reactive so the labels follow a runtime language change; the values that change often
    // (the search text, the star state, what is enabled) ride their own bindings instead, so
    // none of them rebuilds the bar.
    toolbar_reactive(move || {
        vec![
            // First, before anything else — where every desktop expects the sidebar control
            // (docs/toolbars.md). The behavior is the toolkit's own: it drives the window's
            // `selector(Sidebar)` collapse, so no action and no icon here.
            toolbar_sidebar_toggle("toggle-sidebar", res::str::toggle_sidebar()),
            toolbar_button("refresh", res::str::refresh_action())
                .icon(Symbol::Refresh)
                .action(daynews_core::refresh_all),
            toolbar_button("mark-all-read", res::str::mark_all_read())
                .icon(Symbol::Check)
                .action(|| daynews_core::mark_scope_read(true))
                .enabled_when(move || st.total_unread.get() > 0),
            toolbar_separator(),
            toolbar_button("next-unread", res::str::menu_next_unread())
                .icon(Symbol::Down)
                .action(|| {
                    daynews_core::open_next_unread();
                })
                .enabled_when(move || st.total_unread.get() > 0),
            toolbar_toggle("star", res::str::menu_star(), starred)
                .icon(Symbol::Star)
                .enabled_when(move || st.selected.get().is_some())
                // The signal is already set when this runs, so it is the new state.
                .action(move || {
                    if let Some(id) = st.selected.get_untracked() {
                        daynews_core::set_starred(id, starred.get_untracked());
                    }
                }),
            toolbar_toggle("read", res::str::toggle_read(), read)
                .icon(Symbol::CircleFilled)
                .enabled_when(move || st.selected.get().is_some())
                // Same shape as the star: the signal already holds the new state.
                .action(move || {
                    if let Some(id) = st.selected.get_untracked() {
                        daynews_core::set_read(id, read.get_untracked());
                    }
                }),
            toolbar_flexible_space(),
        ]
    });
}
