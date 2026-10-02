//! One app-wide refresh timer, independent of the preferences window's lifetime.
use day::prelude::Signal;
use futures_util::future::{AbortHandle, Abortable};
use std::cell::{OnceCell, RefCell};

const KEY: &str = "news.refresh.minutes";
pub const CHOICES: [u64; 6] = [0, 30, 60, 120, 240, 480];
const DEFAULT: u64 = 120;

thread_local! {
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
                    .unwrap_or(3),
            )
        })
    })
}

pub fn stop() {
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
                loop {
                    day::sleep((minutes * 60 * 1000) as u32).await;
                    daynews_core::refresh_all();
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
