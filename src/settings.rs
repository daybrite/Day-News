//! Settings: how long read articles are kept. The choice lives in the platform preference
//! store; pruning runs at launch and immediately when the window shortens.

use crate::theme::palette;
use day::prelude::*;

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
    // Applying is the watch, not the picker: the picker writes the index, the watch persists
    // it and prunes right away so the choice visibly acts.
    watch(
        move || selected.get(),
        move |i, _| {
            let days = CHOICES.get(*i).copied().unwrap_or(0);
            day::prefs::set(RETENTION_KEY, &days.to_string());
            let pruned = daynews_core::prune(days);
            if pruned > 0 {
                daynews_core::state()
                    .status
                    .set(crate::res::str::retention_pruned(pruned as f64).format());
            }
        },
    );

    scroll(
        column((
            label(crate::res::str::settings_heading())
                .font(Font::Headline)
                .color(move || palette().text),
            labeled(
                crate::res::str::settings_retention_label(),
                picker(CHOICES.iter().map(|d| choice_label(*d)), selected).id("retention-picker"),
            ),
            label(crate::res::str::settings_retention_note())
                .font(Font::Footnote)
                .color(move || palette().text_muted),
            label(move || daynews_core::state().status.get())
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
