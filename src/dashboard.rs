//! A viewport-sized native dashboard. Chart data is a worker-owned metadata projection;
//! only the visible countdown changes each second, never the database or the chart marks.
use crate::{res, theme::palette};
use day::prelude::*;
use day_core::{Boundary, Flex, Layout, LayoutOps};
use day_piece_charts::{
    Coordinate, LegendPosition, Mark, area, bar, chart, line, rect, sector, time, value,
};
use day_spec::{Proposal, Rect};
use daynews_db::{Scope, dashboard::Dashboard};
use std::rc::Rc;

type Data = Signal<Option<Dashboard>>;
fn read<T: Default>(data: Data, f: impl FnOnce(&Dashboard) -> T) -> T {
    data.with(|snapshot| snapshot.as_ref().map(f).unwrap_or_default())
}
fn feed_page_url(data: Data) -> Option<String> {
    read(data, |d| {
        d.site_url
            .clone()
            .filter(|url| !url.trim().is_empty())
            .or_else(|| d.feed_url.clone().filter(|url| !url.trim().is_empty()))
    })
}
fn title(scope: Scope, data: Data) -> String {
    match scope {
        Scope::Feed(_) => read(data, |d| d.title.clone()),
        Scope::Today => res::str::nav_today().format(),
        Scope::Unread => res::str::nav_all_unread().format(),
        Scope::Starred => res::str::nav_starred().format(),
        Scope::All => res::str::nav_all_articles().format(),
        Scope::Folder(id) => daynews_core::state().folders.with(|f| {
            f.iter()
                .find(|f| f.id == id)
                .map(|f| f.name.clone())
                .unwrap_or_default()
        }),
        Scope::Tag(id) => daynews_core::state().tags.with(|f| {
            f.iter()
                .find(|f| f.id == id)
                .map(|f| f.name.clone())
                .unwrap_or_default()
        }),
    }
}
fn accent(scope: Scope) -> Color {
    match scope {
        Scope::Starred => palette().star,
        Scope::Today => Color::hex(if day::dark_mode() { 0x65D7BF } else { 0x168A76 }),
        Scope::Unread => Color::hex(if day::dark_mode() { 0xB49BFF } else { 0x7751C8 }),
        _ => palette().accent,
    }
}
fn card<P: Piece>(content: P) -> impl Piece {
    // Keep decoration behind the card's controls. A canvas overlay covers native hit
    // testing even when it paints only a thin border.
    content
        .padding(12.0)
        .background(move || palette().bg_alt)
        .corner_radius(13.0)
        .padding(1.0)
        .background(move || palette().rule.with_alpha(0.65))
        .corner_radius(14.0)
}

fn metric(
    label_text: impl Fn() -> String + 'static,
    amount: impl Fn() -> String + 'static,
    scope: Signal<Scope>,
) -> AnyPiece {
    column((
        label(amount)
            .font(Font::Title2)
            .weight(FontWeight::Bold)
            .color(move || accent(scope.get()))
            .single_line(),
        label(label_text)
            .font(Font::Caption)
            .color(move || palette().text_muted)
            .single_line(),
    ))
    .spacing(3.0)
    .align(HAlign::Leading)
    .grow_w()
    .any()
}
fn length_labels() -> [String; 5] {
    [
        res::str::dashboard_short().format(),
        res::str::dashboard_medium().format(),
        res::str::dashboard_long().format(),
        res::str::dashboard_deep().format(),
        res::str::dashboard_very_long().format(),
    ]
}
fn age_labels() -> [String; 5] {
    [
        res::str::dashboard_age_today().format(),
        res::str::dashboard_age_week().format(),
        res::str::dashboard_age_month().format(),
        res::str::dashboard_age_quarter().format(),
        res::str::dashboard_age_old().format(),
    ]
}
fn count_axis(config: &mut day_piece_charts::ChartConfig, max: u64) {
    let max = max.max(1);
    config.y_scale.domain = Some(day_piece_charts::Interval::new(0.0, max as f64));
    config.y_axis.format = Some(Rc::new(|v| {
        res::str::dashboard_number(v.as_continuous().unwrap_or(0.0)).format()
    }));
    config.y_axis.values = Some(if max <= 1 {
        vec![0.0, 1.0]
    } else {
        vec![0.0, max.div_ceil(2) as f64, max as f64]
    });
}
fn activity(data: Data, scope: Scope) -> Vec<Mark> {
    let count = res::str::dashboard_chart_count().format();
    if scope == Scope::Unread {
        return age_labels()
            .into_iter()
            .zip(read(data, |d| d.ages))
            .map(|(age, n)| {
                bar(value("", age), value(&count, n))
                    .foreground(accent(scope))
                    .corner_radius(5.0)
            })
            .collect();
    }
    if scope == Scope::Today {
        return read(data, |d| d.hourly)
            .into_iter()
            .enumerate()
            .map(|(hour, n)| {
                bar(
                    value("", res::str::dashboard_chart_hour(hour as f64).format()),
                    value(&count, n),
                )
                .foreground(accent(scope))
                .corner_radius(3.0)
            })
            .collect();
    }
    let now = daynews_time::now_unix();
    let local_now = now + i64::from(daynews_time::local_offset_seconds(now).unwrap_or(0));
    let earliest = read(data, |d| d.days.first().map(|d| d.0)).unwrap_or(local_now);
    let span = ((local_now - earliest).div_euclid(86400) + 1).clamp(28, 366);
    let width = (span + 13) / 14;
    let end = local_now.div_euclid(86400) * 86400;
    let start = end - (width * 14 - 1) * 86400;
    let mut bins = [0_u64; 14];
    data.with(|snapshot| {
        if let Some(snapshot) = snapshot {
            for &(date, n) in &snapshot.days {
                let index = (date - start).div_euclid(width * 86400);
                if (0..14).contains(&index) {
                    bins[index as usize] += n;
                }
            }
        }
    });
    bins.into_iter()
        .enumerate()
        .flat_map(|(i, n)| {
            let date = (start + (i as i64 * width + width / 2) * 86400) as f64;
            [
                area(time("", date), value(&count, n))
                    .foreground(accent(scope))
                    .opacity(0.18),
                line(time("", date), value(&count, n))
                    .foreground(accent(scope))
                    .line_width(2.8),
            ]
        })
        .collect()
}
fn activity_card(data: Data, scope: Signal<Scope>) -> AnyPiece {
    let selection = Signal::new(None);
    card(
        column((
            label(move || match scope.get() {
                Scope::Today => res::str::dashboard_activity_today().format(),
                Scope::Unread => res::str::dashboard_activity_unread().format(),
                Scope::Starred => res::str::dashboard_activity_starred().format(),
                _ => res::str::dashboard_activity().format(),
            })
            .font(Font::Headline)
            .color(move || palette().text)
            .single_line(),
            label(move || match scope.get() {
                Scope::Today => res::str::dashboard_today_note().format(),
                Scope::Unread => res::str::dashboard_unread_note().format(),
                _ => {
                    let now = daynews_time::now_unix();
                    let now = now + i64::from(daynews_time::local_offset_seconds(now).unwrap_or(0));
                    let start =
                        read(data, |d| d.days.first().map(|d| d.0)).unwrap_or(now - 27 * 86400);
                    res::str::dashboard_activity_note(now as f64, start as f64).format()
                }
            })
            .font(Font::Caption)
            .color(move || palette().text_muted)
            .single_line(),
            chart(move || activity(data, scope.get()))
                .animated()
                .animate_appearance()
                .legend(LegendPosition::Hidden)
                .label_size(10.0)
                .configure(move |config| {
                    let max = activity(data, scope.get())
                        .iter()
                        .filter_map(|mark| mark.y.as_ref().and_then(|y| y.datum.as_continuous()))
                        .fold(0.0_f64, f64::max) as u64;
                    count_axis(config, max);
                    config.x_axis.desired_count = 4;
                    config.x_axis.format = if matches!(scope.get(), Scope::Today | Scope::Unread) {
                        None
                    } else {
                        Some(Rc::new(|v| {
                            res::str::dashboard_chart_date(v.as_continuous().unwrap_or(0.0))
                                .format()
                        }))
                    };
                })
                .interact(
                    day_piece_charts::Inspect::new(selection)
                        .snap(day_piece_charts::Snap::NearestX)
                        .guides(day_piece_charts::Guides::RULE),
                )
                .id("dashboard-activity-chart")
                .grow(),
        ))
        .spacing(5.0)
        .align(HAlign::Leading)
        .grow(),
    )
    .any()
}
fn lengths_card(data: Data, scope: Signal<Scope>) -> AnyPiece {
    let selection = Signal::new(None);
    card(
        column((
            label(res::str::dashboard_lengths())
                .font(Font::Headline)
                .color(move || palette().text)
                .single_line(),
            label(res::str::dashboard_lengths_note())
                .font(Font::Caption)
                .color(move || palette().text_muted)
                .single_line(),
            chart(move || {
                length_labels()
                    .into_iter()
                    .zip(read(data, |d| d.lengths))
                    .enumerate()
                    .map(|(i, (name, n))| {
                        bar(
                            value(res::str::dashboard_chart_count().format(), n),
                            value(res::str::dashboard_chart_words().format(), name),
                        )
                        .x_range(
                            value(res::str::dashboard_chart_count().format(), 0),
                            value(res::str::dashboard_chart_count().format(), n),
                        )
                        .foreground(accent(scope.get()).with_alpha(0.45 + i as f64 * 0.12))
                        .corner_radius(4.0)
                    })
                    .collect()
            })
            .animated()
            .animate_appearance()
            .legend(LegendPosition::Hidden)
            .label_size(9.0)
            .y_reversed()
            .configure(move |config| {
                let max = read(data, |d| d.lengths.into_iter().max().unwrap_or(0)).max(1);
                config.x_scale.kind = Some(day_piece_charts::ScaleKind::Linear);
                config.x_scale.domain = Some(day_piece_charts::Interval::new(0.0, max as f64));
                config.x_axis.format = Some(Rc::new(|v| {
                    res::str::dashboard_number(v.as_continuous().unwrap_or(0.0)).format()
                }));
                config.x_axis.values = Some(if max <= 1 {
                    vec![0.0, 1.0]
                } else {
                    vec![0.0, max.div_ceil(2) as f64, max as f64]
                });
            })
            .interact(
                day_piece_charts::Inspect::new(selection)
                    .snap(day_piece_charts::Snap::NearestY)
                    .guides(day_piece_charts::Guides {
                        horizontal: true,
                        label: true,
                        ..day_piece_charts::Guides::NONE
                    }),
            )
            .id("dashboard-length-chart")
            .grow(),
            label(move || {
                res::str::dashboard_words(
                    read(data, |d| d.words.checked_div(d.total).unwrap_or(0)) as f64
                )
                .format()
            })
            .font(Font::Caption)
            .color(move || palette().text_muted)
            .single_line(),
        ))
        .spacing(4.0)
        .align(HAlign::Leading)
        .grow(),
    )
    .any()
}
fn sources(data: Data) -> Vec<(u64, String, u64)> {
    let mut sources = read(data, |d| d.sources.clone());
    if sources.len() > 4 {
        let other = sources.drain(4..).map(|s| s.2).sum();
        sources.push((0, res::str::dashboard_other().format(), other));
    }
    sources
}
fn weekdays() -> [String; 7] {
    [
        res::str::dashboard_mon().format(),
        res::str::dashboard_tue().format(),
        res::str::dashboard_wed().format(),
        res::str::dashboard_thu().format(),
        res::str::dashboard_fri().format(),
        res::str::dashboard_sat().format(),
        res::str::dashboard_sun().format(),
    ]
}
fn habits_card(data: Data) -> AnyPiece {
    let selection = Signal::new(None);
    card(
        column((
            label(res::str::dashboard_habits())
                .font(Font::Headline)
                .color(move || palette().text)
                .single_line(),
            label(res::str::dashboard_habits_note())
                .font(Font::Caption)
                .color(move || palette().text_muted)
                .single_line(),
            chart(move || {
                let habits = read(data, |d| d.habits);
                let max = habits.iter().flatten().copied().max().unwrap_or(0).max(1) as f64;
                weekdays()
                    .into_iter()
                    .enumerate()
                    .flat_map(|(day, name)| {
                        (0..6).map(move |hour| {
                            rect(
                                value("", name.clone()),
                                value(
                                    "",
                                    res::str::dashboard_chart_hour((hour * 4) as f64).format(),
                                ),
                            )
                            .selection_value(value(
                                res::str::dashboard_cell_annotation(
                                    habits[day][hour] as f64,
                                    (hour * 4) as f64,
                                )
                                .format(),
                                habits[day][hour],
                            ))
                            .foreground(if habits[day][hour] == 0 {
                                palette().rule.with_alpha(0.55)
                            } else {
                                day_piece_charts::sequential(
                                    0.35 + 0.6 * habits[day][hour] as f64 / max,
                                )
                            })
                            .corner_radius(3.0)
                        })
                    })
                    .collect()
            })
            .x_categories(weekdays())
            .y_reversed()
            .animated()
            .animate_appearance()
            .legend(LegendPosition::Hidden)
            .label_size(9.0)
            .interact(
                day_piece_charts::Inspect::new(selection)
                    .snap(day_piece_charts::Snap::NearestMark)
                    .guides(day_piece_charts::Guides::CROSSHAIR),
            )
            .id("dashboard-habits-chart")
            .grow(),
            label(move || {
                res::str::dashboard_read_share(
                    read(data, |d| d.starred) as f64,
                    read(data, |d| {
                        100.0 * (d.total - d.unread) as f64 / d.total.max(1) as f64
                    }),
                )
                .format()
            })
            .font(Font::Caption)
            .color(move || palette().text_muted)
            .single_line(),
            label(move || {
                read(data, |d| d.latest)
                    .map(|date| {
                        res::str::dashboard_last_story(
                            (date
                                + i64::from(daynews_time::local_offset_seconds(date).unwrap_or(0)))
                                as f64,
                        )
                        .format()
                    })
                    .unwrap_or_default()
            })
            .font(Font::Caption2)
            .color(move || palette().text_muted)
            .single_line(),
        ))
        .spacing(4.0)
        .align(HAlign::Leading)
        .grow(),
    )
    .any()
}
fn sources_card(data: Data) -> AnyPiece {
    let highlighted =
        day_piece_charts::PointSelection::new().project(day_piece_charts::Projection::Series);
    let links = day_piece_charts::Links::new().on_open(|target| {
        if let Some(id) = target
            .strip_prefix("feed:")
            .and_then(|id| id.parse::<u64>().ok())
        {
            if crate::feed_list::FeedList::app()
                .unread_only
                .get_untracked()
            {
                crate::feed_list::toggle();
            }
            // Changing scopes closes the compact reader. A publisher link opens the
            // target overview, so restore the detail layer after navigation settles.
            let scene = daynews_core::scene();
            batch(|| {
                daynews_core::select_scope(Scope::Feed(id));
                navigate(target);
                day::reactive::at_turn_end(move || scene.reader_open.set(true));
            });
        }
    });
    card(
        column((
            label(res::str::dashboard_sources())
                .font(Font::Headline)
                .color(move || palette().text)
                .single_line(),
            label(res::str::dashboard_sources_note())
                .font(Font::Caption)
                .color(move || palette().text_muted)
                .single_line(),
            PublisherPieces(vec![
                chart(move || {
                    sources(data)
                        .into_iter()
                        .map(|(id, _, n)| {
                            let mark = sector(value("", n))
                                .by_series(value("", id.to_string()))
                                .angular_inset(2.0);
                            if id == 0 {
                                mark
                            } else {
                                mark.link(format!("feed:{id}"))
                            }
                        })
                        .collect()
                })
                .coordinate(Coordinate::donut(0.68))
                .animated()
                .animate_appearance()
                .legend(LegendPosition::Hidden)
                .condition(
                    highlighted.predicate(),
                    day_piece_charts::Visual::default(),
                    day_piece_charts::Visual::opacity(0.22),
                )
                .interact(highlighted.on(day_piece_charts::EventSource::Hover))
                .interact(links.clone())
                .id("dashboard-publisher-chart")
                .any(),
                day_piece_charts::legend(move || {
                    sources(data)
                        .into_iter()
                        .enumerate()
                        .map(|(i, (id, name, n))| {
                            let entry = day_piece_charts::LegendEntry::new(
                                id.to_string(),
                                name,
                                day_piece_charts::categorical(i),
                            )
                            .detail(res::str::dashboard_number(n as f64).format());
                            if id == 0 {
                                entry
                            } else {
                                entry.link(format!("feed:{id}"))
                            }
                        })
                        .collect()
                })
                .interact(highlighted.on(day_piece_charts::EventSource::Hover))
                .id_prefix("dashboard-publisher")
                .links(links)
                .any(),
            ])
            .grow(),
        ))
        .spacing(3.0)
        .align(HAlign::Leading)
        .grow(),
    )
    .any()
}
fn cadence(seconds: i64) -> String {
    if seconds >= 86400 {
        res::str::dashboard_daily().format()
    } else if seconds < 3600 {
        res::str::dashboard_every_minutes((seconds / 60) as f64).format()
    } else {
        res::str::dashboard_every_hours(seconds as f64 / 3600.0).format()
    }
}
fn next_check(data: Data) -> Option<i64> {
    let (mode, fixed) = crate::refresh_schedule::schedule();
    if mode == 0 {
        return None;
    }
    read(data, |d| {
        d.feeds
            .iter()
            .filter_map(|feed| {
                let deadline = if crate::refresh_schedule::automatic(mode) {
                    feed.next.or(feed
                        .last_checked
                        .map(|last| last.saturating_add(feed.interval)))
                } else {
                    fixed
                };
                deadline.map(|deadline| deadline.max(feed.retry_after.unwrap_or(0)))
            })
            .min()
    })
}
fn forecast(data: Data, scope: Signal<Scope>, clock: Signal<i64>) -> AnyPiece {
    card(
        column((
            row((
                label(res::str::dashboard_automatic())
                    .font(Font::Caption2)
                    .weight(FontWeight::Bold)
                    .color(move || palette().accent),
                spacer(),
                label(move || {
                    let intervals = read(data, |d| {
                        d.feeds.iter().map(|f| f.interval).collect::<Vec<_>>()
                    });
                    match (intervals.iter().min(), intervals.iter().max()) {
                        (Some(a), Some(b)) if a != b => {
                            res::str::dashboard_interval_range(cadence(*a), cadence(*b)).format()
                        }
                        (Some(a), _) => cadence(*a),
                        _ => res::str::dashboard_no_schedule().format(),
                    }
                })
                .font(Font::Footnote)
                .weight(FontWeight::Semibold)
                .color(move || palette().text)
                .single_line(),
            ))
            .align(VAlign::Center)
            .grow_w(),
            row((
                label(move || match next_check(data) {
                    Some(next) => {
                        let local =
                            next + i64::from(daynews_time::local_offset_seconds(next).unwrap_or(0));
                        res::str::dashboard_next(local as f64).format()
                    }
                    None if crate::refresh_schedule::schedule().0 == 0 => {
                        res::str::dashboard_manual().format()
                    }
                    _ => res::str::dashboard_no_schedule().format(),
                })
                .font(Font::Caption)
                .color(move || palette().text_muted)
                .single_line()
                .grow_w(),
                label(move || {
                    let now = clock.get();
                    if daynews_core::state().updating_feeds.with(|active| {
                        read(data, |d| {
                            d.feeds.iter().any(|feed| active.contains_key(&feed.id))
                        })
                    }) {
                        return res::str::dashboard_refreshing().format();
                    }
                    let Some(next) = next_check(data) else {
                        return String::new();
                    };
                    let remaining = next.saturating_sub(now).max(0);
                    if remaining == 0 {
                        return res::str::dashboard_due().format();
                    }
                    res::str::dashboard_countdown(
                        (remaining / 3600) as f64,
                        ((remaining / 60) % 60) as f64,
                        (remaining % 60) as f64,
                    )
                    .format()
                })
                .font(Font::Footnote)
                .weight(FontWeight::Semibold)
                .color(move || palette().accent)
                .tabular()
                .id("dashboard-refresh-countdown"),
            ))
            .spacing(8.0)
            .align(VAlign::Center)
            .grow_w(),
            row((
                label(move || {
                    if read(data, |d| {
                        d.feeds.iter().any(|f| f.failed || f.retry_after.is_some())
                    }) {
                        res::str::dashboard_backoff().format()
                    } else if !crate::refresh_schedule::automatic(
                        crate::refresh_schedule::schedule().0,
                    ) && crate::refresh_schedule::schedule().0 != 0
                    {
                        res::str::dashboard_fixed().format()
                    } else {
                        res::str::dashboard_policy().format()
                    }
                })
                .font(Font::Caption2)
                .color(move || palette().text_muted)
                .single_line()
                .grow_w(),
                when(
                    move || read(data, |d| d.feed_id.is_some()),
                    move || {
                        button(res::str::dashboard_refresh_now())
                            .action(move || {
                                if let Some(id) = read(data, |d| d.feed_id) {
                                    daynews_core::refresh_feed(id);
                                }
                            })
                            .enabled(move || {
                                daynews_core::state().refresh_progress.get().is_none()
                                    && read(data, |d| {
                                        d.feed_id.is_some_and(|id| scope.get() == Scope::Feed(id))
                                    })
                            })
                            .id("dashboard-refresh-now")
                    },
                ),
            ))
            .spacing(8.0)
            .align(VAlign::Center)
            .grow_w(),
        ))
        .spacing(5.0)
        .align(HAlign::Leading)
        .grow_w(),
    )
    .any()
}

pub fn dashboard() -> impl Piece {
    let scope = daynews_core::scene().scope;
    let data: Data = Signal::new(None);
    let now = daynews_time::now_unix();
    let local_day = Signal::new(daynews_time::start_of_day(now));
    daynews_core::watch_dashboard(scope, local_day, data);
    let clock = Signal::new(now);
    let timer = day::task(async move {
        loop {
            day::sleep(1000).await;
            let now = daynews_time::now_unix();
            clock.set(now);
            local_day.set_if_changed(daynews_time::start_of_day(now));
        }
    });
    day::reactive::Scope::current().on_cleanup(move || timer.abort());
    let hero_text = column((
        label(move || match scope.get() {
            Scope::Today => res::str::dashboard_today_eyebrow().format(),
            Scope::Unread => res::str::dashboard_unread_eyebrow().format(),
            Scope::Starred => res::str::dashboard_starred_eyebrow().format(),
            Scope::Feed(_) => res::str::dashboard_overview().format(),
            _ => res::str::dashboard_library_eyebrow().format(),
        })
        .font(Font::Caption2)
        .weight(FontWeight::Bold)
        .color(move || accent(scope.get()))
        .single_line(),
        label(move || title(scope.get(), data))
            .font(Font::Title)
            .weight(FontWeight::Bold)
            .color(move || palette().text)
            .single_line()
            .id("dashboard-title"),
        label(move || match scope.get() {
            Scope::Today => res::str::dashboard_today_intro().format(),
            Scope::Unread => res::str::dashboard_unread_intro().format(),
            Scope::Starred => res::str::dashboard_starred_intro().format(),
            Scope::Feed(_) => read(data, |d| d.description.clone())
                .filter(|s| !s.trim().is_empty())
                .unwrap_or_else(|| res::str::dashboard_feed_intro().format()),
            _ => res::str::dashboard_library_intro().format(),
        })
        .font(Font::Footnote)
        .max_lines(2)
        .color(move || palette().text_muted),
        when(
            move || matches!(scope.get(), Scope::Feed(_)) && feed_page_url(data).is_some(),
            move || {
                label(move || feed_page_url(data).unwrap_or_default())
                    .font(Font::Caption2)
                    .single_line()
                    .color(move || accent(scope.get()))
                    .on_tap(move || {
                        if let Some(url) = feed_page_url(data) {
                            crate::settings::open_link(&url);
                        }
                    })
                    .a11y(|b| b.role(Role::Button))
                    .id("dashboard-feed-link")
            },
        )
        .otherwise(move || {
            label(move || {
                if data.get().is_none() {
                    return res::str::dashboard_loading().format();
                }
                if read(data, |d| d.total) == 0 {
                    return res::str::dashboard_empty_note().format();
                }
                res::str::dashboard_authors(read(data, |d| d.authors) as f64).format()
            })
            .font(Font::Caption2)
            .single_line()
            .color(move || accent(scope.get()))
        }),
    ))
    .spacing(5.0)
    .align(HAlign::Leading)
    .grow_w()
    .any();
    let hero = row((
        each(
            items(
                move || {
                    let downloaded = match scope.get() {
                        Scope::Feed(id) => {
                            crate::feed_icons::paths().with(|paths| paths.get(&id).cloned())
                        }
                        _ => None,
                    };
                    vec![(scope.get(), downloaded)]
                },
                |item| format!("{:?}", item),
            ),
            |slot| {
                let (scope, path) = slot.get();
                if let Some(path) = path {
                    image(path).decorative().frame(42.0, 42.0).any()
                } else {
                    vector(match scope {
                        Scope::Today => res::vectors::dashboard_today,
                        Scope::Unread => res::vectors::dashboard_unread,
                        Scope::Starred => res::vectors::dashboard_starred,
                        Scope::Feed(_) => res::vectors::dashboard_feed,
                        _ => res::vectors::dashboard_all,
                    })
                    .tint(move || accent(scope))
                    .decorative()
                    .frame(42.0, 42.0)
                    .any()
                }
            },
        )
        .padding(12.0)
        .background(move || accent(scope.get()).with_alpha(0.1))
        .corner_radius(18.0),
        hero_text,
    ))
    .spacing(14.0)
    .align(VAlign::Center)
    .grow_w()
    .any();
    let metrics = row((
        metric(
            || res::str::dashboard_articles().format(),
            move || res::str::dashboard_number(read(data, |d| d.total) as f64).format(),
            scope,
        ),
        metric(
            || res::str::dashboard_unread().format(),
            move || res::str::dashboard_number(read(data, |d| d.unread) as f64).format(),
            scope,
        ),
        metric(
            || res::str::dashboard_reading().format(),
            move || {
                res::str::dashboard_minutes(read(data, |d| d.words.div_ceil(240)) as f64).format()
            },
            scope,
        ),
        metric(
            move || {
                if matches!(scope.get(), Scope::Feed(_)) {
                    res::str::dashboard_contributors().format()
                } else {
                    res::str::dashboard_publishers().format()
                }
            },
            move || {
                res::str::dashboard_number(read(data, |d| {
                    if matches!(scope.get(), Scope::Feed(_)) {
                        d.authors
                    } else {
                        d.sources.len()
                    }
                }) as f64)
                .format()
            },
            scope,
        ),
    ))
    .spacing(10.0)
    .align(VAlign::Center)
    .grow_w()
    .any();
    DashboardPieces(vec![
        hero,
        metrics,
        activity_card(data, scope),
        lengths_card(data, scope),
        when(
            move || matches!(scope.get(), Scope::Feed(_)),
            move || habits_card(data),
        )
        .otherwise(move || sources_card(data))
        .any(),
        forecast(data, scope, clock),
    ])
    .padding(18.0)
    .background(move || palette().bg)
    .id("feed-dashboard")
    .grow()
}

/// Measures and places the six dashboard regions inside the exact offered viewport.
/// No scroll container or reactive state writes occur in layout. Charts share flexible space.
struct DashboardPieces(Vec<AnyPiece>);
impl Piece for DashboardPieces {
    fn build(self, cx: &mut BuildCx) -> day::RNode {
        let node = cx.native(
            day_spec::kinds::CONTAINER,
            &day_spec::props::ContainerProps {
                clips: true,
                ..Default::default()
            },
            Rc::new(DashboardLayout),
            Flex::default(),
            Boundary::No,
        );
        cx.under(node, |cx| {
            for child in self.0 {
                child.build(cx);
            }
        });
        node
    }
}
// Reserve chart space independently of the legend's intrinsic height. In short
// windows a vertical stack otherwise lets its five legend rows consume the donut.
struct PublisherPieces(Vec<AnyPiece>);
impl Piece for PublisherPieces {
    fn build(self, cx: &mut BuildCx) -> day::RNode {
        let node = cx.native(
            day_spec::kinds::CONTAINER,
            &day_spec::props::ContainerProps {
                clips: true,
                ..Default::default()
            },
            Rc::new(PublisherLayout),
            Flex::default(),
            Boundary::No,
        );
        cx.under(node, |cx| {
            for child in self.0 {
                child.build(cx);
            }
        });
        node
    }
}
struct PublisherLayout;
fn publisher_regions(size: Size) -> [Rect; 2] {
    let gap = 8.0_f64.min(size.width / 4.0);
    let plot = (size.width - gap) * 0.38;
    [
        Rect::new(0.0, 0.0, plot, size.height),
        Rect::new(plot + gap, 0.0, size.width - plot - gap, size.height),
    ]
}
impl Layout for PublisherLayout {
    fn measure(&self, cx: &mut dyn LayoutOps, children: &[day::RNode], p: Proposal) -> Size {
        let size = Size::new(p.width.unwrap_or(240.0), p.height.unwrap_or(150.0));
        for (&child, rect) in children.iter().zip(publisher_regions(size)) {
            cx.measure_child(child, Proposal::exact(rect.size));
        }
        size
    }
    fn place(&self, cx: &mut dyn LayoutOps, children: &[day::RNode], bounds: Rect) {
        for (&child, rect) in children.iter().zip(publisher_regions(bounds.size)) {
            cx.place_child(child, rect);
        }
    }
}
struct DashboardLayout;
fn regions(size: Size) -> [Rect; 6] {
    let w = size.width.max(0.0);
    let h = size.height.max(0.0);
    let compact = w < 480.0;
    let gap = 10.0_f64.min(h / 50.0).min(w / 4.0);
    let hero = (if compact { 100.0_f64 } else { 112.0_f64 }).min(h * 0.2);
    let metrics = 62.0_f64.min(h * 0.12);
    let footer = 94.0_f64.min(h * 0.18);
    let plot = (h - hero - metrics - footer - 4.0 * gap).max(0.0);
    let y = hero + metrics + 2.0 * gap;
    // Short windows need room for every publisher legend row below the activity plot.
    let activity_h = plot * if h < 650.0 { 0.36 } else { 0.46 };
    let lower = (plot - activity_h - gap).max(0.0);
    let half = ((w - gap) / 2.0).max(0.0);
    [
        Rect::new(0.0, 0.0, w, hero),
        Rect::new(0.0, hero + gap, w, metrics),
        Rect::new(0.0, y, w, activity_h),
        Rect::new(0.0, y + activity_h + gap, half, lower),
        Rect::new(half + gap, y + activity_h + gap, half, lower),
        Rect::new(0.0, h - footer, w, footer),
    ]
}
impl Layout for DashboardLayout {
    fn measure(&self, cx: &mut dyn LayoutOps, children: &[day::RNode], p: Proposal) -> Size {
        let size = Size::new(p.width.unwrap_or(680.0), p.height.unwrap_or(700.0));
        for (&child, rect) in children.iter().zip(regions(size)) {
            cx.measure_child(child, Proposal::exact(rect.size));
        }
        size
    }
    fn place(&self, cx: &mut dyn LayoutOps, children: &[day::RNode], bounds: Rect) {
        for (&child, rect) in children.iter().zip(regions(bounds.size)) {
            cx.place_child(child, rect);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dashboard_regions_fit_small_and_large_viewports_without_overlap() {
        for width in [0.0, 280.0, 390.0, 640.0, 1000.0] {
            for height in [0.0, 300.0, 550.0, 800.0, 1200.0] {
                let boxes = regions(Size::new(width, height));
                for rect in &boxes {
                    assert!(rect.origin.x >= 0.0 && rect.origin.y >= 0.0);
                    assert!(rect.size.width >= 0.0 && rect.size.height >= 0.0);
                    assert!(rect.origin.x + rect.size.width <= width + 0.0001);
                    assert!(rect.origin.y + rect.size.height <= height + 0.0001);
                }
                for a in 0..boxes.len() {
                    for b in a + 1..boxes.len() {
                        let x = (boxes[a].origin.x + boxes[a].size.width)
                            .min(boxes[b].origin.x + boxes[b].size.width)
                            - boxes[a].origin.x.max(boxes[b].origin.x);
                        let y = (boxes[a].origin.y + boxes[a].size.height)
                            .min(boxes[b].origin.y + boxes[b].size.height)
                            - boxes[a].origin.y.max(boxes[b].origin.y);
                        assert!(x <= 0.0001 || y <= 0.0001, "overlapping dashboard regions");
                    }
                }
            }
        }
    }
}
