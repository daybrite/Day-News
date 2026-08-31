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

use day_core::Ambient;
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

/// The app's DATA (docs/state.md): the subscription tree, the smart-feed badges, and the state
/// of a refresh. One database, one set of feeds, one unread count — a second window is another
/// view of the same reader, not a second reader.
#[derive(Clone, Copy)]
pub struct SheetsState {
    pub feeds: Signal<Vec<FeedRow>>,
    pub folders: Signal<Vec<FolderRow>>,
    pub tags: Signal<Vec<TagRow>>,
    /// `Some((done, total))` while a refresh runs — the progress NetNewsWire shows in its bar.
    pub refresh_progress: Signal<Option<(usize, usize)>>,
    pub total_unread: Signal<i64>,
    pub total_starred: Signal<i64>,
    pub total_today: Signal<i64>,
    /// A short transient message ("Imported 145 feeds", "3 feeds failed").
    pub status: Signal<String>,
}

/// Everything ONE WINDOW is looking at (docs/state.md): its sidebar scope, its search text, the
/// timeline those two produce, and the article it has open.
///
/// Per-window because that is what a second window is FOR — one on Unread while another sits in
/// a folder, each with its own selection and its own reader. The timeline query behind it is
/// per-window too: `create` stands up one live query and the effects that publish from it, and
/// those effects hold it (the container keeps a live query weakly, so a dropped one goes quiet).
#[derive(Clone, Copy)]
pub struct NewsScene {
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
    /// Articles marked read while the UNREAD scope shows them: they stay visible (their dot
    /// clears in place) until the scope or search changes — NetNewsWire's rule. The timeline
    /// fetch ORs these ids back into the unread predicate.
    sticky_read: Signal<Vec<u64>>,
}

/// The open store and the query handles derived from it — one per PROCESS, held for the app's
/// lifetime. Not app STATE (nothing here is a signal an app reads): a database connection, an
/// undo history, and the count queries whose badges feed `SheetsState`. `Ambient::app` owns the
/// signals; this owns the resources behind them.
struct Store {
    db: RefCell<Option<Db>>,
    undo: OnceCell<day_model::UndoStack>,
    /// Per-feed and per-tag unread badges, created on first sight and reused — each is one
    /// live `SELECT COUNT(*)` that re-runs only when a change touches its dependency set.
    feed_counts: RefCell<HashMap<u64, CountQuery<Article>>>,
    tag_counts: RefCell<HashMap<u64, CountQuery<Article>>>,
    totals: OnceCell<[CountQuery<Article>; 2]>,
}

#[derive(Clone)]
struct StoreHandle(std::rc::Rc<Store>);

impl Ambient for StoreHandle {
    fn create() -> Self {
        StoreHandle(std::rc::Rc::new(Store {
            db: RefCell::new(None),
            undo: OnceCell::new(),
            feed_counts: RefCell::new(HashMap::new()),
            tag_counts: RefCell::new(HashMap::new()),
            totals: OnceCell::new(),
        }))
    }
}

/// The process's one store handle. App-scoped like everything else here (docs/state.md), so
/// there is no `thread_local!` left in this crate at all.
fn store() -> std::rc::Rc<Store> {
    StoreHandle::app().0
}

impl Ambient for SheetsState {
    /// Created on the reactive ROOT scope by `Ambient::app` — which is what the detached scope
    /// this replaces existed for: it outlives any UI subtree.
    fn create() -> Self {
        SheetsState {
            feeds: Signal::new(Vec::new()),
            folders: Signal::new(Vec::new()),
            tags: Signal::new(Vec::new()),
            refresh_progress: Signal::new(None),
            total_unread: Signal::new(0),
            total_starred: Signal::new(0),
            total_today: Signal::new(0),
            status: Signal::new(String::new()),
        }
    }
}

/// The app's data. One reader, one database, one set of badges.
pub fn state() -> SheetsState {
    SheetsState::app()
}

impl Ambient for NewsScene {
    /// One window's view. Stands up THIS window's timeline query and the effects that publish
    /// from it — the effects capture the query, which is what keeps it subscribed (the container
    /// holds a live query weakly).
    fn create() -> Self {
        let scene = NewsScene {
            scope: Signal::new(Scope::Unread),
            articles: Signal::new(Vec::new()),
            selected: Signal::new(None),
            article: Signal::new(None),
            reader_open: Signal::new(false),
            search: Signal::new(String::new()),
            sticky_read: Signal::new(Vec::new()),
        };
        wire_scene(scene);
        scene
    }
}

/// The window whose view a call belongs to: the ambient one while a piece BUILDS, the FOCUSED
/// window's when a command runs later from a handler that belongs to no scope (docs/state.md).
pub fn scene() -> NewsScene {
    NewsScene::try_ambient()
        .or_else(NewsScene::focused)
        .expect("no window is open, so there is no NewsScene to act on")
}

/// Stand up one window's timeline: the live query that follows its scope + search, and the
/// effects that publish rows and the open article from it.
fn wire_scene(sc: NewsScene) {
    // No store (open failed): the window still builds, empty.
    let Some(timeline) = with_db(|db| {
        db.container.query_fn::<Article>(move || {
            let mut fetch = timeline_fetch(sc.scope.get(), &sc.search.get(), TIMELINE_LIMIT);
            let sticky = sc.sticky_read.get();
            if sc.scope.get() == Scope::Unread && !sticky.is_empty() {
                // Re-admit the rows read under the cursor, so they clear in place instead of
                // vanishing (the fetch rebuilds from scratch, so this replaces the filter).
                fetch = timeline_fetch(Scope::All, &sc.search.get(), TIMELINE_LIMIT);
                fetch.pred = fetch.pred
                    & (daynews_db::scope_pred(Scope::Unread) | day_persistence::Pred::IdIn(sticky));
            }
            fetch
        })
    }) else {
        return;
    };

    // Timeline rows: ids from the query, fields read TRACKED so an edit to a visible row
    // (a star, a read dot) rebuilds exactly this list. The closure holds `timeline`.
    Effect::new(move || {
        let rows = build_summaries(&timeline);
        sc.articles.set(rows);
    });

    // The reader: whatever article is selected, kept current as its fields change (a
    // star from the toolbar repaints the open article without any hand patching).
    Effect::new(move || {
        let article = sc.selected.get().and_then(build_reader_article);
        sc.article.set(article);
    });

    // Closing the reader (the platform's back on a phone) drops the selection with it —
    // the row un-highlights, and reopening starts from the list.
    watch(
        move || sc.reader_open.get(),
        move |open, _| {
            if !open {
                sc.selected.set(None);
            }
        },
    );
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
            store_with(|s| *s.db.borrow_mut() = Some(db));
            store_with(|s| {
                let _ = s
                    .undo
                    .set(with_db(|db| db.container.undo(UNDO_LEVELS)).expect("db just set"));
            });
            wire_app_state();
        }
        Err(e) => state()
            .status
            .set(format!("Could not open the article store: {e}")),
    }
}

/// Run `f` against the store. A closed store (open failed) makes this a no-op, so the UI keeps
/// working — empty — rather than panicking.
fn with_db<R>(f: impl FnOnce(&Db) -> R) -> Option<R> {
    store_with(|s| s.db.borrow().as_ref().map(f))
}

/// The container's undo history — `day::install_undo` wires it to the platform.
pub fn undo_stack() -> Option<day_model::UndoStack> {
    store_with(|s| s.undo.get().cloned())
}

// ---- the live pipeline ----------------------------------------------------------------------

/// Stand up the standing effects that DERIVE every published signal from live queries.
/// Stand up the APP-wide standing effects: the subscription tree and the smart-feed badges.
/// A window's own timeline is `wire_scene`, run once per window.
fn wire_app_state() {
    let st = state();
    // On the ROOT scope: these outlive every window (the detached scope this replaces existed
    // for the same reason).
    let scope = RScope::root();

    // Totals: unread and starred are plain count queries; Today re-derives on scope of its
    // predicate (its midnight cutoff re-evaluates whenever the count query re-derives, and a
    // refresh nudges it across midnight).
    let (unread_total, starred_total) =
        with_db(|db| (db.unread_count(Scope::All), db.count(Scope::Starred))).expect("open");
    store_with(|s| {
        let _ = s.totals.set([unread_total.clone(), starred_total.clone()]);
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
    });
}

fn build_summaries(q: &Query<Article>) -> Vec<ArticleSummary> {
    let Some(db) = store_with(|s| s.db.borrow().as_ref().map(|d| d.container.clone())) else {
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
    let Some(db) = store_with(|s| s.db.borrow().as_ref().map(|d| d.container.clone())) else {
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
            let unread = store_with(|s| {
                s.feed_counts
                    .borrow_mut()
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
    let Some(db) = store_with(|s| s.db.borrow().as_ref().map(|d| d.container.clone())) else {
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
    let Some(db) = store_with(|s| s.db.borrow().as_ref().map(|d| d.container.clone())) else {
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
            let count = store_with(|s| {
                s.tag_counts
                    .borrow_mut()
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

pub fn select_scope(new_scope: Scope) {
    let sc = scene();
    sc.scope.set(new_scope);
    sc.selected.set(None);
    sc.reader_open.set(false);
    sc.sticky_read.set(Vec::new());
}

pub fn set_search(text: &str) {
    let sc = scene();
    sc.search.set(text.to_string());
    sc.sticky_read.set(Vec::new());
}

/// Open an article. Reading it marks it read, like every reader does.
pub fn open_article(id: u64) {
    let sc = scene();
    sc.selected.set(Some(id));
    sc.reader_open.set(true);
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
    let sc = scene();
    let rows = sc.articles.get_untracked();
    let current = sc.selected.get_untracked();
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
    // The sticky set belongs to the window that did the reading — only ITS unread timeline
    // should keep the row visible.
    let sc = scene();
    if read && sc.scope.get_untracked() == Scope::Unread {
        // Keep the row visible in the unread timeline until the scope changes.
        let mut sticky = sc.sticky_read.get_untracked();
        if !sticky.contains(&id) {
            sticky.push(id);
            sc.sticky_read.set(sticky);
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
    let in_scope = scene().scope.get_untracked();
    with_db(|d| d.set_read_all(in_scope, read));
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
    // The window that removed the feed cannot stay scoped to it.
    let sc = scene();
    if sc.scope.get_untracked() == Scope::Feed(feed) {
        sc.scope.set(Scope::Unread);
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
        // wasm has no filesystem for the resource opener to read; the web dist serves the
        // same bundle over HTTP instead (`resource/assets/` staged under `assets/data/`,
        // day-cli web.rs), so the asset rides the ordinary fetch path as a same-origin URL.
        #[cfg(target_arch = "wasm32")]
        {
            // Fetch by the RELATIVE dist URL, parse against the absolute `asset:` base —
            // the parser's URL resolution rejects a relative base outright.
            return daynews_feed::fetch_with_base(&format!("assets/data/{name}"), url).await;
        }
        #[cfg(not(target_arch = "wasm32"))]
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

/// Run `f` against the process store — the shape the old `thread_local!` accessor had, so every
/// call site below reads the same.
fn store_with<R>(f: impl FnOnce(&Store) -> R) -> R {
    f(&store())
}
