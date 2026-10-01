//! UI signals stay on the UI thread; SQLite, migrations, FTS, materialization, and undo
//! live on one database worker. Committed, owned projections are the only bridge.
use day_core::Ambient;
use day_model::Source;
use day_persistence::{DatabaseWorker, DbError, ModelContainer, Pred};
use day_reactive::Binding;
use day_reactive::{Effect, Signal, batch, watch};
use daynews_db::{
    Article, ArticleFields, ArticleRelations, Db, IncomingArticle, Scope, timeline_fetch,
};
pub use daynews_db::{Scope as TimelineScope, article_id, feed_id, folder_id, tag_id};
use futures_util::{
    FutureExt, StreamExt,
    future::{LocalBoxFuture, Shared},
    stream,
};
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    path::PathBuf,
    rc::Rc,
};

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
    pub icon_url: Option<String>,
    pub has_fetched: bool,
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

/// A timeline row. Excludes the body: it lives in its own model
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

/// The app's data (docs/state.md): the subscription tree, the smart-feed badges, and the state
/// of a refresh. One database, one set of feeds, one unread count: a second window is another
/// view of the same reader, not a second reader.
#[derive(Clone, Copy)]
pub struct SheetsState {
    pub feeds: Signal<Vec<FeedRow>>,
    pub folders: Signal<Vec<FolderRow>>,
    pub tags: Signal<Vec<TagRow>>,
    /// `Some((done, total))` while a refresh runs: the progress NetNewsWire shows in its bar.
    pub refresh_progress: Signal<Option<(usize, usize)>>,
    /// Feeds with an active request, shared by every window.
    pub updating_feeds: Signal<HashMap<u64, Option<f64>>>,
    pub total_unread: Signal<i64>,
    pub total_starred: Signal<i64>,
    pub total_today: Signal<i64>,
    /// A short transient message ("Imported 145 feeds", "3 feeds failed").
    pub status: Signal<String>,
}

/// Everything one window is looking at (docs/state.md): its sidebar scope, its search text, the
/// timeline those two produce, and the article it has open.
///
/// Per-window because that is what a second window is for: one on Unread while another sits in
/// a folder, each with its own selection and its own reader. The timeline query behind it is
/// per-window too: `create` stands up one live query and the effects that publish from it, and
/// those effects hold it (the container keeps a live query weakly, so a dropped one goes quiet).
#[derive(Clone, Copy)]
pub struct NewsScene {
    pub scope: Signal<Scope>,
    pub articles: Signal<Vec<ArticleSummary>>,
    /// Unread count for the whole scope, including rows outside the timeline window.
    pub scope_unread: Signal<usize>,
    /// The open article's id, and its loaded body.
    pub selected: Signal<Option<u64>>,
    /// Two-way native timeline focus, requested by keyboard reading commands.
    pub timeline_focused: Signal<bool>,
    pub article: Signal<Option<StoredArticle>>,
    /// Whether the reader is showing: the selector's `detail_visible` binding. On a phone
    /// this is the push gate for the reader page, and the platform's back writes it false;
    /// wide layouts keep the reader pane on screen and ignore it.
    pub reader_open: Signal<bool>,
    pub search: Signal<String>,
    /// Articles marked read while the unread scope shows them: they stay visible (their dot
    /// clears in place) until the scope or search changes, which is NetNewsWire's rule. The
    /// timeline fetch ORs these ids back into the unread predicate.
    sticky_read: Signal<Vec<u64>>,
}

struct Store {
    worker: RefCell<Option<DatabaseWorker>>,
    format_status: RefCell<Option<fn(StatusMessage) -> String>>,
    ready: Signal<bool>,
    opening: RefCell<Option<Shared<LocalBoxFuture<'static, Result<DatabaseWorker, DbError>>>>>,
    refresh_slots: [futures_util::lock::Mutex<()>; 4],
    scheduled_feeds: RefCell<HashSet<u64>>,
    can_undo: Signal<bool>,
    can_redo: Signal<bool>,
}
#[derive(Clone)]
struct StoreHandle(Rc<Store>);
impl Ambient for StoreHandle {
    fn create() -> Self {
        Self(Rc::new(Store {
            worker: RefCell::new(None),
            format_status: RefCell::new(None),
            opening: RefCell::new(None),
            ready: Signal::new(false),
            refresh_slots: std::array::from_fn(|_| futures_util::lock::Mutex::new(())),
            scheduled_feeds: RefCell::new(HashSet::new()),
            can_undo: Signal::new(false),
            can_redo: Signal::new(false),
        }))
    }
}
fn store() -> Rc<Store> {
    StoreHandle::app().0
}
fn worker() -> Option<DatabaseWorker> {
    store().worker.borrow().clone()
}
fn status(message: StatusMessage) -> String {
    store()
        .format_status
        .borrow()
        .map(|format| format(message))
        .unwrap_or_default()
}
pub fn shutdown() {
    if let Some(worker) = worker() {
        if let Err(error) = worker.close_blocking() {
            report(error);
        }
    }
}
async fn ready_worker() -> Result<DatabaseWorker, DbError> {
    if let Some(worker) = worker() {
        return Ok(worker);
    }
    let opening = store().opening.borrow().clone();
    match opening {
        Some(opening) => opening.await,
        None => Err(DbError::new(
            day_persistence::DbErrorKind::Closed,
            "article store not initialized",
        )),
    }
}
// Error diagnostics are data. The application supplies a localized presentation on the UI.
#[derive(Clone, Copy)]
pub struct StorageError(pub Signal<Option<String>>);
impl Ambient for StorageError {
    fn create() -> Self {
        Self(Signal::new(None))
    }
}
fn report(error: DbError) {
    StorageError::app().0.set(Some(error.to_string()));
}

impl Ambient for SheetsState {
    fn create() -> Self {
        Self {
            feeds: Signal::new(vec![]),
            folders: Signal::new(vec![]),
            tags: Signal::new(vec![]),
            refresh_progress: Signal::new(None),
            updating_feeds: Signal::new(HashMap::new()),
            total_unread: Signal::new(0),
            total_starred: Signal::new(0),
            total_today: Signal::new(0),
            status: Signal::new(String::new()),
        }
    }
}
pub fn state() -> SheetsState {
    SheetsState::app()
}
impl Ambient for NewsScene {
    fn create() -> Self {
        let sc = Self {
            scope: Signal::new(Scope::Unread),
            articles: Signal::new(vec![]),
            scope_unread: Signal::new(0),
            selected: Signal::new(None),
            timeline_focused: Signal::new(false),
            article: Signal::new(None),
            reader_open: Signal::new(false),
            search: Signal::new(String::new()),
            sticky_read: Signal::new(vec![]),
        };
        wire_scene(sc);
        sc
    }
}
pub fn scene() -> NewsScene {
    try_scene().expect("no window is open")
}
pub fn try_scene() -> Option<NewsScene> {
    NewsScene::try_ambient().or_else(NewsScene::focused)
}

/// Own a task for precisely one effect run. Changing scope/search/selection cancels the old
/// subscription before starting a new one; a late result cannot replace the new projection.
fn scoped_task(future: impl std::future::Future<Output = ()> + 'static) {
    let task = day_core::task(future);
    day_reactive::on_run_retrack(move || task.abort());
}
fn wire_scene(sc: NewsScene) {
    let st = store();
    watch(
        move || sc.search.get(),
        move |_, _| sc.sticky_read.set(vec![]),
    );
    Effect::new(move || {
        if !st.ready.get() {
            return;
        }
        let Some(worker) = worker() else {
            return;
        };
        let scope = sc.scope.get();
        let search = sc.search.get();
        let sticky = sc.sticky_read.get();
        scoped_task(async move {
            let projection = worker
                .observe(move |db| timeline_snapshot(db, scope, &search, &sticky))
                .await;
            let mut projection = match projection {
                Ok(v) => v,
                Err(e) => {
                    report(e);
                    return;
                }
            };
            while let Some(result) = projection.next().await {
                match result {
                    Ok(snapshot) => batch(|| {
                        sc.articles.set_if_changed(snapshot.value.0);
                        sc.scope_unread.set_if_changed(snapshot.value.1);
                    }),
                    Err(e) => report(e),
                }
            }
        });
    });
    let st = store();
    Effect::new(move || {
        let ready = st.ready.get();
        let id = sc.selected.get();
        // Clear immediately: the previous article must never masquerade as the selection.
        sc.article.set(None);
        let (true, Some(id), Some(worker)) = (ready, id, worker()) else {
            return;
        };
        scoped_task(async move {
            let mut projection = match worker.observe(move |db| reader_snapshot(db, id)).await {
                Ok(v) => v,
                Err(e) => {
                    report(e);
                    return;
                }
            };
            while let Some(result) = projection.next().await {
                match result {
                    Ok(v) => sc.article.set_if_changed(v.value),
                    Err(e) => report(e),
                }
            }
        });
    });
    watch(
        move || sc.reader_open.get(),
        move |open, _| {
            if !open {
                sc.selected.set(None);
            }
        },
    );
}

pub enum StatusMessage {
    RefreshedFeed(String, bool),
    RefreshedFeeds(usize, usize),
    Imported(usize, usize),
    ExportTitle,
}
pub fn init(format_status: fn(StatusMessage) -> String) {
    let st = store();
    *st.format_status.borrow_mut() = Some(format_status);
    let path = store_dir().join("sheets2.sqlite3"); // Resolve platform directories on the UI thread.
    let can_undo = st.can_undo;
    let can_redo = st.can_redo;
    day_core::install_undo_bridge(
        can_undo,
        can_redo,
        Signal::new(String::new()),
        Signal::new(String::new()),
        |redo| {
            let Some(worker) = worker() else {
                return;
            };
            day_core::task(async move {
                if let Err(e) = worker.undo(redo).await {
                    report(e);
                }
                update_undo(&worker).await;
            });
        },
    );
    let opening = async move {
        let worker = DatabaseWorker::open(move || Db::open(&path).map(|db| db.container)).await?;
        worker.enable_undo(UNDO_LEVELS).await?;
        Ok(worker)
    }
    .boxed_local()
    .shared();
    *st.opening.borrow_mut() = Some(opening.clone());
    day_core::task(async move {
        let worker = match opening.await {
            Ok(worker) => worker,
            Err(error) => {
                report(error);
                return;
            }
        };
        *st.worker.borrow_mut() = Some(worker.clone());
        st.ready.set(true);
        let mut projection = match worker.observe(sidebar_snapshot).await {
            Ok(v) => v,
            Err(e) => {
                report(e);
                return;
            }
        };
        while let Some(result) = projection.next().await {
            match result {
                Ok(snapshot) => batch(|| {
                    let (feeds, folders, tags, unread, starred, today) = snapshot.value;
                    let state = state();
                    state.feeds.set_if_changed(feeds);
                    state.folders.set_if_changed(folders);
                    state.tags.set_if_changed(tags);
                    state.total_unread.set_if_changed(unread);
                    state.total_starred.set_if_changed(starred);
                    state.total_today.set_if_changed(today);
                }),
                Err(e) => report(e),
            }
        }
    });
}
async fn update_undo(worker: &DatabaseWorker) {
    match worker.undo_status().await {
        Ok((undo, redo)) => {
            let st = store();
            batch(|| {
                st.can_undo.set(undo);
                st.can_redo.set(redo);
            });
        }
        Err(e) => report(e),
    }
}
fn edit(f: impl FnOnce(&Db) -> Result<(), DbError> + Send + 'static) {
    day_core::task(async move {
        let worker = match ready_worker().await {
            Ok(w) => w,
            Err(e) => {
                report(e);
                return;
            }
        };
        if let Err(e) = worker
            .write(move |container| {
                f(&Db {
                    container: container.clone(),
                })
            })
            .await
        {
            report(e);
        }
        update_undo(&worker).await;
    });
}

type SidebarSnapshot = (Vec<FeedRow>, Vec<FolderRow>, Vec<TagRow>, i64, i64, i64);
fn sidebar_snapshot(db: &ModelContainer) -> Result<SidebarSnapshot, DbError> {
    use daynews_db::{Feed, Folder, Tag};
    let mut unread = HashMap::new();
    db.try_with_connection(|conn| {
        conn.query(
            "SELECT feed, COUNT(*) FROM articles WHERE is_read = 0 GROUP BY feed",
            &[],
            &mut |row| {
                if let (Ok(id), Ok(count)) = (row.get(0).as_int(), row.get(1).as_int()) {
                    unread.insert(id as u64, count);
                }
            },
        )
    })?;
    let feeds = db
        .query::<Feed>()
        .sort(Feed::position().asc())
        .sort(Feed::title().asc())
        .live()
        .try_collect()?
        .into_iter()
        .map(|f| FeedRow {
            id: f.id,
            title: f.title,
            unread: unread.get(&f.id).copied().unwrap_or(0),
            folder_id: f.folder.and_then(|f| f.id()).map(|id| id.handle()),
            has_error: f.last_error.is_some(),
            site_url: f.site_url,
            feed_url: f.feed_url,
            icon_url: f.icon_url,
            has_fetched: f.last_fetched_at.is_some(),
        })
        .collect();
    let folders = db
        .query::<Folder>()
        .sort(Folder::position().asc())
        .sort(Folder::name().asc())
        .live()
        .try_collect()?
        .into_iter()
        .map(|f| FolderRow {
            id: f.id,
            name: f.name,
        })
        .collect();
    let d = Db {
        container: db.clone(),
    };
    let tags = db
        .query::<Tag>()
        .sort(Tag::name().asc())
        .live()
        .try_collect()?
        .into_iter()
        .map(|t| {
            Ok(TagRow {
                id: t.id,
                name: t.name,
                count: d.count(Scope::Tag(t.id)).try_get()? as i64,
            })
        })
        .collect::<Result<_, DbError>>()?;
    Ok((
        feeds,
        folders,
        tags,
        unread.values().sum(),
        d.count(Scope::Starred).try_get()? as i64,
        d.unread_count(Scope::Today).try_get()? as i64,
    ))
}
fn timeline_snapshot(
    db: &ModelContainer,
    scope: Scope,
    search: &str,
    sticky: &[u64],
) -> Result<(Vec<ArticleSummary>, usize), DbError> {
    let mut fetch = timeline_fetch(scope, search, TIMELINE_LIMIT);
    if scope == Scope::Unread && !sticky.is_empty() {
        fetch = timeline_fetch(Scope::All, search, TIMELINE_LIMIT);
        fetch.pred =
            fetch.pred & (daynews_db::scope_pred(Scope::Unread) | Pred::IdIn(sticky.to_vec()));
    }
    let rows = db
        .query::<Article>()
        .filter(fetch.pred)
        .sort(Article::published_at().desc())
        .limit(TIMELINE_LIMIT)
        .live()
        .try_collect()?;
    let mut feeds = HashMap::new();
    let mut result = Vec::with_capacity(rows.len());
    for a in rows {
        let feed_id = a.feed.id().map(|v| v.handle()).unwrap_or(0);
        if !feeds.contains_key(&feed_id) {
            let title = db
                .try_get::<daynews_db::Feed>(feed_id)?
                .and_then(|f| f.with_value_untracked(|f| f.map(|f| f.title.clone())))
                .unwrap_or_default();
            feeds.insert(feed_id, title);
        }
        result.push(ArticleSummary {
            id: a.id,
            feed_id,
            feed_title: feeds[&feed_id].clone(),
            title: a.title,
            url: a.url,
            author: a.author,
            published_at: a.published_at,
            summary: a.summary,
            is_read: a.is_read,
            is_starred: a.is_starred,
        });
    }
    let count = db
        .query::<Article>()
        .filter(daynews_db::scope_pred(scope) & Article::is_read().eq(false))
        .live_count()
        .try_get()?;
    Ok((result, count))
}
fn reader_snapshot(db: &ModelContainer, id: u64) -> Result<Option<StoredArticle>, DbError> {
    let Some(a) = db
        .try_get::<Article>(id)?
        .and_then(|a| a.with_value_untracked(|a| a.cloned()))
    else {
        return Ok(None);
    };
    let feed_id = a.feed.id().map(|id| id.handle()).unwrap_or(0);
    let feed_title = db
        .try_get::<daynews_db::Feed>(feed_id)?
        .and_then(|f| f.with_value_untracked(|f| f.map(|f| f.title.clone())))
        .unwrap_or_default();
    let content_html = db
        .try_get::<daynews_db::ArticleBody>(id)?
        .and_then(|b| b.with_value_untracked(|b| b.map(|b| b.content_html.clone())));
    Ok(Some(StoredArticle {
        id,
        feed_id,
        feed_title,
        title: a.title,
        url: a.url,
        author: a.author,
        published_at: a.published_at,
        summary: a.summary,
        content_html,
        is_read: a.is_read,
        is_starred: a.is_starred,
    }))
}

pub fn select_scope(scope: Scope) {
    let sc = scene();
    batch(|| {
        sc.scope.set(scope);
        sc.selected.set(None);
        sc.reader_open.set(false);
        sc.sticky_read.set(vec![]);
    });
}
pub fn set_search(text: &str) {
    scene().search.set(text.to_owned());
}
pub fn open_article(id: u64) {
    let sc = scene();
    batch(|| {
        set_read(id, true);
        sc.selected.set(Some(id));
        sc.reader_open.set(true);
    });
}
pub fn open_next_unread() -> bool {
    let sc = scene();
    let rows = sc.articles.get_untracked();
    let current = sc.selected.get_untracked();
    let start = current
        .and_then(|id| rows.iter().position(|r| r.id == id))
        .map(|i| i + 1)
        .unwrap_or(0);
    let next = rows[start.min(rows.len())..]
        .iter()
        .chain(rows[..start.min(rows.len())].iter())
        .find(|r| !r.is_read && Some(r.id) != current)
        .map(|r| r.id);
    if let Some(id) = next {
        open_article(id);
        return true;
    }
    let Some(worker) = worker() else {
        return false;
    };
    if state().total_unread.get_untracked() == 0 {
        return false;
    }
    batch(|| {
        sc.scope.set(Scope::Unread);
        sc.search.set(String::new());
        sc.sticky_read.set(vec![]);
        sc.selected.set(None);
        sc.reader_open.set(false);
    });
    let request = worker.read(move |db| {
        let q = db
            .query::<Article>()
            .filter(
                daynews_db::scope_pred(Scope::Unread) & !Pred::IdIn(current.into_iter().collect()),
            )
            .sort(Article::published_at().desc())
            .limit(1)
            .live();
        Ok(q.try_collect()?.first().map(|a| a.id))
    });
    // Capture the originating scene. An asynchronous completion must never target whatever
    // other window happens to become focused while the query runs.
    day_core::task(async move {
        match request.await {
            Ok(Some(id))
                if sc.selected.try_get() == Some(None)
                    && sc.scope.try_get() == Some(Scope::Unread)
                    && sc.search.try_get().is_some_and(|s| s.is_empty()) =>
            {
                batch(|| {
                    sc.scope.set(Scope::Unread);
                    sc.search.set(String::new());
                    sc.sticky_read.set(vec![id]);
                    sc.selected.set(Some(id));
                    sc.reader_open.set(true);
                });
                edit(move |db| {
                    db.set_read(id, true);
                    Ok(())
                });
            }
            Ok(_) => (),
            Err(e) => report(e),
        }
    });
    true
}
pub fn set_read(id: u64, read: bool) {
    let sc = scene();
    if read && sc.scope.get_untracked() == Scope::Unread {
        sc.sticky_read.update(|ids| {
            if !ids.contains(&id) {
                ids.push(id);
            }
        });
    }
    edit(move |db| {
        db.set_read(id, read);
        Ok(())
    });
}
pub fn toggle_read(id: u64) {
    let sc = scene();
    if sc.scope.get_untracked() == Scope::Unread {
        sc.sticky_read.update(|ids| {
            if !ids.contains(&id) {
                ids.push(id);
            }
        });
    }
    // Read-modify-write on the queue, never against a potentially stale UI snapshot.
    edit(move |db| {
        if let Some(a) = db.container.try_get::<Article>(id)? {
            a.is_read().write(!a.is_read().peek());
        }
        Ok(())
    });
}
pub fn set_starred(id: u64, starred: bool) {
    edit(move |db| {
        db.set_starred(id, starred);
        Ok(())
    });
}
pub fn toggle_tag(article: u64, name: &str) {
    let name = name.trim().to_owned();
    if name.is_empty() {
        return;
    }
    edit(move |db| {
        let tag = db.add_tag(&name);
        let tagged = db
            .container
            .try_get::<Article>(article)?
            .map(|a| a.tags().contains(tag))
            .unwrap_or(false);
        db.set_tagged(article, tag, !tagged);
        Ok(())
    });
}
pub fn mark_scope_read(read: bool) {
    let sc = scene();
    let scope = sc.scope.get_untracked();
    edit(move |db| {
        db.set_read_all(scope, read);
        Ok(())
    });
    sc.sticky_read.set(vec![]);
}
pub fn mark_feed_read(feed: u64, read: bool) {
    edit(move |db| {
        db.set_read_all(Scope::Feed(feed), read);
        Ok(())
    });
}
pub async fn prune(days: u32) -> usize {
    if days == 0 {
        return 0;
    }
    let worker = match ready_worker().await {
        Ok(w) => w,
        Err(e) => {
            report(e);
            return 0;
        }
    };
    match worker
        .write(move |db| {
            day_model::with_author("retention", || {
                Ok(Db {
                    container: db.clone(),
                }
                .prune_older_than(days))
            })
        })
        .await
    {
        Ok(n) => n,
        Err(e) => {
            report(e);
            0
        }
    }
}
pub fn subscribe(url: &str) {
    let url = normalize_feed_url(url);
    if url.is_empty() {
        return;
    }
    day_core::task(async move {
        let worker = match ready_worker().await {
            Ok(w) => w,
            Err(e) => {
                report(e);
                return;
            }
        };
        let copy = url.clone();
        match worker
            .write(move |db| {
                Ok(Db {
                    container: db.clone(),
                }
                .add_feed(&copy, &fallback_title(&copy), None))
            })
            .await
        {
            Ok(id) => {
                update_undo(&worker).await;
                refresh_one(id, url).await;
            }
            Err(e) => report(e),
        }
    });
}
pub fn unsubscribe(feed: u64) {
    edit(move |db| {
        db.container.delete::<daynews_db::Feed>(feed)?;
        Ok(())
    });
    let sc = scene();
    if sc.scope.get_untracked() == Scope::Feed(feed) {
        select_scope(Scope::Unread);
    }
}
pub fn create_folder(name: &str) -> Option<u64> {
    let name = name.to_owned();
    let id = folder_id(&name);
    edit(move |db| {
        db.add_folder(&name);
        Ok(())
    });
    Some(id)
}
pub fn rename_feed(feed: u64, title: &str) {
    let title = title.to_owned();
    edit(move |db| {
        db.rename_feed(feed, &title);
        Ok(())
    });
}
/// Refresh one subscription: a feed row's Refresh and Feed ▸ Refresh Feed. A full refresh
/// already under way fetches this feed too, so asking again while it runs does nothing.
pub fn refresh_feed(feed: u64) {
    let st = state();
    if st.refresh_progress.get_untracked().is_some() {
        return;
    }
    let Some((title, url)) = st
        .feeds
        .get_untracked()
        .iter()
        .find(|f| f.id == feed)
        .map(|f| (f.title.clone(), f.feed_url.clone()))
    else {
        return;
    };
    st.refresh_progress.set(Some((0, 1)));
    day_core::task(async move {
        let ok = refresh_one(feed, url).await;
        let st = state();
        st.refresh_progress.set(None);
        st.status
            .set(status(StatusMessage::RefreshedFeed(title, ok)));
    });
}

// ---- refreshing -----------------------------------------------------------------------------

/// Refresh up to four subscriptions concurrently. A slow server does not hold up every
/// other feed; the bounded stream keeps resource use predictable. Futures still poll on
/// the UI executor, where the store and reactive state belong.
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
        let mut pending = stream::iter(feeds)
            .map(|(id, url)| refresh_one(id, url))
            .buffer_unordered(4);
        let mut done = 0;
        while let Some(ok) = pending.next().await {
            failed += usize::from(!ok);
            done += 1;
            state().refresh_progress.set(Some((done, total)));
        }
        let st = state();
        st.refresh_progress.set(None);
        st.status
            .set(status(StatusMessage::RefreshedFeeds(total, failed)));
    });
}

/// Fetch and store one feed. Returns whether it succeeded.
/// A feed's bytes, parsed: over HTTP normally, or from the app bundle for `asset:` URLs, the
/// deterministic, network-free source the walkthrough seeds from on every platform
/// (dayscript/seed-demo.yaml subscribes to the demo feeds bundled under
/// `resource/assets/demo/`). A missing asset reports as a 404 rather than a new error
/// arm; the subscription then shows the same failed-refresh state a dead feed does.
async fn fetch_feed(url: &str) -> Result<daynews_feed::ParsedFeed, daynews_feed::FeedError> {
    if let Some(name) = url.strip_prefix("asset:") {
        let locale = day_l10n::locale().get_untracked();
        for candidate in asset_candidates(name, &locale) {
            // wasm has no filesystem for the resource opener to read; the web dist serves the
            // same bundle over HTTP instead (`resource/assets/` staged under `assets/data/`,
            // day-cli web.rs), so the asset rides the ordinary fetch path as a same-origin URL.
            // Fetch by the relative dist URL, parse against the absolute `asset:` base, because
            // the parser's URL resolution rejects a relative base outright.
            #[cfg(target_arch = "wasm32")]
            match daynews_feed::fetch_with_base(&format!("assets/data/{candidate}"), url).await {
                Err(daynews_feed::FeedError::Status(404)) => continue,
                other => return other,
            }
            #[cfg(not(target_arch = "wasm32"))]
            if let Some(res) = day_core::resource(day_core::AssetName::dynamic(candidate)) {
                return daynews_feed::parse(res.as_slice(), url);
            }
        }
        return Err(daynews_feed::FeedError::Status(404));
    }
    daynews_feed::fetch(url).await
}

/// The bundle paths an `asset:` name may live at, most specific first.
///
/// The demo feeds are written once per language (resource/assets/demo/README.md) and subscribed
/// to without one, as `demo/night-sky.xml`, so a subscription reads the set for the language the
/// app is running in: the exact locale, then its language alone, then English. Any other name is
/// one file, taken as written.
fn asset_candidates(name: &str, locale: &str) -> Vec<String> {
    let Some(file) = name.strip_prefix("demo/") else {
        return vec![name.to_string()];
    };
    let tag = locale.split("-u-").next().unwrap_or(locale);
    let lang = tag.split('-').next().unwrap_or(tag);
    let mut paths: Vec<String> = Vec::new();
    for dir in [tag, lang, "en"] {
        let path = format!("demo/{dir}/{file}");
        if !dir.is_empty() && !paths.contains(&path) {
            paths.push(path);
        }
    }
    paths
}

struct UpdatingFeed {
    store: std::rc::Rc<Store>,
    id: u64,
    active: Signal<HashMap<u64, Option<f64>>>,
}
impl Drop for UpdatingFeed {
    fn drop(&mut self) {
        self.store.scheduled_feeds.borrow_mut().remove(&self.id);
        self.active.update(|ids| {
            ids.remove(&self.id);
        });
    }
}

async fn refresh_one(id: u64, url: String) -> bool {
    let store = store();
    if !store.scheduled_feeds.borrow_mut().insert(id) {
        return true;
    }
    let active = state().updating_feeds;
    let _updating = UpdatingFeed {
        store: store.clone(),
        id,
        active,
    };
    // Subscription fetches and full/manual refreshes share the same four native exchanges.
    let (_slot, _, waiting) = futures_util::future::select_all(
        store.refresh_slots.iter().map(|slot| Box::pin(slot.lock())),
    )
    .await;
    drop(waiting);
    let worker = match ready_worker().await {
        Ok(w) => w,
        Err(e) => {
            report(e);
            return false;
        }
    };
    let validators = worker
        .read(move |db| {
            Ok(Db {
                container: db.clone(),
            }
            .feed_validators(id))
        })
        .await;
    let (etag, last_modified) = match validators {
        Ok(Some(v)) => v,
        Ok(None) => return true,
        Err(e) => {
            report(e);
            return false;
        }
    };
    active.update(|ids| {
        ids.insert(id, None);
    });
    let validators = daynews_feed::CacheValidators {
        etag,
        last_modified,
    };
    let result = if url.starts_with("asset:") {
        fetch_feed(&url)
            .await
            .map(|feed| daynews_feed::FeedUpdate::Modified(feed, Default::default()))
    } else {
        let mut last = None;
        let mut last_update = 0;
        daynews_feed::fetch_conditional_with_progress(&url, &validators, |received, total| {
            let fraction = total
                .filter(|n| *n > 0)
                .map(|n| (received as f64 / n as f64).clamp(0.0, 1.0));
            let now = daynews_time::now_epoch_ms();
            if fraction != last
                && (fraction.is_some() != last.is_some()
                    || fraction == Some(1.0)
                    || now.saturating_sub(last_update) >= 100)
            {
                active.update(|values| {
                    values.insert(id, fraction);
                });
                last = fraction;
                last_update = now;
            }
        })
        .await
    };
    let applied = worker
        .write(move |container| {
            day_model::with_author("feed-refresh", || {
                let d = Db {
                    container: container.clone(),
                };
                Ok(match result {
                    Ok(daynews_feed::FeedUpdate::NotModified(validators)) => {
                        d.feed_checked(id, validators.etag, validators.last_modified);
                        true
                    }
                    Ok(daynews_feed::FeedUpdate::Modified(parsed, validators)) => {
                        let items: Vec<IncomingArticle> = parsed
                            .items
                            .into_iter()
                            .map(|i| {
                                // Move bodies into the store rather than cloning every downloaded article
                                // on the UI thread. Title-less items still get their readable display title.
                                let title = Some(i.display_title());
                                IncomingArticle {
                                    guid: i.guid,
                                    title,
                                    url: i.url,
                                    author: i.author,
                                    published: i.published,
                                    summary: i.summary,
                                    content_html: i.content_html,
                                }
                            })
                            .collect();
                        {
                            d.update_feed_metadata(
                                id,
                                parsed.title.as_deref(),
                                parsed.site_url.as_deref(),
                                parsed.description.as_deref(),
                                parsed.icon_url.as_deref(),
                            );
                            // Unsubscribing while the request was in flight must not insert orphan articles.
                            if d.feed_validators(id).is_some() {
                                d.try_upsert_articles(id, &url, &items)?;
                                d.feed_checked(id, validators.etag, validators.last_modified);
                            }
                        }
                        true
                    }
                    Err(e) => {
                        d.set_feed_error(id, &e.to_string());
                        false
                    }
                })
            })
        })
        .await;
    match applied {
        Ok(ok) => ok,
        Err(e) => {
            report(e);
            false
        }
    }
}

/// Import subscriptions, creating folders as needed. Returns (added, already present).
pub async fn import_opml(text: &str) -> std::result::Result<(usize, usize), String> {
    let doc = daynews_opml::parse(text).map_err(|e| e.to_string())?;
    let entries: Vec<_> = doc
        .feeds()
        .into_iter()
        .map(|(path, feed)| (path, feed.clone()))
        .collect();
    let worker = ready_worker().await.map_err(|e| e.to_string())?;
    let (added, existing) = worker
        .write(move |container| {
            let d = Db {
                container: container.clone(),
            };
            let (mut added, mut existing) = (0usize, 0usize);
            for (path, feed) in &entries {
                // Only the innermost folder becomes a folder; deeper nesting is rare and flattening
                // it matches what the sidebar can represent.
                let folder = path.last().map(|name| d.add_folder(name));
                let url = feed.xml_url.clone();
                if d.container.get::<daynews_db::Feed>(feed_id(&url)).is_some() {
                    existing += 1;
                    continue;
                }
                d.add_feed(&url, &feed.display_title(), folder);
                added += 1;
            }
            Ok((added, existing))
        })
        .await
        .map_err(|e| e.to_string())?;
    update_undo(&worker).await;
    state()
        .status
        .set(status(StatusMessage::Imported(added, existing)));
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
        title: Some(status(StatusMessage::ExportTitle)),
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

/// A provisional name for a brand-new subscription, replaced by the feed's title on first
/// refresh, the same placeholder NetNewsWire shows.
fn fallback_title(url: &str) -> String {
    // A bundled feed has no host: name it after its file, not the `asset:demo` prefix.
    if let Some(name) = url.strip_prefix("asset:") {
        let file = name.rsplit('/').next().unwrap_or(name);
        return file.split('.').next().unwrap_or(file).to_string();
    }
    let rest = url.split_once("://").map(|(_, r)| r).unwrap_or(url);
    let host = rest.split(['/', '?', '#']).next().unwrap_or(rest);
    host.strip_prefix("www.").unwrap_or(host).to_string()
}

/// Where the SQLite file lives, per platform.
pub fn store_dir() -> PathBuf {
    // UI validation can use a fresh store without modifying a reader's subscriptions.
    std::env::var_os("DAY_NEWS_DATA_DIR")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .unwrap_or_else(base_dir)
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
    // Web: the "path" names an OPFS file, not a filesystem location; there is no $HOME and
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        future::Future,
        sync::Arc,
        task::{Context, Poll, Wake, Waker},
        time::{Duration, Instant},
    };
    fn wait<F: Future>(future: F) -> F::Output {
        struct Unpark(std::thread::Thread);
        impl Wake for Unpark {
            fn wake(self: Arc<Self>) {
                self.0.unpark();
            }
        }
        let waker = Waker::from(Arc::new(Unpark(std::thread::current())));
        let mut cx = Context::from_waker(&waker);
        let mut future = std::pin::pin!(future);
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            if let Poll::Ready(value) = future.as_mut().poll(&mut cx) {
                return value;
            }
            assert!(Instant::now() < deadline, "article worker timed out");
            std::thread::park_timeout(Duration::from_millis(10));
        }
    }
    const URL: &str = "https://synthetic.example/feed";
    fn item(id: &str, title: &str, body: &str, published: i64) -> IncomingArticle {
        IncomingArticle {
            guid: id.into(),
            title: Some(title.into()),
            url: Some(format!("https://synthetic.example/{id}")),
            author: None,
            published: Some(published),
            summary: None,
            content_html: Some(format!("<p>{body}</p>")),
        }
    }
    fn seeded() -> DatabaseWorker {
        let worker = wait(DatabaseWorker::open(|| {
            Db::open_in_memory().map(|db| db.container)
        }))
        .unwrap();
        wait(worker.write(|container| {
            let db = Db {
                container: container.clone(),
            };
            let feed = db.add_feed(URL, "Synthetic publication", None);
            db.upsert_articles(
                feed,
                URL,
                &[
                    item("1", "Moon landing", "celestial body", 100),
                    item("2", "Other article", "uniqueparallax", 200),
                ],
            );
            Ok(())
        }))
        .unwrap();
        worker
    }
    #[test]
    fn worker_search_reads_title_and_body_indexes_and_handles_user_syntax() {
        let worker = seeded();
        for (term, expected) in [
            ("moon", vec![article_id(URL, "1")]),
            ("uniqueparallax", vec![article_id(URL, "2")]),
            ("doesnotexist", vec![]),
            ("\" AND (", vec![]),
        ] {
            let result =
                wait(worker.read(move |db| timeline_snapshot(db, Scope::All, term, &[]))).unwrap();
            assert_eq!(result.0.iter().map(|a| a.id).collect::<Vec<_>>(), expected);
        }
        worker.close_blocking().unwrap();
    }
    #[test]
    fn independent_search_subscriptions_keep_their_own_scopes() {
        let worker = seeded();
        let mut moon =
            wait(worker.observe(|db| timeline_snapshot(db, Scope::All, "moon", &[]))).unwrap();
        let mut body =
            wait(worker.observe(|db| timeline_snapshot(db, Scope::All, "uniqueparallax", &[])))
                .unwrap();
        assert_eq!(
            wait(moon.next()).unwrap().unwrap().value.0[0].id,
            article_id(URL, "1")
        );
        assert_eq!(
            wait(body.next()).unwrap().unwrap().value.0[0].id,
            article_id(URL, "2")
        );
        wait(worker.write(|db| {
            db.try_get::<Article>(article_id(URL, "1"))?
                .unwrap()
                .is_starred()
                .write(true);
            Ok(())
        }))
        .unwrap();
        assert!(wait(moon.next()).unwrap().unwrap().value.0[0].is_starred);
        worker.close_blocking().unwrap();
    }
    #[test]
    fn sticky_read_rows_and_sidebar_counts_reflect_committed_edits() {
        let worker = seeded();
        wait(worker.write(|db| {
            db.try_get::<Article>(article_id(URL, "1"))?
                .unwrap()
                .is_read()
                .write(true);
            Ok(())
        }))
        .unwrap();
        let (rows, count) = wait(
            worker.read(|db| timeline_snapshot(db, Scope::Unread, "", &[article_id(URL, "1")])),
        )
        .unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(count, 1);
        let sidebar = wait(worker.read(sidebar_snapshot)).unwrap();
        assert_eq!(sidebar.0[0].unread, 1);
        assert_eq!(sidebar.3, 1);
        let filtered = wait(
            worker.read(|db| timeline_snapshot(db, Scope::Unread, "moon", &[article_id(URL, "1")])),
        )
        .unwrap();
        assert_eq!(filtered.0.len(), 1);
        assert!(filtered.0[0].is_read);
        worker.close_blocking().unwrap();
    }
    #[test]
    fn concurrent_refresh_and_read_edits_preserve_read_state() {
        let worker = seeded();
        let importer = worker.clone();
        let refresh = std::thread::spawn(move || {
            for _ in 0..40 {
                wait(importer.write(|container| {
                    day_model::with_author("feed-refresh", || {
                        let db = Db {
                            container: container.clone(),
                        };
                        db.upsert_articles(
                            feed_id(URL),
                            URL,
                            &[item("1", "Moon landing", "updated body", 100)],
                        );
                        Ok(())
                    })
                }))
                .unwrap();
            }
        });
        wait(worker.write(|db| {
            db.try_get::<Article>(article_id(URL, "1"))?
                .unwrap()
                .is_read()
                .write(true);
            Ok(())
        }))
        .unwrap();
        refresh.join().unwrap();
        let article = wait(worker.read(|db| reader_snapshot(db, article_id(URL, "1"))))
            .unwrap()
            .unwrap();
        assert!(article.is_read);
        assert_eq!(
            article.content_html.as_deref(),
            Some("<p>celestial body</p>")
        );
        worker.close_blocking().unwrap();
    }
    #[test]
    fn deleted_article_clears_reader_projection_after_commit() {
        let worker = seeded();
        let mut reader =
            wait(worker.observe(|db| reader_snapshot(db, article_id(URL, "1")))).unwrap();
        assert!(wait(reader.next()).unwrap().unwrap().value.is_some());
        wait(worker.write(|db| {
            db.delete::<Article>(article_id(URL, "1"));
            Ok(())
        }))
        .unwrap();
        assert!(wait(reader.next()).unwrap().unwrap().value.is_none());
        worker.close_blocking().unwrap();
    }
    #[test]
    fn rapid_search_cancellation_keeps_latest_subscription() {
        let worker = seeded();
        for i in 0..100 {
            let term = format!("obsolete{i}");
            drop(
                wait(worker.observe(move |db| timeline_snapshot(db, Scope::All, &term, &[])))
                    .unwrap(),
            );
        }
        let mut current =
            wait(worker.observe(|db| timeline_snapshot(db, Scope::All, "uniqueparallax", &[])))
                .unwrap();
        let result = wait(current.next()).unwrap().unwrap().value;
        assert_eq!(result.0.len(), 1);
        assert_eq!(result.0[0].id, article_id(URL, "2"));
        worker.close_blocking().unwrap();
    }
}
