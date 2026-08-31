//! The article list: NetNewsWire's middle pane — title, summary, and a feed·date footer, with
//! an unread dot in the left gutter.

use crate::format::{relative_time, snippet};
use crate::theme::palette;
use day::prelude::*;
use daynews_core::ArticleSummary;
use daynews_db::Scope;

/// The left gutter the unread dot lives in. Wide enough for the dot plus breathing room, and
/// applied to read rows too so every title starts on the same x.
const GUTTER: f64 = 18.0;
const DOT: f64 = 8.0;

/// The row's type ramp, in Day's semantic steps so it follows the reader's accessibility text
/// size: the headline, its summary, then the feed·date footer.
const TITLE_FONT: Font = Font::Body;
const SUMMARY_FONT: Font = Font::Footnote;
const FOOTER_FONT: Font = Font::Caption;

/// The list's uniform row pitch: a one-line title, up to two summary lines, and the footer,
/// plus the row's vertical padding. Uniform because the native hosts size `Automatic` rows at
/// a fixed default today (docs/list.md) — and a fixed pitch is the Mail/NetNewsWire idiom
/// anyway. A title that wraps borrows the summary's space; content past the pitch clips.
const ROW_H: f64 = 88.0;

/// How much summary the row shows. Two footnote lines at the pane's width — trimmed here so
/// an overlong summary doesn't push the footer past the fixed row pitch.
const SUMMARY_CHARS: usize = 110;

/// One timeline row, bound to its slot.
///
/// Every varying field — the ID INCLUDED — is read INSIDE a reactive closure. The native list
/// recycles cells: a scrolled-away row's cell is rebound to a different article by one slot
/// write, so anything captured eagerly freezes at the value the cell was BORN with, not the
/// article it now shows.
fn row_for(slot: ItemSlot<ArticleSummary, String>) -> impl Piece {
    let sc = daynews_core::scene();
    let id = move || slot.field(|a| a.id);
    let read = move || slot.field(|a| a.is_read);

    // Selection is the native list's to draw (docs/list.md): the platform highlight tracks
    // the table's own focus the way Mail's does. Rows keep their content colors.
    let title_color = move || {
        if read() {
            palette().text_muted
        } else {
            palette().text
        }
    };
    let sub_color = move || palette().text_muted;

    row((
        // The dot rides at the title's optical center rather than the row's: a three-line row
        // would otherwise float it down beside the summary. The gutter keeps its width whether
        // or not a dot is drawn, so every title starts on the same x.
        column((
            column(()).height(5.0),
            when(
                move || !read(),
                move || {
                    // A drawn shape, not an empty container with a background: a childless
                    // container has no content to paint on GTK/Qt.
                    circle()
                        .fill(move || palette().unread_dot)
                        .frame(DOT, DOT)
                        .id_of(move || format!("unread-dot-{}", id()))
                },
            ),
        ))
        .width(GUTTER),
        column((
            label(move || {
                slot.field(|a| a.title.clone())
                    .unwrap_or_else(|| crate::res::str::untitled().format())
            })
            .font(TITLE_FONT)
            .weight(FontWeight::Semibold)
            .color(title_color),
            when(
                move || slot.field(|a| a.summary.is_some()),
                move || {
                    label(move || {
                        slot.field(|a| snippet(a.summary.as_deref().unwrap_or(""), SUMMARY_CHARS))
                    })
                    .font(SUMMARY_FONT)
                    .color(sub_color)
                },
            ),
            row((
                label(move || slot.field(|a| a.feed_title.clone()))
                    .font(FOOTER_FONT)
                    .weight(FontWeight::Medium)
                    .color(sub_color)
                    .grow_w(),
                label(move || slot.field(|a| relative_time(a.published_at)))
                    .font(FOOTER_FONT)
                    .color(sub_color),
            ))
            .spacing(8.0)
            .grow_w(),
        ))
        .spacing(3.0)
        .align(HAlign::Leading)
        .grow_w(),
        when(
            move || slot.field(|a| a.is_starred),
            move || {
                label("\u{2605}")
                    .font(FOOTER_FONT)
                    .color(move || palette().star)
            },
        ),
    ))
    .spacing(6.0)
    .align(VAlign::Top)
    .padding(Insets::symmetric(12.0, 8.0))
    // Taps are the native table's now (they select, and selection opens — see
    // `timeline_pane`), so the menu is the row's only gesture of its own.
    .context_menu(vec![
        menu_item(crate::res::str::mark_read().format())
            .action(move || daynews_core::set_read(id(), true)),
        menu_item(crate::res::str::mark_unread().format())
            .action(move || daynews_core::set_read(id(), false)),
        menu_item(crate::res::str::star().format())
            .action(move || daynews_core::set_starred(id(), true)),
        menu_item(crate::res::str::unstar().format())
            .action(move || daynews_core::set_starred(id(), false)),
        menu_item(crate::res::str::tag_action().format()).action(move || begin_tag(id())),
    ])
    // The POSITIONAL id: a script addresses "the first row" without knowing which article
    // the network delivered. Reactive, so a recycled cell re-labels as it rebinds.
    // Separation between rows is the LIST's (`.separators(true)` in `timeline_pane`), drawn
    // by the host at the row boundary — nothing else wraps the row.
    .id_of(move || {
        let id = id();
        let pos = sc
            .articles
            .with(|a| a.iter().position(|x| x.id == id))
            .unwrap_or(usize::MAX);
        format!("article-row-{pos}")
    })
    .grow_w()
}

/// Prompt for a tag name and toggle it on the article — creating the tag on first use.
pub(crate) fn begin_tag(article: u64) {
    day::task(async move {
        if let Some(name) = prompt(crate::res::str::tag_prompt_title())
            .placeholder(crate::res::str::tag_prompt_placeholder().format())
            .await
        {
            daynews_core::toggle_tag(article, &name);
        }
    });
}

/// The heading over the list: which scope is showing, and how much of it is unread.
fn scope_title() -> String {
    let st = daynews_core::state();
    let sc = daynews_core::scene();
    match sc.scope.get() {
        Scope::Today => crate::res::str::nav_today().format(),
        Scope::Unread => crate::res::str::nav_all_unread().format(),
        Scope::Starred => crate::res::str::nav_starred().format(),
        Scope::All => crate::res::str::nav_all_articles().format(),
        Scope::Feed(id) => st
            .feeds
            .with(|f| f.iter().find(|r| r.id == id).map(|r| r.title.clone()))
            .unwrap_or_default(),
        Scope::Folder(id) => st
            .folders
            .with(|f| f.iter().find(|r| r.id == id).map(|r| r.name.clone()))
            .unwrap_or_default(),
        Scope::Tag(id) => st
            .tags
            .with(|t| t.iter().find(|r| r.id == id).map(|r| r.name.clone()))
            .unwrap_or_default(),
    }
}

pub fn timeline_pane() -> impl Piece {
    let st = daynews_core::state();
    let sc = daynews_core::scene();
    // Search lives in the window toolbar where there is one; a phone has none, so the timeline
    // carries the field itself there. Both write the same signal.
    let search = crate::toolbar::search();
    let in_toolbar = crate::toolbar::available();
    if !in_toolbar {
        watch(move || search.get(), |q, _| daynews_core::set_search(q));
    }

    column((
        // Heading — the scope's name over its unread count, with the two actions a reader
        // reaches for pinned right. NetNewsWire's shape, and it tells you where you are, which
        // a bare row of controls did not.
        row((
            column((
                label(scope_title)
                    .font(Font::Title3)
                    .weight(FontWeight::Bold)
                    .color(move || palette().text)
                    .id("scope-title"),
                label(move || {
                    let n = sc
                        .articles
                        .with(|a| a.iter().filter(|x| !x.is_read).count());
                    crate::res::str::unread_count(n as f64).format()
                })
                .font(Font::Caption)
                .color(move || palette().text_muted),
            ))
            .spacing(1.0)
            .align(HAlign::Leading)
            .grow_w(),
            // Both commands are toolbar items where there is a toolbar; repeating them in the
            // content there would just be two ways to press the same button.
            when(
                move || !in_toolbar,
                || {
                    row((
                        button(crate::res::str::refresh_action())
                            .action(daynews_core::refresh_all)
                            .id("refresh"),
                        button(crate::res::str::mark_all_read())
                            .action(|| daynews_core::mark_scope_read(true))
                            .id("mark-all-read"),
                    ))
                    .spacing(8.0)
                },
            ),
        ))
        .spacing(8.0)
        .align(VAlign::Center)
        .padding(Insets {
            top: 10.0,
            leading: 12.0,
            bottom: 8.0,
            trailing: 12.0,
        })
        .grow_w(),
        when(
            move || !in_toolbar,
            move || {
                row((text_field(search)
                    .placeholder(crate::res::str::search_placeholder())
                    .id("search")
                    .grow_w(),))
                .padding(Insets {
                    top: 0.0,
                    leading: 12.0,
                    bottom: 10.0,
                    trailing: 12.0,
                })
                .grow_w()
            },
        ),
        // Refresh progress, shown only while a refresh is running.
        when(
            move || st.refresh_progress.get().is_some(),
            move || {
                let (done, total) = st.refresh_progress.get_untracked().unwrap_or((0, 0));
                row((
                    spinner(),
                    label(crate::res::str::refresh_progress(done as f64, total as f64))
                        .font(Font::Caption)
                        .color(move || palette().text_muted)
                        .id("refresh-progress"),
                ))
                .spacing(8.0)
                .align(VAlign::Center)
                .padding(Insets::symmetric(12.0, 4.0))
                .grow_w()
            },
        ),
        divider(),
        when(
            move || sc.articles.with(|a| a.is_empty()),
            move || {
                column((
                    spacer(),
                    // An empty unread scope is an achievement, not an absence.
                    label(move || {
                        if sc.scope.get() == Scope::Unread {
                            crate::res::str::timeline_empty_unread().format()
                        } else {
                            crate::res::str::timeline_empty().format()
                        }
                    })
                    .font(Font::Body)
                    .color(move || palette().text_muted)
                    .id("timeline-empty"),
                    spacer(),
                ))
                .align(HAlign::Center)
                .grow()
            },
        ),
        {
            // Programmatic selection moves (Next Unread, a scripted `select:`) can land
            // anywhere in the window — follow them so the selected row is visible. `watch`
            // never fires for the initial run, so building the pane doesn't force a scroll.
            let jump: Signal<Option<usize>> = Signal::new(None);
            watch(
                move || {
                    let sel = sc.selected.get();
                    sc.articles
                        .with(|a| sel.and_then(|id| a.iter().position(|x| x.id == id)))
                },
                move |pos: &Option<usize>, _| {
                    if pos.is_some() {
                        jump.set(*pos);
                    }
                },
            );
            // The NATIVE list (docs/list.md): the platform table owns scrolling, cell reuse,
            // selection — drawn with the platform's own focused/unfocused treatment — the
            // arrow keys, and the swipe actions where the toolkit has them
            // (Cap::ListSwipeActions; the context menu and the Article menu carry the same
            // commands everywhere else).
            list(
                items(
                    move || sc.articles.get(),
                    |a: &ArticleSummary| a.id.to_string(),
                ),
                row_for,
            )
            .row_height(RowHeight::Uniform(ROW_H))
            // Selection reads as it moves (NetNewsWire): a click or a native arrow step both
            // land here and open the row.
            .on_select(|key: String| {
                if let Ok(id) = key.parse::<u64>() {
                    daynews_core::open_article(id);
                }
            })
            // Two-way: app-driven selection (Next Unread, the reader's restore) syncs into
            // the native list without re-emitting.
            .selected_rows(move || {
                let sel = sc.selected.get();
                sc.articles
                    .with(|a| sel.and_then(|id| a.iter().position(|x| x.id == id)))
                    .into_iter()
                    .collect()
            })
            .scroll_to_row(jump)
            // The trailing swipe toggles read/unread — Mail's triage gesture. The offer is
            // pulled at GESTURE time, so the button names the flip it would make.
            .swipe_trailing(move |i| {
                let Some((id, read)) = sc.articles.with(|a| a.get(i).map(|x| (x.id, x.is_read)))
                else {
                    return Vec::new();
                };
                let text = if read {
                    crate::res::str::mark_unread()
                } else {
                    crate::res::str::mark_read()
                };
                // The glyph speaks the dot language: marking read REMOVES the dot (an
                // outlined circle), marking unread restores it (filled).
                let symbol = if read {
                    Symbol::CircleFilled
                } else {
                    Symbol::Circle
                };
                vec![
                    swipe_action(text.format())
                        .symbol(symbol)
                        .tint(palette().accent)
                        .action(move || daynews_core::set_read(id, !read)),
                ]
            })
            // The leading swipe stars, in the star's own warm tint.
            .swipe_leading(move |i| {
                let Some((id, starred)) =
                    sc.articles.with(|a| a.get(i).map(|x| (x.id, x.is_starred)))
                else {
                    return Vec::new();
                };
                let text = if starred {
                    crate::res::str::unstar()
                } else {
                    crate::res::str::star()
                };
                vec![
                    swipe_action(text.format())
                        .symbol(Symbol::Star)
                        .tint(palette().star)
                        .action(move || daynews_core::set_starred(id, !starred)),
                ]
            })
            // The host draws the row separators, at the row boundary — aligned with the
            // native selection, and stationary while a swipe slides the row past them.
            .separators(true)
            // `.id` on the LIST itself (not a wrapper): `select:`/`swipe_row:` steps resolve
            // the id's node and expect the list driver right there.
            .id("timeline")
            .grow()
        },
    ))
    .background(move || palette().bg_alt)
    .grow()
}
