//! Reader operations shared by native chrome and content fallbacks.
use crate::res;
use day::prelude::*;

pub(crate) fn refresh() -> CommandHandle {
    Command {
        id: "refresh",
        label: res::str::refresh_action(),
        action: daynews_core::refresh_all,
    }
    .build()
    .icon(Symbol::Refresh)
    .shortcut(Shortcut::new("r"))
}

pub(crate) fn mark_all_read() -> CommandHandle {
    Command {
        id: "mark-all-read",
        label: res::str::mark_all_read(),
        action: || daynews_core::mark_scope_read(true),
    }
    .build()
    .icon(Symbol::Check)
    .enabled(|| daynews_core::state().total_unread.get() > 0)
}
