//! One app-wide refresh timer, independent of the preferences window's lifetime.
use day::prelude::Signal;
use futures_util::future::{AbortHandle, Abortable};
use std::cell::{OnceCell, RefCell};

const KEY: &str = "news.refresh.minutes";
const AUTOMATIC: u64 = u64::MAX;
pub const CHOICES: [u64; 7] = [AUTOMATIC, 0, 30, 60, 120, 240, 480];
const DEFAULT: u64 = AUTOMATIC;

thread_local! {
    static NEXT: OnceCell<Signal<Option<i64>>> = const { OnceCell::new() };
    static INTERVAL: OnceCell<Signal<usize>> = const { OnceCell::new() };
    static TIMER: RefCell<Option<AbortHandle>> = const { RefCell::new(None) };
}

fn stored_minutes() -> u64 {
    day::prefs::get(KEY)
        .and_then(|value| value.parse().ok())
        .filter(|value| CHOICES.contains(value))
        .unwrap_or(DEFAULT)
}

fn interval() -> Signal<usize> {
    INTERVAL.with(|slot| {
        *slot.get_or_init(|| {
            Signal::new_in(
                day::reactive::Scope::root(),
                CHOICES
                    .iter()
                    .position(|value| *value == stored_minutes())
                    .unwrap_or(0),
            )
        })
    })
}

pub fn stop() {
    next().set(None);
    TIMER.with(|timer| {
        if let Some(handle) = timer.borrow_mut().take() {
            handle.abort();
        }
    });
}

pub fn start() {
    stop();
    let minutes = CHOICES[interval().get_untracked()];
    if minutes == 0 {
        return;
    }
    let (handle, registration) = AbortHandle::new_pair();
    TIMER.with(|timer| *timer.borrow_mut() = Some(handle));
    day::task(async move {
        let _ = Abortable::new(
            async move {
                if minutes == AUTOMATIC {
                    daynews_core::refresh_due();
                }
                loop {
                    let delay = if minutes == AUTOMATIC {
                        60_000
                    } else {
                        (minutes * 60 * 1000) as u32
                    };
                    next().set(Some(
                        daynews_time::now_unix().saturating_add(i64::from(delay / 1000)),
                    ));
                    day::sleep(delay).await;
                    if minutes == AUTOMATIC {
                        daynews_core::refresh_due();
                    } else {
                        daynews_core::refresh_all();
                    }
                }
            },
            registration,
        )
        .await;
    });
}

#[derive(Clone, Copy)]
pub struct Interval;
impl day::prelude::Binding<usize> for Interval {
    fn read(&self) -> usize {
        interval().get()
    }
    fn peek(&self) -> usize {
        interval().get_untracked()
    }
    fn write(&self, index: usize) {
        if let Some(value) = CHOICES.get(index) {
            day::prefs::set(KEY, &value.to_string());
            interval().set(index);
            start();
        }
    }
}

fn next() -> Signal<Option<i64>> {
    NEXT.with(|slot| *slot.get_or_init(|| Signal::new_in(day::reactive::Scope::root(), None)))
}
/// Actual scheduling mode and fixed timer deadline, for the dashboard's estimate.
pub fn schedule() -> (u64, Option<i64>) {
    (CHOICES[interval().get()], next().get())
}
pub fn automatic(value: u64) -> bool {
    value == AUTOMATIC
}
