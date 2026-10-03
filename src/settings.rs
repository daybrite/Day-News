//! Appearance, language, browser, and retention preferences. Choices persist in the
//! platform preference store and apply immediately.

use crate::theme::palette;
use day::prelude::*;

pub(crate) const THEME_KEY: &str = "news.theme";
const LOCALE_KEY: &str = "news.locale";
const BROWSER_KEY: &str = "news.browser";

const LIST_SCALE_KEY: &str = "news.list.scale";
pub const DEFAULT_LIST_SCALE: usize = 120;
pub const MIN_LIST_SCALE: f64 = 70.0;
pub const MAX_LIST_SCALE: f64 = 250.0;

const PREVIEW_LINES_KEY: &str = "news.list.preview_lines";
pub const DEFAULT_PREVIEW_LINES: usize = 2;
pub const MAX_PREVIEW_LINES: usize = 5;

thread_local! {
    static LIST_SCALE: std::cell::OnceCell<Signal<usize>> = const { std::cell::OnceCell::new() };
    static PREVIEW_LINES: std::cell::OnceCell<Signal<usize>> = const { std::cell::OnceCell::new() };
}

fn normalized_list_scale(value: f64) -> usize {
    if value.is_finite() {
        ((value / 10.0).round() * 10.0).clamp(MIN_LIST_SCALE, MAX_LIST_SCALE) as usize
    } else {
        DEFAULT_LIST_SCALE
    }
}

pub fn list_scale() -> Signal<usize> {
    LIST_SCALE.with(|slot| {
        *slot.get_or_init(|| {
            Signal::new_in(
                day::reactive::Scope::root(),
                day::prefs::get(LIST_SCALE_KEY)
                    .and_then(|value| value.parse().ok())
                    .map(normalized_list_scale)
                    .unwrap_or(DEFAULT_LIST_SCALE),
            )
        })
    })
}

#[derive(Clone, Copy)]
struct ListScale;
impl Binding<f64> for ListScale {
    fn read(&self) -> f64 {
        list_scale().get() as f64
    }
    fn peek(&self) -> f64 {
        list_scale().get_untracked() as f64
    }
    fn write(&self, value: f64) {
        let value = normalized_list_scale(value);
        if value == DEFAULT_LIST_SCALE {
            day::prefs::remove(LIST_SCALE_KEY);
        } else {
            day::prefs::set(LIST_SCALE_KEY, &value.to_string());
        }
        list_scale().set(value);
    }
}

pub fn preview_lines() -> Signal<usize> {
    PREVIEW_LINES.with(|slot| {
        *slot.get_or_init(|| {
            Signal::new_in(
                day::reactive::Scope::root(),
                day::prefs::get(PREVIEW_LINES_KEY)
                    .and_then(|value| value.parse::<usize>().ok())
                    .filter(|value| *value <= MAX_PREVIEW_LINES)
                    .unwrap_or(DEFAULT_PREVIEW_LINES),
            )
        })
    })
}

#[derive(Clone, Copy)]
struct PreviewLines;
impl Binding<usize> for PreviewLines {
    fn read(&self) -> usize {
        preview_lines().get()
    }
    fn peek(&self) -> usize {
        preview_lines().get_untracked()
    }
    fn write(&self, value: usize) {
        let value = value.min(MAX_PREVIEW_LINES);
        if value == DEFAULT_PREVIEW_LINES {
            day::prefs::remove(PREVIEW_LINES_KEY);
        } else {
            day::prefs::set(PREVIEW_LINES_KEY, &value.to_string());
        }
        preview_lines().set(value);
    }
}

pub fn reset_list_styles() {
    ListScale.write(DEFAULT_LIST_SCALE as f64);
    PreviewLines.write(DEFAULT_PREVIEW_LINES);
}

fn list_preferences() -> impl Piece {
    let labels = (0..=MAX_PREVIEW_LINES).map(|lines| {
        if lines == 0 {
            crate::res::str::settings_preview_none().format()
        } else {
            crate::res::str::settings_preview_lines_count(lines as f64).format()
        }
    });
    column((
        divider(),
        label(crate::res::str::settings_list_heading()).font(Font::Headline),
        labeled(
            crate::res::str::settings_list_size(),
            row((
                slider(ListScale)
                    .range(MIN_LIST_SCALE..=MAX_LIST_SCALE)
                    .step(10.0)
                    .id("list-size-slider")
                    .grow_w(),
                label(|| {
                    crate::res::str::settings_list_percent(list_scale().get() as f64).format()
                })
                .id("list-size-value")
                .width(64.0),
            ))
            .spacing(8.0)
            .align(VAlign::Center)
            .grow_w(),
        ),
        labeled(
            crate::res::str::settings_preview_lines(),
            picker(labels, PreviewLines).id("preview-lines-picker"),
        ),
    ))
    .spacing(12.0)
    .align(HAlign::Leading)
    .grow_w()
}

pub fn apply_startup() {
    day_piece_settings::apply_startup(THEME_KEY, LOCALE_KEY);
}

/// Browser overrides apply only to web links; mail, PDF/file and other schemes retain their
/// own system associations. Discovery is refreshed whenever preferences are opened.
pub fn open_link(url: &str) {
    let web_link = url.split_once(':').is_some_and(|(scheme, _)| {
        scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https")
    });
    let chosen = web_link.then(|| day::prefs::get(BROWSER_KEY)).flatten();
    if let Some(application) = chosen {
        let url = url.to_owned();
        day::task(async move {
            if day::open_url_with_application(&url, &application)
                .await
                .is_err()
            {
                // Removed/moved applications must never strand a link inside the reader.
                open_url(&url);
            }
        });
    } else {
        open_url(url);
    }
}

fn browser_picker() -> AnyPiece {
    let Ok(handlers) = day::application_handlers(&day::HandlerQuery::Scheme("https".into())) else {
        return column((
            labeled(
                crate::res::str::settings_browser_label(),
                label(crate::res::str::settings_browser_system()),
            ),
            label(crate::res::str::settings_browser_unavailable())
                .font(Font::Footnote)
                .color(move || palette().text_muted),
        ))
        .spacing(8.0)
        .align(HAlign::Leading)
        .any();
    };
    let stored = day::prefs::get(BROWSER_KEY);
    let selected = Signal::new(
        handlers
            .applications
            .iter()
            .position(|app| Some(&app.id) == stored.as_ref())
            .map_or(0, |i| i + 1),
    );
    let mut labels = vec![match &handlers.default {
        Some(app) => crate::res::str::settings_browser_default(app.name.as_str()).format(),
        None => crate::res::str::settings_browser_system().format(),
    }];
    labels.extend(handlers.applications.iter().map(|app| app.name.clone()));
    watch(
        move || selected.get(),
        move |index, _| {
            if let Some(app) = index
                .checked_sub(1)
                .and_then(|i| handlers.applications.get(i))
            {
                day::prefs::set(BROWSER_KEY, &app.id);
            } else {
                day::prefs::remove(BROWSER_KEY);
            }
        },
    );
    column((
        labeled(
            crate::res::str::settings_browser_label(),
            picker(labels, selected).id("browser-picker"),
        ),
        label(crate::res::str::settings_browser_note())
            .font(Font::Footnote)
            .color(move || palette().text_muted),
    ))
    .spacing(8.0)
    .align(HAlign::Leading)
    .any()
}

const RETENTION_KEY: &str = "retention.days";

/// The retention choices, in days; `0` keeps everything.
const CHOICES: [u32; 5] = [30, 90, 180, 365, 0];

fn choice_label(days: u32) -> String {
    match days {
        30 => crate::res::str::retention_30().format(),
        90 => crate::res::str::retention_90().format(),
        180 => crate::res::str::retention_180().format(),
        365 => crate::res::str::retention_365().format(),
        _ => crate::res::str::retention_forever().format(),
    }
}

/// The stored retention window, defaulting to daynews-core's 90 days.
pub fn retention_days() -> u32 {
    day::prefs::get(RETENTION_KEY)
        .and_then(|v| v.parse().ok())
        .unwrap_or(daynews_core::DEFAULT_RETENTION_DAYS)
}

pub fn settings_page() -> impl Piece {
    let selected = Signal::new(
        CHOICES
            .iter()
            .position(|d| *d == retention_days())
            .unwrap_or(1),
    );
    // This page's own note. The app-wide status line reports subscription work ("Subscribed to
    // …"), and shown here it sat under the retention setting as if the two were related.
    let note = Signal::new(String::new());
    // Applying is the watch, not the picker: the picker writes the index, the watch persists
    // it and prunes right away so the choice visibly acts.
    watch(
        move || selected.get(),
        move |i, _| {
            let days = CHOICES.get(*i).copied().unwrap_or(0);
            day::prefs::set(RETENTION_KEY, &days.to_string());
            day::task(async move {
                let pruned = daynews_core::prune(days).await;
                if pruned > 0 && note.try_get().is_some() {
                    note.set(crate::res::str::retention_pruned(pruned as f64).format());
                }
            });
        },
    );

    scroll(
        column((
            row((
                button(crate::res::str::opml_import())
                    .action(crate::subscriptions::import_opml)
                    .id("opml-import"),
                button(crate::res::str::opml_export())
                    .action(crate::subscriptions::export_opml)
                    .id("opml-export"),
            ))
            .spacing(8.0),
            label(move || daynews_core::state().status.get())
                .font(Font::Footnote)
                .id("status"),
            label(crate::res::str::settings_heading())
                .font(Font::Headline)
                .color(move || palette().text),
            labeled(
                crate::res::str::settings_appearance(),
                picker(
                    [
                        crate::res::str::settings_light().format(),
                        crate::res::str::settings_dark().format(),
                        crate::res::str::settings_system().format(),
                    ],
                    crate::reader_styles::Appearance,
                )
                .segmented()
                .id("theme-picker"),
            ),
            labeled(
                crate::res::str::group_by_feeds(),
                toggle(Grouping).id("group-by-feeds-picker"),
            ),
            list_preferences(),
            reader_preferences(),
            crate::reader_options::controls(None),
            crate::site_browser::privacy_control(),
            day_piece_settings::language_picker(LOCALE_KEY, crate::res::locales::ALL),
            browser_picker(),
            labeled(
                crate::res::str::settings_refresh_feeds(),
                picker(
                    [
                        crate::res::str::settings_refresh_automatic().format(),
                        crate::res::str::settings_refresh_manual().format(),
                        crate::res::str::settings_refresh_30().format(),
                        crate::res::str::settings_refresh_60().format(),
                        crate::res::str::settings_refresh_120().format(),
                        crate::res::str::settings_refresh_240().format(),
                        crate::res::str::settings_refresh_480().format(),
                    ],
                    crate::refresh_schedule::Interval,
                )
                .id("refresh-interval-picker"),
            ),
            label(crate::res::str::settings_refresh_automatic_note())
                .font(Font::Footnote)
                .max_lines(3),
            labeled(
                crate::res::str::settings_retention_label(),
                picker(CHOICES.iter().map(|d| choice_label(*d)), selected).id("retention-picker"),
            ),
            label(crate::res::str::settings_retention_note())
                .font(Font::Footnote)
                .color(move || palette().text_muted),
            label(move || note.get())
                .font(Font::Footnote)
                .color(move || palette().text_muted)
                .id("settings-status"),
        ))
        .spacing(12.0)
        .align(HAlign::Leading)
        .padding(18.0)
        .grow_w(),
    )
    .background(move || palette().bg)
    .grow()
}

fn reader_preferences() -> impl Piece {
    use crate::reader_styles::{self as styles, FontChoice, ReaderColor};
    let (mut families, boundary) = styles::font_choices(
        day::font_families()
            .iter()
            .map(|f| f.family.clone())
            .collect(),
    );
    // A persisted family can disappear after uninstalling a font. Keep it visible and let
    // CSS use its system fallback until the user selects another family or resets styles.
    let stored = styles::state().get_untracked().family;
    if !stored.is_empty() && !families.contains(&stored) {
        families.push(stored);
    }
    let mut labels = families.clone();
    labels[0] = crate::res::str::settings_reader_default_font().format();
    column((
        divider(),
        label(crate::res::str::settings_reader_heading()).font(Font::Headline),
        labeled(
            crate::res::str::settings_reader_size(),
            row((
                slider(styles::Scale)
                    .range(styles::MIN_SCALE..=styles::MAX_SCALE)
                    .step(10.0)
                    .id("reader-size-slider")
                    .grow_w(),
                label(move || {
                    crate::res::str::settings_reader_percent(styles::state().get().scale).format()
                })
                .id("reader-size-value")
                .width(64.0),
            ))
            .spacing(12.0)
            .grow_w(),
        ),
        labeled(
            crate::res::str::settings_reader_font(),
            picker(labels, FontChoice(std::rc::Rc::new(families)))
                .separators_before([boundary])
                .id("reader-font-picker"),
        ),
        label(crate::res::str::settings_reader_font_note())
            .font(Font::Footnote)
            .color(move || palette().text_muted),
        labeled(
            crate::res::str::settings_reader_background(),
            day_piece_colorpicker::color_picker(ReaderColor::Background)
                .title(crate::res::str::settings_reader_background())
                .key("reader-background-color"),
        ),
        labeled(
            crate::res::str::settings_reader_text(),
            day_piece_colorpicker::color_picker(ReaderColor::Text)
                .title(crate::res::str::settings_reader_text())
                .key("reader-text-color"),
        ),
        button(crate::res::str::settings_reader_reset())
            .action(styles::reset)
            .id("reader-reset-styles"),
        label(crate::res::str::settings_reader_reset_note())
            .font(Font::Footnote)
            .color(move || palette().text_muted),
        divider(),
    ))
    .spacing(12.0)
    .align(HAlign::Leading)
    .grow_w()
}

#[derive(Clone, Copy)]
struct Grouping;
impl Binding<bool> for Grouping {
    fn read(&self) -> bool {
        daynews_core::state().group_by_feed.get()
    }
    fn peek(&self) -> bool {
        daynews_core::state().group_by_feed.get_untracked()
    }
    fn write(&self, value: bool) {
        if value != self.peek() {
            crate::feed_list::toggle_grouping();
        }
    }
}
