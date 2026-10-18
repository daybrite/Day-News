//! Native account setup; translated presentation stays on the UI thread.
use crate::res;
use day::prelude::*;

pub fn page() -> impl Piece {
    let status = daynews_core::sync_status();
    let username = Signal::new(status.get_untracked().username);
    let password = Signal::new(String::new());
    let merge = Signal::new(false);
    column((
        label(res::str::sync_feedbin()).font(Font::Title2),
        label(res::str::sync_description()).font(Font::Body),
        sync_progress(),
        when(
            move || !status.get().connected && !status.get().preview,
            move || {
                column((
                    label(res::str::sync_username()),
                    text_field(username)
                        .placeholder(res::str::sync_username())
                        .id("sync-username")
                        .grow_w(),
                    label(res::str::sync_password()),
                    secure_field(password)
                        .placeholder(res::str::sync_password())
                        .id("sync-password")
                        .grow_w(),
                    button(res::str::sync_sign_in())
                        .enabled(move || {
                            !status.get().busy
                                && !username.get().trim().is_empty()
                                && !password.get().is_empty()
                        })
                        .action(move || {
                            let secret = password.get_untracked();
                            password.set(String::new());
                            daynews_core::sync_sign_in(username.get_untracked(), secret);
                        })
                        .id("sync-sign-in"),
                ))
                .spacing(8.0)
                .grow_w()
            },
        ),
        when(
            move || status.get().preview,
            move || {
                column((
                    label(move || {
                        res::str::sync_preview(
                            status.get().local_feeds as f64,
                            status.get().remote_feeds as f64,
                        )
                        .format()
                    })
                    .id("sync-preview"),
                    label(res::str::sync_read_state_import()).font(Font::Footnote),
                    row((
                        label(res::str::sync_merge()).grow_w(),
                        toggle(merge).id("sync-merge"),
                    ))
                    .spacing(8.0),
                    row((
                        button(res::str::sync_connect())
                            .enabled(move || !status.get().busy)
                            .action(move || daynews_core::sync_connect(merge.get_untracked()))
                            .id("sync-connect"),
                        button(res::str::sync_cancel())
                            .enabled(move || !status.get().busy)
                            .action(daynews_core::sync_cancel_login)
                            .id("sync-cancel"),
                    ))
                    .spacing(12.0),
                ))
                .spacing(12.0)
            },
        ),
        when(
            move || status.get().connected,
            move || {
                column((
                    label(move || res::str::sync_account(status.get().username).format())
                        .id("sync-account"),
                    label(move || res::str::sync_pending(status.get().pending as f64).format())
                        .id("sync-pending"),
                    label(move || match status.get().last_sync {
                        Some(at) => res::str::sync_last(crate::format::full_date(at)).format(),
                        None => res::str::sync_never().format(),
                    })
                    .id("sync-last"),
                    row((
                        button(res::str::sync_now())
                            .enabled(move || !status.get().busy)
                            .action(daynews_core::sync_now)
                            .id("sync-now"),
                        button(res::str::sync_sign_in())
                            .enabled(move || !status.get().busy)
                            .action(daynews_core::sync_reauthenticate)
                            .id("sync-reauthenticate"),
                        button(res::str::sync_disconnect())
                            .enabled(move || !status.get().busy)
                            .action(daynews_core::sync_disconnect)
                            .id("sync-disconnect"),
                    ))
                    .spacing(12.0),
                    label(res::str::sync_disconnect_note()).font(Font::Footnote),
                    button(res::str::sync_reset())
                        .enabled(move || {
                            !status.get().busy
                                && daynews_core::state().refresh_progress.get().is_none()
                                && daynews_core::state().updating_feeds.get().is_empty()
                        })
                        .action(|| {
                            // All translations are formatted on the UI thread before awaiting.
                            let dialog = alert(res::str::sync_reset_title())
                                .message(res::str::sync_reset_message())
                                .destructive(res::str::sync_reset_confirm(), ())
                                .cancel(res::str::sync_cancel());
                            day::task(async move {
                                if dialog.present().await.is_some() {
                                    daynews_core::sync_reset_local();
                                }
                            });
                        })
                        .id("sync-reset"),
                ))
                .spacing(10.0)
            },
        ),
        label(move || {
            let s = status.get();
            if let Some(error) = s.error {
                match error {
                    daynews_sync::Error::Authentication => res::str::sync_auth_error().format(),
                    daynews_sync::Error::Storage => res::str::sync_storage_error().format(),
                    daynews_sync::Error::Network | daynews_sync::Error::Retry { .. } => {
                        res::str::sync_network_error().format()
                    }
                    _ => res::str::sync_failed().format(),
                }
            } else if s.paused && !s.busy {
                res::str::sync_paused().format()
            } else if s.busy {
                res::str::sync_working().format()
            } else if s.connected && s.pending > 0 {
                res::str::sync_waiting().format()
            } else if s.connected {
                res::str::sync_ready().format()
            } else {
                String::new()
            }
        })
        .id("sync-status"),
    ))
    .spacing(14.0)
    .align(HAlign::Leading)
    .grow_w()
}

/// Shared by Accounts and the article list, so a toolbar-triggered sync is observable too.
pub fn sync_progress() -> impl Piece {
    let status = daynews_core::sync_status();
    when(
        move || status.get().busy && status.get().progress.is_some(),
        move || {
            column((
                when(
                    move || {
                        status
                            .get()
                            .progress
                            .as_ref()
                            .and_then(|p| p.fraction())
                            .is_some()
                    },
                    move || {
                        progress(move || {
                            status
                                .get()
                                .progress
                                .as_ref()
                                .and_then(|p| p.fraction())
                                .unwrap_or(0.0)
                        })
                        .id("sync-progress-bar")
                        .grow_w()
                    },
                ),
                when(
                    move || {
                        status
                            .get()
                            .progress
                            .as_ref()
                            .and_then(|p| p.fraction())
                            .is_none()
                    },
                    spinner,
                ),
                label(move || progress_text(&status.get().progress.unwrap_or_default()))
                    .font(Font::Footnote)
                    .id("sync-progress-activity"),
                when(
                    move || {
                        status
                            .get()
                            .progress
                            .as_ref()
                            .is_some_and(|p| p.feed.is_some())
                    },
                    move || {
                        label(move || {
                            res::str::sync_progress_feed(
                                status
                                    .get()
                                    .progress
                                    .and_then(|p| p.feed)
                                    .unwrap_or_default(),
                            )
                            .format()
                        })
                        .font(Font::Caption)
                        .id("sync-progress-feed")
                    },
                ),
                label(move || {
                    let s = status.get();
                    let duration = |seconds: u64| {
                        res::str::sync_progress_duration(
                            (seconds / 60) as f64,
                            (seconds % 60) as f64,
                        )
                        .format()
                    };
                    match s.remaining_seconds {
                        Some(seconds) => res::str::sync_progress_eta(
                            duration(s.elapsed_seconds),
                            duration(seconds),
                        )
                        .format(),
                        None => {
                            res::str::sync_progress_estimating(duration(s.elapsed_seconds)).format()
                        }
                    }
                })
                .font(Font::Caption)
                .id("sync-progress-time"),
                when(
                    move || status.get().cancellable,
                    || {
                        button(res::str::sync_cancel_transfer())
                            .action(daynews_core::sync_cancel)
                            .id("sync-cancel-transfer")
                    },
                ),
            ))
            .spacing(6.0)
            .align(HAlign::Leading)
            .grow_w()
        },
    )
}
fn progress_text(p: &daynews_sync::progress::Progress) -> String {
    use daynews_sync::progress::Phase;
    match p.phase {
        Phase::Preparing => res::str::sync_progress_preparing().format(),
        Phase::Uploading => {
            res::str::sync_progress_upload(p.completed as f64, p.total.unwrap_or(0) as f64).format()
        }
        Phase::Subscriptions => {
            res::str::sync_progress_subscriptions(p.subscriptions as f64).format()
        }
        Phase::ReadState => res::str::sync_progress_read().format(),
        Phase::Articles => match p.total {
            Some(total) => res::str::sync_progress_articles(
                p.articles as f64,
                p.completed as f64,
                total as f64,
            )
            .format(),
            None => res::str::sync_progress_articles_unknown(p.articles as f64, p.completed as f64)
                .format(),
        },
        Phase::MissingArticles => {
            res::str::sync_progress_missing(p.completed as f64, p.total.unwrap_or(0) as f64)
                .format()
        }
        Phase::Finalizing => res::str::sync_progress_finalizing().format(),
        Phase::Complete => res::str::sync_progress_complete().format(),
        Phase::Resetting => res::str::sync_progress_resetting().format(),
    }
}
