//! The article list, NetNewsWire's middle pane: title, summary, and a feed·date footer, with
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

/// Scale the two title lines, footer, and preview line budget together; keep padding
/// and inter-label gaps fixed. Zero preview lines also removes one gap.
#[cfg(not(any(target_os = "ios", target_os = "android", target_env = "ohos")))]
fn row_height(preview_lines: usize, scale: f64) -> f64 {
    22.0 + (46.0 + preview_lines as f64 * 14.0) * scale - if preview_lines == 0 { 3.0 } else { 0.0 }
}
#[cfg(any(target_os = "ios", target_os = "android", target_env = "ohos"))]
fn row_height(preview_lines: usize, scale: f64) -> f64 {
    22.0 + (66.0 + preview_lines as f64 * 18.0) * scale - if preview_lines == 0 { 3.0 } else { 0.0 }
}

/// One timeline row, bound to its slot.
///
/// Every varying field, the id included, is read inside a reactive closure. The native list
/// recycles cells: a scrolled-away row's cell is rebound to a different article by one slot
/// write, so anything captured eagerly freezes at the value the cell was born with, not the
/// article it now shows.
fn row_for(
    slot: ItemSlot<ArticleSummary, String>,
    preview_lines: usize,
    scale: f64,
    sc: daynews_core::NewsScene,
    grouped: bool,
) -> impl Piece {
    let id = move || slot.field(|a| a.id);
    let read = move || slot.field(|a| a.is_read);

    // Selection is the native list's to draw (docs/list.md): the platform highlight tracks
    // the table's own focus the way Mail's does. AppKit adapts plain label colors
    // to the native selection while preserving these colors for unselected rows.
    let title_color = move || {
        if read() {
            palette().text_muted
        } else {
            palette().text
        }
    };
    let sub_color = move || palette().text_muted;

    let article = row((
        // The dot rides at the title's optical center rather than the row's: a three-line row
        // would otherwise float it down beside the summary. The gutter keeps its width whether
        // or not a dot is drawn, so every title starts on the same x.
        column((
            column(()).height(9.0 * scale - DOT / 2.0),
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
            .font_scale(scale)
            .max_lines(2)
            .weight(FontWeight::Semibold)
            .color(title_color),
            when(
                move || preview_lines > 0 && slot.field(|a| a.summary.is_some()),
                move || {
                    // Apple labels enforce a real line cap. Keep the previous approximate
                    // excerpt budget on backends that do not yet support native line limits.
                    let chars = if cfg!(any(feature = "appkit", feature = "uikit")) {
                        2048
                    } else if cfg!(any(target_os = "android", target_env = "ohos")) {
                        45 * preview_lines
                    } else {
                        55 * preview_lines
                    };
                    label(move || {
                        slot.field(|a| snippet(a.summary.as_deref().unwrap_or(""), chars))
                    })
                    .font(SUMMARY_FONT)
                    .font_scale(scale)
                    .max_lines(preview_lines as u32)
                    .id_of(move || format!("article-preview-{}", id()))
                    .color(sub_color)
                },
            ),
            row((
                label(move || slot.field(|a| a.feed_title.clone()))
                    .font(FOOTER_FONT)
                    .font_scale(scale)
                    .single_line()
                    .weight(FontWeight::Medium)
                    .color(sub_color)
                    .grow_w(),
                label(move || slot.field(|a| relative_time(a.published_at)))
                    .font(FOOTER_FONT)
                    .font_scale(scale)
                    .single_line()
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
                    .font_scale(scale)
                    .color(move || palette().star)
            },
        ),
    ))
    .spacing(6.0)
    .align(VAlign::Top)
    .padding(Insets::symmetric(12.0, 8.0))
    // Taps are the native table's now (they select, and selection opens; see
    // `timeline_pane`), so the menu is the row's only gesture of its own.
    .context_menu_fn(move |_| {
        // Snapshot the clicked row when the menu opens. A recycled cell or later selection
        // change must not redirect a menu action to a different article.
        let article = slot.get();
        let (id, read, starred) = (article.id, article.is_read, article.is_starred);
        let url = article.url.filter(|url| !url.trim().is_empty());
        let browser_url = url.clone();
        vec![
            menu_item(if read {
                crate::res::str::mark_unread().format()
            } else {
                crate::res::str::mark_read().format()
            })
            .id("row-toggle-read")
            .action(move || daynews_core::set_read(id, !read)),
            menu_item(if starred {
                crate::res::str::unstar().format()
            } else {
                crate::res::str::star().format()
            })
            .id("row-toggle-star")
            .action(move || daynews_core::set_starred(id, !starred)),
            menu_item(crate::res::str::tag_action().format())
                .id("row-tag")
                .action(move || begin_tag(id)),
            menu_separator(),
            menu_item(crate::res::str::menu_open_in_browser().format())
                .id("row-open-in-browser")
                .enabled(url.is_some())
                .action(move || {
                    if let Some(url) = &browser_url {
                        crate::settings::open_link(url);
                    }
                }),
            menu_item(crate::res::str::menu_copy_article_link().format())
                .id("row-copy-article-link")
                .enabled(url.is_some())
                .action(move || {
                    if let Some(url) = &url {
                        crate::commands::copy_link(url);
                    }
                }),
        ]
    })
    // The positional id: a script addresses "the first row" without knowing which article
    // the network delivered. Reactive, so a recycled cell re-labels as it rebinds.
    // Separation between rows is the list's (`.separators(true)` in `timeline_pane`), drawn
    // by the host at the row boundary; nothing else wraps the row.
    .id_of(move || {
        let id = id();
        let pos = sc
            .articles
            .with(|a| a.iter().position(|x| x.id == id))
            .unwrap_or(usize::MAX);
        format!("article-row-{pos}")
    })
    .grow_w();
    column((
        when(
            move || {
                grouped
                    && sc.articles.with(|rows| {
                        rows.iter()
                            .position(|row| row.id == id())
                            .is_some_and(|index| {
                                index == 0 || rows[index - 1].feed_id != rows[index].feed_id
                            })
                    })
            },
            move || {
                feed_header(
                    move || slot.field(|a| (a.feed_id, a.feed_title.clone())),
                    false,
                )
            },
        ),
        article,
    ))
    .spacing(0.0)
    .align(HAlign::Leading)
    .grow_w()
}

fn feed_header(feed: impl Fn() -> (u64, String) + Copy + 'static, floating: bool) -> impl Piece {
    row((
        each(
            items(
                move || {
                    vec![crate::feed_icons::paths().with(|icons| icons.get(&feed().0).cloned())]
                },
                |path| path.clone(),
            ),
            |slot| {
                if let Some(path) = slot.get() {
                    image(path).decorative().frame(20.0, 20.0).any()
                } else {
                    vector(crate::res::vectors::dashboard_feed)
                        .tint(move || palette().accent)
                        .decorative()
                        .frame(20.0, 20.0)
                        .any()
                }
            },
        ),
        label(move || feed().1)
            .font(Font::Footnote)
            .weight(FontWeight::Semibold)
            .single_line()
            .id_of(move || {
                if floating {
                    "floating-feed-name".to_owned()
                } else {
                    format!("feed-group-name-{}", feed().0)
                }
            })
            .grow_w(),
    ))
    .spacing(8.0)
    .align(VAlign::Center)
    .padding(Insets::symmetric(12.0, 4.0))
    .height(32.0)
    .background(move || {
        if floating {
            let bg = palette().bg;
            let tint = palette().accent;
            Color::rgba(
                bg.r * 0.91 + tint.r * 0.09,
                bg.g * 0.91 + tint.g * 0.09,
                bg.b * 0.91 + tint.b * 0.09,
                0.92,
            )
        } else {
            palette().bg_alt
        }
    })
    .corner_radius(if floating { 8.0 } else { 0.0 })
    .overlay(canvas(move |draw, size| {
        if floating {
            draw.stroke(
                Shape::RoundedRect(
                    Rect::new(
                        0.5,
                        0.5,
                        (size.width - 1.0).max(0.0),
                        (size.height - 1.0).max(0.0),
                    ),
                    8.0,
                ),
                palette().accent.with_alpha(0.38),
                1.0,
            );
        }
    }))
    .id_of(move || {
        if floating {
            "floating-feed-group".to_owned()
        } else {
            format!("feed-group-{}", feed().0)
        }
    })
    .grow_w()
}

/// Prompt for a tag name and toggle it on the article, creating the tag on first use.
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
    // Search lives in the window toolbar where there is one (the nav's `.searchable` field);
    // without a toolbar the timeline carries the field itself. Both write the same signal, and
    // the query follows it either way; watching only the inline field left the toolbar's
    // search typing into a box that filtered nothing.
    let search = crate::toolbar::search();
    let in_toolbar = crate::toolbar::available();

    column((
        // Heading: the scope's name over its unread count, with the two actions a reader
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
                    let n = sc.scope_unread.get();
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
                        crate::commands::refresh().button(),
                        crate::commands::mark_all_read().button(),
                        crate::commands::next_unread().button(),
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
                row((
                    spinner(),
                    label(move || {
                        let (done, total) = st.refresh_progress.get().unwrap_or((0, 0));
                        crate::res::str::refresh_progress(done as f64, total as f64).format()
                    })
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
        // Rebuild just the native list when its display geometry changes; selection is
        // restored from the scene's stable article ID by the existing two-way binding.
        each(
            items(
                move || {
                    vec![(
                        crate::settings::preview_lines().get(),
                        crate::settings::list_scale().get(),
                        st.group_by_feed.get(),
                    )]
                },
                |style| *style,
            ),
            move |slot| {
                let (lines, percent, grouped) = slot.get();
                timeline_rows(lines, percent as f64 / 100.0, sc, grouped)
            },
        ),
    ))
    .background(move || palette().bg_alt)
    .toolbar(crate::toolbar::list_items)
    .grow()
}

fn timeline_rows(
    preview_lines: usize,
    scale: f64,
    sc: daynews_core::NewsScene,
    grouped: bool,
) -> impl Piece {
    column((
        when(
            move || sc.articles.with(|a| a.is_empty()),
            move || {
                column((
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
                        .font_scale(scale)
                        .color(move || palette().text_muted)
                        .id("timeline-empty"),
                        spacer(),
                    ))
                    .align(HAlign::Center)
                    .height(row_height(preview_lines, scale))
                    .grow_w()
                    .id("timeline-empty-row"),
                    divider(),
                ))
                .grow_w()
            },
        ),
        {
            // Programmatic selection moves (Next Unread, a scripted `select:`) can land
            // anywhere in the window; follow them so the selected row is visible. `watch`
            // never fires for the initial run, so building the pane doesn't force a scroll.
            let jump: Signal<Option<usize>> = Signal::new(None);
            watch(
                move || {
                    let sel = sc.selected.get();
                    let pos = sc
                        .articles
                        .with(|a| sel.and_then(|id| a.iter().position(|x| x.id == id)));
                    (sel, pos)
                },
                move |(selected, pos), previous| {
                    // New feed rows can move the index of the same selected article. That
                    // must not pull the scroll position away from a person browsing the list.
                    let changed =
                        previous.is_none_or(|(old, old_pos)| old != selected || old_pos.is_none());
                    if changed && pos.is_some() {
                        jump.set(*pos);
                    }
                },
            );
            // The native list (docs/list.md): the platform table owns scrolling, cell reuse,
            // selection (drawn with the platform's focused/unfocused treatment), the
            // arrow keys, and the swipe actions where the toolkit has them
            // (Cap::ListSwipeActions; the context menu and the Article menu carry the same
            // commands everywhere else).
            let first_visible = Signal::new(0usize);
            let timeline = list(
                items(
                    move || sc.articles.get(),
                    |a: &ArticleSummary| a.id.to_string(),
                ),
                move |slot| row_for(slot, preview_lines, scale, sc, grouped),
            );
            let timeline = if grouped {
                timeline.first_visible_row(first_visible)
            } else {
                timeline
            };
            timeline
                .row_height(RowHeight::Uniform(
                    row_height(preview_lines, scale) + if grouped { 32.0 } else { 0.0 },
                ))
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
                .focused(sc.timeline_focused)
                // The left (leading) swipe action toggles read/unread. The offer is
                // pulled at gesture time, so the button names the flip it would make.
                .swipe_leading(move |i| {
                    let Some((id, read)) =
                        sc.articles.with(|a| a.get(i).map(|x| (x.id, x.is_read)))
                    else {
                        return Vec::new();
                    };
                    let text = if read {
                        crate::res::str::mark_unread()
                    } else {
                        crate::res::str::mark_read()
                    };
                    // The glyph speaks the dot language: marking read removes the dot (an
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
                // The right (trailing) swipe action stars, in the star's own warm tint.
                .swipe_trailing(move |i| {
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
                // The host draws the row separators, at the row boundary, aligned with the
                // native selection, and stationary while a swipe slides the row past them.
                .separators(true)
                // `.id` on the list itself (not a wrapper): `select:`/`swipe_row:` steps resolve
                // the id's node and expect the list driver right there.
                .id("timeline")
                .overlay_aligned(
                    Alignment::TopLeading,
                    when(
                        move || grouped && !sc.articles.with(|rows| rows.is_empty()),
                        move || {
                            feed_header(
                                move || {
                                    sc.articles.with(|rows| {
                                        rows.get(
                                            first_visible.get().min(rows.len().saturating_sub(1)),
                                        )
                                        .map(|article| {
                                            (article.feed_id, article.feed_title.clone())
                                        })
                                        .unwrap_or_default()
                                    })
                                },
                                true,
                            )
                        },
                    ),
                )
                .grow()
        },
    ))
    .grow()
}
