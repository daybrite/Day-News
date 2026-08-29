//! The app's view-model: opens the container, wires LIVE queries, and publishes what the UI
//! renders as reactive signals — every one of them DERIVED. There is no reload call anywhere:
//! a write lands in the store, the affected queries re-derive, the standing effects rebuild
//! exactly the signals whose sources moved, and the UI follows. Undo, a background refresh,
//! and a menu command all reach the screen through the same road.
//!
//! Single-threaded by design. day's reactive core is `!Send` and its executor (`day::task`)
//! polls futures on the UI thread, so the container lives here in a `RefCell` and every
//! mutation happens between awaits — no marshaling, no locks. Network I/O is the only
//! off-thread part, and the HTTP part hands the response back on the main thread.

use std::cell::{OnceCell, RefCell};
use std::collections::HashMap;
use std::path::PathBuf;

use day_persistence::{CountQuery, Query};
use day_reactive::{Effect, Scope as RScope, Signal, watch};
use daynews_db::{
    Article, ArticleFields, ArticleRelations, Db, FeedFields, FolderFields, IncomingArticle, Scope,
    TagFields, timeline_fetch,
};

pub use daynews_db::{Scope as TimelineScope, article_id, feed_id, folder_id, tag_id};

/// How many articles the timeline shows at once. The query is windowed (`LIMIT`), so this caps
/// what the UI materializes, not what the store holds.
const TIMELINE_LIMIT: usize = 500;

/// How deep the undo history goes.
const UNDO_LEVELS: usize = 200;

/// The retention default: prune read, unstarred, untagged articles older than this many days.
/// `0` means keep everything; the app stores the user's choice in its preferences.
pub const DEFAULT_RETENTION_DAYS: u32 = 90;

/// A sidebar row: the feed plus the badge count the UI draws.
#[derive(Debug, Clone, PartialEq)]
pub struct FeedRow {
    pub id: u64,
    pub title: String,
    pub unread: i64,
    pub folder_id: Option<u64>,
    pub has_error: bool,
    pub site_url: Option<String>,
    pub feed_url: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FolderRow {
    pub id: u64,
    pub name: String,
}

/// A sidebar tag row with its article count.
#[derive(Debug, Clone, PartialEq)]
pub struct TagRow {
    pub id: u64,
    pub name: String,
    pub count: i64,
}

/// A timeline row. Deliberately excludes the body: it lives in its own model
/// (`ArticleBody`) and faults in only when the reader opens the article.
#[derive(Debug, Clone, PartialEq)]
pub struct ArticleSummary {
    pub id: u64,
    pub feed_id: u64,
    pub feed_title: String,
    pub title: Option<String>,
    pub url: Option<String>,
    pub author: Option<String>,
    pub published_at: i64,
    pub summary: Option<String>,
    pub is_read: bool,
    pub is_starred: bool,
}

/// A full article, body included, for the reader pane.
#[derive(Debug, Clone, PartialEq)]
pub struct StoredArticle {
    pub id: u64,
    pub feed_id: u64,
    pub feed_title: String,
    pub title: Option<String>,
    pub url: Option<String>,
    pub author: Option<String>,
    pub published_at: i64,
    pub summary: Option<String>,
    pub content_html: Option<String>,
    pub is_read: bool,
    pub is_starred: bool,
}

pub use daynews_opml as opml;

/// Everything the UI observes. All fields are `Copy` signals; read them on the main thread.
#[derive(Clone, Copy)]
pub struct SheetsState {
    pub feeds: Signal<Vec<FeedRow>>,
    pub folders: Signal<Vec<FolderRow>>,
    pub tags: Signal<Vec<TagRow>>,
    pub scope: Signal<Scope>,
    pub articles: Signal<Vec<ArticleSummary>>,
    /// The open article's id, and its loaded body.
    pub selected: Signal<Option<u64>>,
    pub article: Signal<Option<StoredArticle>>,
    /// Whether the reader is SHOWING — the selector's `detail_visible` binding. On a phone
    /// this is the push gate for the reader page, and the platform's back writes it false;
    /// wide layouts keep the reader pane on screen and ignore it.
    pub reader_open: Signal<bool>,
    pub search: Signal<String>,
    /// `Some((done, total))` while a refresh runs — the progress NetNewsWire shows in its bar.
    pub refresh_progress: Signal<Option<(usize, usize)>>,
    pub total_unread: Signal<i64>,
    pub total_starred: Signal<i64>,
    pub total_today: Signal<i64>,
    /// A short transient message ("Imported 145 feeds", "3 feeds failed").
    pub status: Signal<String>,
    /// Articles marked read while the UNREAD scope shows them: they stay visible (their dot
    /// clears in place) until the scope or search changes — NetNewsWire's rule. The timeline
    /// fetch ORs these ids back into the unread predicate.
    sticky_read: Signal<Vec<u64>>,
}

thread_local! {
    static DB: RefCell<Option<Db>> = const { RefCell::new(None) };
    static STATE: OnceCell<SheetsState> = const { OnceCell::new() };
    static TIMELINE: OnceCell<Query<Article>> = const { OnceCell::new() };
    static UNDO: OnceCell<day_model::UndoStack> = const { OnceCell::new() };
    /// Per-feed and per-tag unread badges, created on first sight and reused — each is one
    /// live `SELECT COUNT(*)` that re-runs only when a change touches its dependency set.
    static FEED_COUNTS: RefCell<HashMap<u64, CountQuery<Article>>> = RefCell::new(HashMap::new());
    static TAG_COUNTS: RefCell<HashMap<u64, CountQuery<Article>>> = RefCell::new(HashMap::new());
    static TOTALS: OnceCell<[CountQuery<Article>; 2]> = const { OnceCell::new() };
}

/// The reactive state, created once in a detached scope so it outlives any UI subtree.
pub fn state() -> SheetsState {
    STATE.with(|c| {
        *c.get_or_init(|| {
            RScope::detached().enter(|| SheetsState {
                feeds: Signal::new(Vec::new()),
                folders: Signal::new(Vec::new()),
                tags: Signal::new(Vec::new()),
                scope: Signal::new(Scope::Unread),
                articles: Signal::new(Vec::new()),
                selected: Signal::new(None),
                article: Signal::new(None),
                reader_open: Signal::new(false),
                search: Signal::new(String::new()),
                refresh_progress: Signal::new(None),
                total_unread: Signal::new(0),
                total_starred: Signal::new(0),
                total_today: Signal::new(0),
                status: Signal::new(String::new()),
                sticky_read: Signal::new(Vec::new()),
            })
        })
    })
}

/// Open the container and stand up the live pipeline. Call once at startup, before the first
/// build.
pub fn init() {
    let dir = store_dir();
    // The diesel-era store is a different schema; the redesign starts fresh, deliberately.
    for legacy in ["sheets.sqlite3", "sheets.sqlite3-wal", "sheets.sqlite3-shm"] {
        let _ = std::fs::remove_file(dir.join(legacy));
    }
    match Db::open(&dir.join("sheets2.sqlite3")) {
        Ok(db) => {
            DB.with(|c| *c.borrow_mut() = Some(db));
            UNDO.with(|c| {
                let _ = c.set(with_db(|db| db.container.undo(UNDO_LEVELS)).expect("db just set"));
            });
            wire_live_state();
        }
        Err(e) => state()
            .status
            .set(format!("Could not open the article store: {e}")),
    }
}

/// Run `f` against the store. A closed store (open failed) makes this a no-op, so the UI keeps
/// working — empty — rather than panicking.
fn with_db<R>(f: impl FnOnce(&Db) -> R) -> Option<R> {
    DB.with(|c| c.borrow().as_ref().map(f))
}

/// The container's undo history — `day::install_undo` wires it to the platform.
pub fn undo_stack() -> Option<day_model::UndoStack> {
    UNDO.with(|c| c.get().cloned())
}

// ---- the live pipeline ----------------------------------------------------------------------

/// Stand up the standing effects that DERIVE every published signal from live queries.
fn wire_live_state() {
    let st = state();
    let scope = RScope::detached();

    // The timeline query follows the sidebar scope, the search text, and the sticky read ids.
    let timeline = with_db(|db| {
        db.container.query_fn::<Article>(move || {
            let mut fetch = timeline_fetch(st.scope.get(), &st.search.get(), TIMELINE_LIMIT);
            let sticky = st.sticky_read.get();
            if st.scope.get() == Scope::Unread && !sticky.is_empty() {
                // Re-admit the rows read under the cursor, so they clear in place instead of
                // vanishing (the fetch rebuilds from scratch, so this replaces the filter).
                fetch = timeline_fetch(Scope::All, &st.search.get(), TIMELINE_LIMIT);
                fetch.pred = fetch.pred
                    & (daynews_db::scope_pred(Scope::Unread) | day_persistence::Pred::IdIn(sticky));
            }
            fetch
        })
    })
    .expect("wire_live_state runs only with an open db");
    TIMELINE.with(|c| {
        let _ = c.set(timeline.clone());
    });

    // Totals: unread and starred are plain count queries; Today re-derives on scope of its
    // predicate (its midnight cutoff re-evaluates whenever the count query re-derives, and a
    // refresh nudges it across midnight).
    let (unread_total, starred_total) =
        with_db(|db| (db.unread_count(Scope::All), db.count(Scope::Starred))).expect("open");
    TOTALS.with(|c| {
        let _ = c.set([unread_total.clone(), starred_total.clone()]);
    });
    let today_total = with_db(|db| db.unread_count(Scope::Today)).expect("open");

    // The sidebar lists are standing queries too: the container holds a live query WEAKLY, so
    // one created inside an effect run and dropped at its end takes the subscription with it
    // and the list goes quiet. These live for the session, like the timeline's.
    let (feeds_q, folders_q, tags_q) = with_db(|db| {
        (
            db.container
                .query::<daynews_db::Feed>()
                .sort(daynews_db::Feed::position().asc())
                .sort(daynews_db::Feed::title().asc())
                .live(),
            db.container
                .query::<daynews_db::Folder>()
                .sort(daynews_db::Folder::position().asc())
                .sort(daynews_db::Folder::name().asc())
                .live(),
            db.container
                .query::<daynews_db::Tag>()
                .sort(daynews_db::Tag::name().asc())
                .live(),
        )
    })
    .expect("open");

    scope.enter(|| {
        // Timeline rows: ids from the query, fields read TRACKED so an edit to a visible row
        // (a star, a read dot) rebuilds exactly this list.
        Effect::new(move || {
            let rows = build_summaries(&timeline);
            st.articles.set(rows);
        });

        // Sidebar: feeds with their badges, folders, tags with theirs.
        Effect::new(move || {
            let rows = build_feed_rows(&feeds_q);
            st.feeds.set(rows);
        });
        Effect::new(move || {
            let rows = build_folder_rows(&folders_q);
            st.folders.set(rows);
        });
        Effect::new(move || {
            let rows = build_tag_rows(&tags_q);
            st.tags.set(rows);
        });

        // The smart-feed badges.
        Effect::new(move || st.total_unread.set(unread_total.get() as i64));
        Effect::new(move || st.total_starred.set(starred_total.get() as i64));
        Effect::new(move || st.total_today.set(today_total.get() as i64));

        // The reader: whatever article is selected, kept current as its fields change (a
        // star from the toolbar repaints the open article without any hand patching).
        Effect::new(move || {
            let article = st.selected.get().and_then(build_reader_article);
            st.article.set(article);
        });

        // Closing the reader (the platform's back on a phone) drops the selection with it —
        // the row un-highlights, and reopening starts from the list.
        watch(
            move || st.reader_open.get(),
            move |open, _| {
                if !open {
                    st.selected.set(None);
                }
            },
        );
    });
}

fn build_summaries(q: &Query<Article>) -> Vec<ArticleSummary> {
    let Some(db) = DB.with(|c| c.borrow().as_ref().map(|d| d.container.clone())) else {
        return Vec::new();
    };
    let ids = q.ids();
    let keys: Vec<u64> = ids.iter().map(|i| i.handle()).collect();
    let _ = db.ensure_resident::<Article>(&keys);
    let articles = db.cache::<Article>();
    // The window's feeds too, for the footer titles.
    let feed_keys: Vec<u64> = articles.with_untracked(|k| {
        keys.iter()
            .filter_map(|id| k.get(*id).and_then(|a| a.feed.id()))
            .map(|i| i.handle())
            .collect()
    });
    let _ = db.ensure_resident::<daynews_db::Feed>(&feed_keys);
    let feeds = db.cache::<daynews_db::Feed>();

    keys.iter()
        .filter_map(|id| {
            let a = articles.elem(*id);
            if !a.exists() {
                return None;
            }
            let feed_key = a
                .feed()
                .with(|f| f.and_then(|f| f.id()))
                .map(|i| i.handle());
            let feed_title = feed_key
                .map(|fk| {
                    feeds
                        .elem(fk)
                        .title()
                        .with(|t| t.cloned().unwrap_or_default())
                })
                .unwrap_or_default();
            Some(ArticleSummary {
                id: *id,
                feed_id: feed_key.unwrap_or(0),
                feed_title,
                title: a.title().with(|v| v.cloned().flatten()),
                url: a.url().with(|v| v.cloned().flatten()),
                author: a.author().with(|v| v.cloned().flatten()),
                published_at: a.published_at().with(|v| v.copied().unwrap_or(0)),
                summary: a.summary().with(|v| v.cloned().flatten()),
                is_read: a.is_read().with(|v| v.copied().unwrap_or(false)),
                is_starred: a.is_starred().with(|v| v.copied().unwrap_or(false)),
            })
        })
        .collect()
}

fn build_feed_rows(q: &Query<daynews_db::Feed>) -> Vec<FeedRow> {
    let Some(db) = DB.with(|c| c.borrow().as_ref().map(|d| d.container.clone())) else {
        return Vec::new();
    };
    let ids = q.ids();
    let keys: Vec<u64> = ids.iter().map(|i| i.handle()).collect();
    let _ = db.ensure_resident::<daynews_db::Feed>(&keys);
    let feeds = db.cache::<daynews_db::Feed>();
    keys.iter()
        .filter_map(|id| {
            let f = feeds.elem(*id);
            if !f.exists() {
                return None;
            }
            let unread = FEED_COUNTS.with(|c| {
                c.borrow_mut()
                    .entry(*id)
                    .or_insert_with(|| with_db(|d| d.unread_count(Scope::Feed(*id))).expect("open"))
                    .get() as i64
            });
            Some(FeedRow {
                id: *id,
                title: f.title().with(|t| t.cloned().unwrap_or_default()),
                unread,
                folder_id: f
                    .folder()
                    .with(|v| v.copied().flatten())
                    .and_then(|o| o.id())
                    .map(|i| i.handle()),
                has_error: f.last_error().with(|v| v.cloned().flatten()).is_some(),
                site_url: f.site_url().with(|v| v.cloned().flatten()),
                feed_url: f.feed_url().with(|v| v.cloned().unwrap_or_default()),
            })
        })
        .collect()
}

fn build_folder_rows(q: &Query<daynews_db::Folder>) -> Vec<FolderRow> {
    let Some(db) = DB.with(|c| c.borrow().as_ref().map(|d| d.container.clone())) else {
        return Vec::new();
    };
    let keys: Vec<u64> = q.ids().iter().map(|i| i.handle()).collect();
    let _ = db.ensure_resident::<daynews_db::Folder>(&keys);
    let folders = db.cache::<daynews_db::Folder>();
    keys.iter()
        .filter_map(|id| {
            let f = folders.elem(*id);
            f.exists().then(|| FolderRow {
                id: *id,
                name: f.name().with(|n| n.cloned().unwrap_or_default()),
            })
        })
        .collect()
}

fn build_tag_rows(q: &Query<daynews_db::Tag>) -> Vec<TagRow> {
    let Some(db) = DB.with(|c| c.borrow().as_ref().map(|d| d.container.clone())) else {
        return Vec::new();
    };
    let keys: Vec<u64> = q.ids().iter().map(|i| i.handle()).collect();
    let _ = db.ensure_resident::<daynews_db::Tag>(&keys);
    let tags = db.cache::<daynews_db::Tag>();
    keys.iter()
        .filter_map(|id| {
            let t = tags.elem(*id);
            if !t.exists() {
                return None;
            }
            let count = TAG_COUNTS.with(|c| {
                c.borrow_mut()
                    .entry(*id)
                    .or_insert_with(|| with_db(|d| d.count(Scope::Tag(*id))).expect("open"))
                    .get() as i64
            });
            Some(TagRow {
                id: *id,
                name: t.name().with(|n| n.cloned().unwrap_or_default()),
                count,
            })
        })
        .collect()
}

fn build_reader_article(id: u64) -> Option<StoredArticle> {
    let a = with_db(|d| d.container.get::<Article>(id))??;
    let feed_key = a
        .feed()
        .with(|f| f.and_then(|f| f.id()))
        .map(|i| i.handle());
    let feed_title = feed_key
        .and_then(|fk| {
            with_db(|d| d.container.get::<daynews_db::Feed>(fk))
                .flatten()
                .map(|f| f.title().with(|t| t.cloned().unwrap_or_default()))
        })
        .unwrap_or_default();
    let content_html = with_db(|d| d.body(id)).flatten();
    Some(StoredArticle {
        id,
        feed_id: feed_key.unwrap_or(0),
        feed_title,
        title: a.title().with(|v| v.cloned().flatten()),
        url: a.url().with(|v| v.cloned().flatten()),
        author: a.author().with(|v| v.cloned().flatten()),
        published_at: a.published_at().with(|v| v.copied().unwrap_or(0)),
        summary: a.summary().with(|v| v.cloned().flatten()),
        content_html,
        is_read: a.is_read().with(|v| v.copied().unwrap_or(false)),
        is_starred: a.is_starred().with(|v| v.copied().unwrap_or(false)),
    })
}

// ---- selection ------------------------------------------------------------------------------

pub fn select_scope(scope: Scope) {
    let st = state();
    st.scope.set(scope);
    st.selected.set(None);
    st.reader_open.set(false);
    st.sticky_read.set(Vec::new());
}

pub fn set_search(text: &str) {
    let st = state();
    st.search.set(text.to_string());
    st.sticky_read.set(Vec::new());
}

/// Open an article. Reading it marks it read, like every reader does.
pub fn open_article(id: u64) {
    state().selected.set(Some(id));
    state().reader_open.set(true);
    let unread = with_db(|d| {
        d.container
            .get::<Article>(id)
            .map(|a| !a.is_read().with(|v| v.copied().unwrap_or(true)))
    })
    .flatten()
    .unwrap_or(false);
    if unread {
        set_read(id, true);
    }
}

/// Open the next unread article after the current one — NetNewsWire's ⌘/ .
///
/// Searches the visible timeline first (so it follows whatever the sidebar and search box have
/// filtered to), wrapping to the top; if nothing there is unread, falls back to the global
/// unread list so the shortcut still advances from a fully-read view.
pub fn open_next_unread() -> bool {
    let st = state();
    let rows = st.articles.get_untracked();
    let current = st.selected.get_untracked();
    let start = current
        .and_then(|id| rows.iter().position(|r| r.id == id))
        .map(|i| i + 1)
        .unwrap_or(0);
    // After the cursor, then wrapping around to the rows before it.
    let next = rows[start.min(rows.len())..]
        .iter()
        .chain(rows[..start.min(rows.len())].iter())
        .find(|r| !r.is_read && Some(r.id) != current)
        .map(|r| r.id);
    if let Some(id) = next {
        open_article(id);
        return true;
    }
    // Nothing unread in view: jump to the oldest unread anywhere, which is what a reader wants
    // when it has finished a feed.
    let anywhere = with_db(|d| {
        d.container
            .query::<Article>()
            .filter(daynews_db::scope_pred(Scope::Unread))
            .sort(Article::published_at().asc())
            .limit(1)
            .live()
            .first()
            .map(|i| i.handle())
    })
    .flatten();
    match anywhere {
        Some(id) => {
            select_scope(Scope::Unread);
            open_article(id);
            true
        }
        None => {
            state().status.set("No unread articles".into());
            false
        }
    }
}

pub fn set_read(id: u64, read: bool) {
    let st = state();
    if read && st.scope.get_untracked() == Scope::Unread {
        // Keep the row visible in the unread timeline until the scope changes.
        let mut sticky = st.sticky_read.get_untracked();
        if !sticky.contains(&id) {
            sticky.push(id);
            st.sticky_read.set(sticky);
        }
    }
    with_db(|d| d.set_read(id, read));
}

/// Flip one article's read flag — the row swipe, the toolbar toggle, and the menu item all
/// land here, so every affordance agrees on what "toggle" means.
pub fn toggle_read(id: u64) {
    let read = with_db(|d| {
        d.container
            .get::<Article>(id)
            .map(|a| a.is_read().with(|v| v.copied().unwrap_or(false)))
    })
    .flatten()
    .unwrap_or(false);
    set_read(id, !read);
}

pub fn set_starred(id: u64, starred: bool) {
    with_db(|d| d.set_starred(id, starred));
}

/// Toggle a named tag on an article, creating the tag on first use.
pub fn toggle_tag(article: u64, name: &str) {
    let name = name.trim();
    if name.is_empty() {
        return;
    }
    with_db(|d| {
        let tag = d.add_tag(name);
        let tagged = d
            .container
            .get::<Article>(article)
            .map(|a| a.tags().contains(tag))
            .unwrap_or(false);
        d.set_tagged(article, tag, !tagged);
    });
}

/// "Mark All as Read" for whatever the sidebar has selected.
pub fn mark_scope_read(read: bool) {
    let scope = state().scope.get_untracked();
    with_db(|d| d.set_read_all(scope, read));
}

pub fn mark_feed_read(feed: u64, read: bool) {
    with_db(|d| d.set_read_all(Scope::Feed(feed), read));
}

// ---- retention ------------------------------------------------------------------------------

/// Prune old read articles (starred and tagged stay). `0` keeps everything. The app calls
/// this at startup and when the setting shortens.
pub fn prune(days: u32) -> usize {
    if days == 0 {
        return 0;
    }
    with_db(|d| d.prune_older_than(days)).unwrap_or(0)
}

// ---- subscriptions --------------------------------------------------------------------------

/// Subscribe and immediately fetch, so the feed is named and populated without a manual refresh.
pub fn subscribe(url: &str) {
    let url = normalize_feed_url(url);
    if url.is_empty() {
        return;
    }
    let st = state();
    let Some(id) = with_db(|d| d.add_feed(&url, &fallback_title(&url), None)) else {
        st.status.set("Could not add the subscription.".into());
        return;
    };
    st.status
        .set(format!("Subscribed to {}", fallback_title(&url)));
    day_core::task(async move {
        refresh_one(id, url).await;
        state().refresh_progress.set(None);
    });
}

pub fn unsubscribe(feed: u64) {
    with_db(|d| d.delete_feed(feed));
    let st = state();
    if st.scope.get_untracked() == Scope::Feed(feed) {
        st.scope.set(Scope::Unread);
    }
}

/// Create a folder (idempotent by name) and report it.
pub fn create_folder(name: &str) -> Option<u64> {
    let id = with_db(|d| d.add_folder(name))?;
    state().status.set(format!("Created folder “{name}”"));
    Some(id)
}

pub fn rename_feed(feed: u64, title: &str) {
    with_db(|d| d.rename_feed(feed, title));
}

// ---- refreshing -----------------------------------------------------------------------------

/// Refresh every subscription, one at a time, publishing progress as it goes.
///
/// Sequential on purpose: each `await` returns to the main loop, so the UI stays responsive
/// and rows appear progressively, and a 145-feed import does not open 145 sockets at once.
pub fn refresh_all() {
    let st = state();
    if st.refresh_progress.get_untracked().is_some() {
        return; // already running
    }
    let feeds: Vec<(u64, String)> = st
        .feeds
        .get_untracked()
        .iter()
        .map(|f| (f.id, f.feed_url.clone()))
        .collect();
    if feeds.is_empty() {
        return;
    }
    let total = feeds.len();
    st.refresh_progress.set(Some((0, total)));
    day_core::task(async move {
        let mut failed = 0usize;
        for (i, (id, url)) in feeds.into_iter().enumerate() {
            if !refresh_one(id, url).await {
                failed += 1;
            }
            state().refresh_progress.set(Some((i + 1, total)));
        }
        let st = state();
        st.refresh_progress.set(None);
        st.status.set(match failed {
            0 => format!("Refreshed {total} feeds"),
            n => format!("Refreshed {} of {total} feeds — {n} failed", total - n),
        });
    });
}

/// Fetch and store one feed. Returns whether it succeeded.
/// A feed's bytes, parsed: over HTTP normally, or from the app bundle for `asset:` URLs —
/// the deterministic, network-free source the walkthrough seeds from on every platform
/// (dayscript/seed-fixtures.yaml subscribes to the parser fixtures bundled under
/// `resource/assets/fixtures/`). A missing asset reports as a 404 rather than a new error
/// arm — the subscription then shows the same failed-refresh state a dead feed does.
async fn fetch_feed(url: &str) -> Result<daynews_feed::ParsedFeed, daynews_feed::FeedError> {
    if let Some(name) = url.strip_prefix("asset:") {
        return match day_core::resource(day_core::AssetName::dynamic(name.to_string())) {
            Some(res) => daynews_feed::parse(res.as_slice(), url),
            None => Err(daynews_feed::FeedError::Status(404)),
        };
    }
    daynews_feed::fetch(url).await
}

async fn refresh_one(id: u64, url: String) -> bool {
    match fetch_feed(&url).await {
        Ok(parsed) => {
            let items: Vec<IncomingArticle> = parsed
                .items
                .iter()
                .map(|i| IncomingArticle {
                    guid: i.guid.clone(),
                    // Store the DISPLAY title so title-less microblog items are readable in the
                    // timeline and findable in search.
                    title: Some(i.display_title()),
                    url: i.url.clone(),
                    author: i.author.clone(),
                    published: i.published,
                    summary: i.summary.clone(),
                    content_html: i.content_html.clone(),
                })
                .collect();
            with_db(|d| {
                d.update_feed_metadata(
                    id,
                    parsed.title.as_deref(),
                    parsed.site_url.as_deref(),
                    parsed.description.as_deref(),
                    parsed.icon_url.as_deref(),
                );
                d.upsert_articles(id, &url, &items);
            });
            true
        }
        Err(e) => {
            with_db(|d| d.set_feed_error(id, &e.to_string()));
            false
        }
    }
}

// ---- OPML -----------------------------------------------------------------------------------

/// Import subscriptions, creating folders as needed. Returns (added, already present).
pub fn import_opml(text: &str) -> std::result::Result<(usize, usize), String> {
    let doc = daynews_opml::parse(text).map_err(|e| e.to_string())?;
    let entries = doc.feeds();
    let (mut added, mut existing) = (0usize, 0usize);
    with_db(|d| {
        for (path, feed) in &entries {
            // Only the innermost folder becomes a folder; deeper nesting is rare and flattening
            // it keeps the sidebar honest about what it can represent.
            let folder = path.last().map(|name| d.add_folder(name));
            let url = feed.xml_url.clone();
            if d.container.get::<daynews_db::Feed>(feed_id(&url)).is_some() {
                existing += 1;
                continue;
            }
            d.add_feed(&url, &feed.display_title(), folder);
            added += 1;
        }
    });
    state().status.set(match existing {
        0 => format!("Imported {added} feeds"),
        n => format!("Imported {added} feeds ({n} already subscribed)"),
    });
    Ok((added, existing))
}

/// Serialize the current subscriptions as OPML, grouped by folder.
pub fn export_opml() -> String {
    let st = state();
    let feeds = st.feeds.get_untracked();
    let folders = st.folders.get_untracked();
    let mut root: Vec<daynews_opml::Outline> = Vec::new();
    for folder in &folders {
        let children: Vec<daynews_opml::Outline> = feeds
            .iter()
            .filter(|f| f.folder_id == Some(folder.id))
            .map(to_outline)
            .collect();
        if !children.is_empty() {
            root.push(daynews_opml::Outline::Folder {
                title: folder.name.clone(),
                children,
            });
        }
    }
    root.extend(
        feeds
            .iter()
            .filter(|f| f.folder_id.is_none())
            .map(to_outline),
    );
    daynews_opml::write(&daynews_opml::Opml {
        title: Some("Day News Subscriptions".into()),
        outlines: root,
    })
    .unwrap_or_default()
}

fn to_outline(f: &FeedRow) -> daynews_opml::Outline {
    daynews_opml::Outline::Feed(daynews_opml::FeedRef {
        title: f.title.clone(),
        xml_url: f.feed_url.clone(),
        html_url: f.site_url.clone(),
    })
}

// ---- helpers --------------------------------------------------------------------------------

/// Accept what people paste: bare hosts get a scheme, and whitespace is trimmed.
pub fn normalize_feed_url(input: &str) -> String {
    let t = input.trim();
    // `asset:` is the bundled-fixture scheme (see `fetch_feed`): pass it through untouched.
    if t.starts_with("http://") || t.starts_with("https://") || t.starts_with("asset:") {
        t.to_string()
    } else if t.is_empty() {
        String::new()
    } else {
        format!("https://{t}")
    }
}

/// A provisional name for a brand-new subscription, replaced by the feed's own title on first
/// refresh — the same placeholder NetNewsWire shows.
fn fallback_title(url: &str) -> String {
    let rest = url.split_once("://").map(|(_, r)| r).unwrap_or(url);
    let host = rest.split(['/', '?', '#']).next().unwrap_or(rest);
    host.strip_prefix("www.").unwrap_or(host).to_string()
}

/// Where the SQLite file lives, per platform.
fn store_dir() -> PathBuf {
    base_dir()
}

#[cfg(not(any(target_os = "ios", target_os = "android", target_arch = "wasm32")))]
fn base_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join(".daybrite-sheets")
}

#[cfg(target_arch = "wasm32")]
fn base_dir() -> PathBuf {
    // Web: the "path" names an OPFS file, not a filesystem location — there is no $HOME and
    // `std::env::temp_dir` panics on wasm.
    PathBuf::from("daybrite-sheets")
}

#[cfg(target_os = "ios")]
fn base_dir() -> PathBuf {
    // `$HOME` is the sandbox container, whose root is not writable; Application Support is.
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("Library/Application Support/daybrite-sheets")
}

#[cfg(target_os = "android")]
fn base_dir() -> PathBuf {
    // The app's private files dir, via the JNI bridge. Resolved on the main thread (the only
    // thread this crate runs on), so the app classloader is reachable.
    android_files_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("daybrite-sheets")
}

#[cfg(target_os = "android")]
fn android_files_dir() -> Option<PathBuf> {
    use day_android::{DayEnv, as_jstring, read_jstring, with_env};
    const BRIDGE: &str = "dev/daybrite/day/bridge/DayBridge";
    with_env(|env| {
        let obj = env
            .dcall_static(BRIDGE, "filesDirPath", "()Ljava/lang/String;", &[])
            .ok()?
            .l()
            .ok()?;
        if obj.is_null() {
            return None;
        }
        let path = read_jstring(env, &as_jstring(obj))?;
        (!path.is_empty()).then(|| PathBuf::from(path))
    })
}
