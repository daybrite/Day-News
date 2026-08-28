//! The local article store, on day-persistence's lazy engine.
//!
//! The schema is relational and declared: folders own feeds (ordered, cascade), feeds own
//! articles (cascade), an article's BODY is its own row (so the timeline's window-faulting
//! never loads bodies), tags cross articles through a join table, and search runs through
//! generated FTS5 shadows over titles/authors/summaries and over bodies — one fetch reads
//! both via a relation-crossing match. Nothing loads at open; the UI binds live queries and
//! the engine answers them.
//!
//! Identity is DETERMINISTIC: a feed's id is a hash of its URL, an article's a hash of
//! (feed URL, guid), a folder's/tag's a hash of its name. Subscribing twice, re-importing an
//! OPML, or refetching a feed therefore cannot create duplicates — the id already exists —
//! and read state survives every refresh because an existing article is never touched.

use day_macros::Model;
use day_model::{ModelId, Op};
use day_persistence::{
    CountQuery, DbError, Fetch, Many, ModelContainer, One, Pred, Query, Sqlite, schema,
};
use day_reactive::Binding;

pub use day_persistence::Value;

// Every fts(…) below declares `tokenize = "unicode61 remove_diacritics 2"`: diacritics-
// insensitive search, so `ecole` finds `École` — what a reader's search field means. (An
// attribute takes only literals, so the string repeats rather than naming a const.)

// ---------------------------------------------------------------------------
// Models
// ---------------------------------------------------------------------------

#[derive(Model, Clone, Default, PartialEq, Debug)]
#[model(table = "folders")]
pub struct Folder {
    #[model(id)]
    pub id: u64,
    #[model(unique)]
    pub name: String,
    /// Sidebar order among folders (fractional keying, like every ordered surface).
    pub position: f64,
    /// Deleting a folder unsubscribes its feeds — and their articles, bodies and tag
    /// memberships go with them: one delete, the whole subtree, one undo unit.
    #[model(relation(target = Feed, inverse = "folder", delete = "cascade", ordered = "position"))]
    pub feeds: Many<Feed>,
}

#[derive(Model, Clone, Default, PartialEq, Debug)]
#[model(table = "feeds")]
pub struct Feed {
    #[model(id)]
    pub id: u64,
    #[model(unique)]
    pub feed_url: String,
    #[model(index)]
    pub title: String,
    /// `None` is a top-level subscription (no folder).
    pub folder: Option<One<Folder>>,
    pub site_url: Option<String>,
    pub description: Option<String>,
    pub icon_url: Option<String>,
    pub last_fetched_at: Option<i64>,
    pub last_error: Option<String>,
    /// Order within the folder (or among top-level feeds).
    pub position: f64,
    #[model(relation(target = Article, inverse = "feed", delete = "cascade"))]
    pub articles: Many<Article>,
}

#[derive(Model, Clone, Default, PartialEq, Debug)]
#[model(
    table = "articles",
    index("feed", "published_at"),
    index("is_read", "published_at"),
    fts(
        "title",
        "author",
        "summary",
        tokenize = "unicode61 remove_diacritics 2"
    )
)]
pub struct Article {
    #[model(id)]
    pub id: u64,
    pub guid: String,
    pub feed: One<Feed>,
    pub title: Option<String>,
    pub url: Option<String>,
    pub author: Option<String>,
    #[model(index)]
    pub published_at: i64,
    pub summary: Option<String>,
    #[model(index)]
    pub is_read: bool,
    #[model(index)]
    pub is_starred: bool,
    pub first_seen_at: i64,
    /// The body rides in its own row so a timeline window faults metadata only.
    #[model(relation(target = ArticleBody, inverse = "article", delete = "cascade"))]
    pub body: Many<ArticleBody>,
    /// User labels — a many-to-many; membership rows cascade with either side.
    #[model(relation(target = Tag, join = "article_tags"))]
    pub tags: Many<Tag>,
}

/// An article's HTML body — one row per article, keyed by the SAME id, faulted only when the
/// reader opens it, and searched through its own FTS shadow.
#[derive(Model, Clone, Default, PartialEq, Debug)]
#[model(
    table = "article_bodies",
    fts("content_html", tokenize = "unicode61 remove_diacritics 2")
)]
pub struct ArticleBody {
    #[model(id)]
    pub id: u64,
    pub article: One<Article>,
    pub content_html: String,
}

#[derive(Model, Clone, Default, PartialEq, Debug)]
#[model(table = "tags")]
pub struct Tag {
    #[model(id)]
    pub id: u64,
    #[model(unique)]
    pub name: String,
    #[model(relation(target = Article, join = "article_tags"))]
    pub articles: Many<Article>,
}

// ---------------------------------------------------------------------------
// Identity
// ---------------------------------------------------------------------------

/// FNV-1a over the parts, masked into the integer-key space (the top bit is day-model's
/// interned-handle floor) and steered off 0. Deterministic identity is the dedup story:
/// the same URL or (feed, guid) always names the same row.
fn ident(parts: &[&str]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for p in parts {
        for b in p.as_bytes() {
            h ^= *b as u64;
            h = h.wrapping_mul(0x0100_0000_01b3);
        }
        h ^= 0x1f; // separator, so ("ab","c") and ("a","bc") differ
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    let h = h & (u64::MAX >> 1);
    if h == 0 { 1 } else { h }
}

pub fn feed_id(feed_url: &str) -> u64 {
    ident(&["feed", feed_url])
}

pub fn article_id(feed_url: &str, guid: &str) -> u64 {
    ident(&["article", feed_url, guid])
}

pub fn folder_id(name: &str) -> u64 {
    ident(&["folder", name])
}

pub fn tag_id(name: &str) -> u64 {
    ident(&["tag", name])
}

// ---------------------------------------------------------------------------
// Scopes and fetches
// ---------------------------------------------------------------------------

/// What the timeline is showing — the sidebar selection, in NetNewsWire's terms.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// Every article from every feed.
    All,
    /// Unread only, across all feeds — NetNewsWire's "All Unread".
    Unread,
    /// Published since local midnight, read or not — NetNewsWire's "Today".
    Today,
    Starred,
    Feed(u64),
    Folder(u64),
    Tag(u64),
}

/// The instant local midnight happened, as unix seconds. Local rather than UTC because "Today"
/// is a claim about the reader's calendar, not the server's. day-part-timezone answers on
/// every target — bundled tzdb offsets, the browser's zone on web — where chrono's `Local`
/// aborts on wasm.
pub fn start_of_today() -> i64 {
    let now_s = now_unix();
    let off =
        i64::from(day_part_timezone::local_offset_seconds(day_part_timezone::now()).unwrap_or(0));
    let midnight = (now_s + off).div_euclid(86_400) * 86_400 - off;
    // A DST change between local midnight and now shifts the boundary by the offsets'
    // difference; re-derive once with the offset that was in force AT that instant.
    if midnight >= 0 {
        let at = std::time::UNIX_EPOCH + std::time::Duration::from_secs(midnight as u64);
        let off2 = day_part_timezone::local_offset_seconds(at)
            .map(i64::from)
            .unwrap_or(off);
        if off2 != off {
            return (now_s + off2).div_euclid(86_400) * 86_400 - off2;
        }
    }
    midnight
}

/// The scope's predicate alone (no search, no sort) — what count badges share with the
/// timeline.
pub fn scope_pred(scope: Scope) -> Pred {
    match scope {
        Scope::All => Pred::Always,
        Scope::Unread => Article::is_read().eq(false),
        Scope::Today => Article::published_at().ge(start_of_today()),
        Scope::Starred => Article::is_starred().eq(true),
        Scope::Feed(id) => Article::feed().is(id),
        Scope::Folder(id) => Article::feed().any(Feed::folder().is(id)),
        Scope::Tag(id) => Article::tags().any(Pred::IdIn(vec![id])),
    }
}

/// The timeline's fetch: the scope, the search text (through BOTH full-text shadows — titles
/// and bodies), newest first, windowed.
pub fn timeline_fetch(scope: Scope, search: &str, limit: usize) -> Fetch {
    let mut f = Fetch::new()
        .filter(scope_pred(scope))
        .sort(Article::published_at().desc())
        .limit(limit);
    let search = search.trim();
    if !search.is_empty() {
        let q = fts_query(search);
        f = f.filter(
            Article::fts().matches(q.clone()) | Article::body().any(ArticleBody::fts().matches(q)),
        );
    }
    f
}

/// Turn user text into an FTS5 query. Everything is quoted as a phrase and the last token gets
/// a prefix `*`, so search feels live as you type. Quoting also means a stray `"` or `-` is
/// searched for rather than being read as FTS syntax and erroring.
pub fn fts_query(input: &str) -> String {
    let tokens: Vec<String> = input
        .split_whitespace()
        .map(|t| t.replace('"', ""))
        .filter(|t| !t.is_empty())
        .collect();
    if tokens.is_empty() {
        return "\"\"".into();
    }
    let last = tokens.len() - 1;
    tokens
        .iter()
        .enumerate()
        .map(|(i, t)| {
            if i == last {
                format!("\"{t}\"*")
            } else {
                format!("\"{t}\"")
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

// ---------------------------------------------------------------------------
// The store
// ---------------------------------------------------------------------------

/// An article as it arrives from a parsed feed, ready to store.
#[derive(Debug, Clone)]
pub struct IncomingArticle {
    pub guid: String,
    pub title: Option<String>,
    pub url: Option<String>,
    pub author: Option<String>,
    pub published: Option<i64>,
    pub summary: Option<String>,
    pub content_html: Option<String>,
}

/// The open store: a [`ModelContainer`] plus the reader's domain operations. The UI reads
/// through live queries ([`Db::timeline`], [`Db::count`], the caches); everything here writes
/// through the front door, so autosave, undo and every query hear it.
pub struct Db {
    pub container: ModelContainer,
}

/// Bound the rows materialized at once by a bulk write, so marking thousands read cannot
/// balloon the cache: each chunk faults, writes, and flushes before the next.
const BULK_CHUNK: usize = 2_000;

/// Statement logging in debug builds: every SQL the engine executes for this store —
/// migrations, autosave flushes, cascades, live queries — through the engine's own trace
/// (docs/persistence.md), at `trace!` because it is a per-statement firehose (docs/logging.md).
/// `DAY_LOG=trace` shows it; anything less hides it, which is the point of a level. The
/// `cfg!(debug_assertions)` guard stays: a release build should not pay to format SQL it will
/// then discard.
fn traced(driver: Sqlite) -> Sqlite {
    if cfg!(debug_assertions) {
        driver.trace_sql(|sql| log::trace!("sql: {sql}"))
    } else {
        driver
    }
}

impl Db {
    /// Open (creating if needed) the store at `path`.
    pub fn open(path: &std::path::Path) -> Result<Db, DbError> {
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        Self::from_driver(Sqlite::at(path))
    }

    /// An in-memory store, for tests.
    pub fn open_in_memory() -> Result<Db, DbError> {
        Self::from_driver(Sqlite::memory())
    }

    fn from_driver(driver: Sqlite) -> Result<Db, DbError> {
        let container = ModelContainer::open(
            traced(driver),
            schema![Folder, Feed, Article, ArticleBody, Tag],
        )?;
        Ok(Db { container })
    }

    // ---- feeds & folders ------------------------------------------------------------------

    /// Subscribe, or answer the existing subscription — the id is the URL's hash, so
    /// subscribing twice cannot duplicate (and OPML re-imports are naturally idempotent).
    pub fn add_feed(&self, feed_url: &str, title: &str, folder: Option<u64>) -> u64 {
        let id = feed_id(feed_url);
        if self.container.get::<Feed>(id).is_some() {
            return id;
        }
        let position = self.next_position::<Feed>(Feed::position());
        self.container.insert(Feed {
            id,
            feed_url: feed_url.to_string(),
            title: title.to_string(),
            folder: folder.map(One::to),
            position,
            ..Default::default()
        });
        id
    }

    /// Create a folder (idempotent by name — the id IS the name's hash).
    pub fn add_folder(&self, name: &str) -> u64 {
        let id = folder_id(name);
        if self.container.get::<Folder>(id).is_none() {
            let position = self.next_position::<Folder>(Folder::position());
            self.container.insert(Folder {
                id,
                name: name.to_string(),
                position,
                ..Default::default()
            });
        }
        id
    }

    /// Create a tag (idempotent by name).
    pub fn add_tag(&self, name: &str) -> u64 {
        let id = tag_id(name);
        if self.container.get::<Tag>(id).is_none() {
            self.container.insert(Tag {
                id,
                name: name.to_string(),
                ..Default::default()
            });
        }
        id
    }

    /// One past the current largest `position` — new rows land last.
    fn next_position<M: day_persistence::Model>(&self, col: day_persistence::Col<f64>) -> f64 {
        let last = self
            .container
            .query::<M>()
            .sort(col.desc())
            .limit(1)
            .live()
            .first();
        let Some(id) = last else { return 1.0 };
        if self.container.get::<M>(id).is_none() {
            return 1.0;
        }
        // Read the position through the cache row the fault just landed.
        let mut v = 0.0;
        self.container.cache::<M>().with_untracked(|k| {
            if let Some(m) = k.get(id.handle())
                && let Some(p) = day_model::ApplyField::read_field(m, col.field)
                && let Some(p) = p.downcast_ref::<f64>()
            {
                v = *p;
            }
        });
        v + 1.0
    }

    /// Record what a refresh learned about the channel itself.
    pub fn update_feed_metadata(
        &self,
        id: u64,
        title: Option<&str>,
        site_url: Option<&str>,
        description: Option<&str>,
        icon_url: Option<&str>,
    ) {
        let Some(feed) = self.container.get::<Feed>(id) else {
            return;
        };
        // Only overwrite the title when the feed actually supplied one, so a subscription
        // named by hand (or by its URL) is not blanked by a feed with an empty <title>.
        if let Some(t) = title.filter(|t| !t.trim().is_empty()) {
            feed.title().write(t.to_string());
        }
        feed.site_url().write(site_url.map(str::to_string));
        feed.description().write(description.map(str::to_string));
        feed.icon_url().write(icon_url.map(str::to_string));
        feed.last_fetched_at().write(Some(now()));
        feed.last_error().write(None);
    }

    /// Record a failed refresh so the sidebar can show the feed as broken.
    pub fn set_feed_error(&self, id: u64, message: &str) {
        if let Some(feed) = self.container.get::<Feed>(id) {
            feed.last_error().write(Some(message.to_string()));
            feed.last_fetched_at().write(Some(now()));
        }
    }

    pub fn rename_feed(&self, id: u64, title: &str) {
        if let Some(feed) = self.container.get::<Feed>(id) {
            feed.title().write(title.to_string());
        }
    }

    /// Unsubscribe. Articles, bodies and tag memberships go with the feed — the cascade —
    /// and with an undo stack installed the whole subtree comes back as one unit.
    pub fn delete_feed(&self, id: u64) {
        let _ = self.container.delete::<Feed>(id);
    }

    /// Delete a folder AND its feeds (their articles cascade too) — the deep cascade.
    pub fn delete_folder(&self, id: u64) {
        let _ = self.container.delete::<Folder>(id);
    }

    // ---- articles -------------------------------------------------------------------------

    /// Store a refresh's items for one feed. Returns how many were NEW — an item already
    /// present is left completely alone, which is what preserves read state (ids are the
    /// (feed, guid) hash, so existence is one id-set query, no faulting).
    pub fn upsert_articles(&self, feed: u64, feed_url: &str, items: &[IncomingArticle]) -> usize {
        let seen = now();
        let ids: Vec<u64> = items
            .iter()
            .map(|i| article_id(feed_url, &i.guid))
            .collect();
        let existing: std::collections::HashSet<u64> = self
            .container
            .query::<Article>()
            .filter(Pred::IdIn(ids.clone()))
            .live()
            .ids()
            .iter()
            .map(|i| i.handle())
            .collect();
        let mut added = 0usize;
        for (item, id) in items.iter().zip(&ids) {
            if existing.contains(id) {
                continue;
            }
            self.container.insert(Article {
                id: *id,
                guid: item.guid.clone(),
                feed: One::to(feed),
                title: item.title.clone(),
                url: item.url.clone(),
                author: item.author.clone(),
                // Undated items sort by when we first saw them, so they do not all pile up
                // at the epoch.
                published_at: item.published.unwrap_or(seen),
                summary: item.summary.clone(),
                first_seen_at: seen,
                ..Default::default()
            });
            if let Some(html) = item.content_html.clone().filter(|h| !h.is_empty()) {
                self.container.insert(ArticleBody {
                    id: *id,
                    article: One::to(*id),
                    content_html: html,
                });
            }
            added += 1;
        }
        added
    }

    /// One article's body, faulted on open — `None` when the item shipped none.
    pub fn body(&self, article: u64) -> Option<String> {
        self.container
            .get::<ArticleBody>(article)
            .map(|b| b.content_html().peek())
    }

    pub fn set_read(&self, article: u64, read: bool) {
        if let Some(a) = self.container.get::<Article>(article) {
            a.is_read().write(read);
        }
    }

    pub fn set_starred(&self, article: u64, starred: bool) {
        if let Some(a) = self.container.get::<Article>(article) {
            a.is_starred().write(starred);
        }
    }

    /// Tag or untag one article — a join-row link, one INSERT or DELETE.
    pub fn set_tagged(&self, article: u64, tag: u64, on: bool) {
        let Some(a) = self.container.get::<Article>(article) else {
            return;
        };
        if on {
            a.tags().add(tag);
        } else {
            a.tags().remove(tag);
        }
    }

    /// Mark everything in `scope` read (or unread) — "Mark All as Read". Chunked, so the
    /// working set stays bounded however many rows the scope holds.
    pub fn set_read_all(&self, scope: Scope, read: bool) -> usize {
        let want = !read;
        let fetch = Fetch::new()
            .filter(scope_pred(scope) & Article::is_read().eq(want))
            .sort(Article::published_at().asc());
        self.bulk_write(fetch, |a| a.is_read().write(read))
    }

    /// Delete read, unstarred, untagged articles older than `days` — the retention pass.
    /// Starred and tagged articles are the user's; they stay whatever their age.
    pub fn prune_older_than(&self, days: u32) -> usize {
        let cutoff = now() - i64::from(days) * 86_400;
        let doomed: Vec<ModelId<Article>> = self
            .container
            .query::<Article>()
            .filter(
                Article::is_read().eq(true)
                    & Article::is_starred().eq(false)
                    & Article::published_at().lt(cutoff)
                    & Article::first_seen_at().lt(cutoff)
                    & Article::tags().is_empty(),
            )
            .live()
            .ids();
        let n = doomed.len();
        let store = self.container.cache::<Article>();
        for chunk in doomed.chunks(BULK_CHUNK) {
            for id in chunk {
                let h = id.handle();
                store.restructure("prune", Op::Delete, h, |k| {
                    k.remove(h);
                });
            }
            let _ = self.container.save();
        }
        n
    }

    /// Fault, write, and flush a fetch's rows in bounded chunks.
    fn bulk_write(&self, fetch: Fetch, write: impl Fn(day_model::Elem<Article>)) -> usize {
        let ids = self
            .container
            .query::<Article>()
            .filter(fetch.pred.clone())
            .live()
            .ids();
        let store = self.container.cache::<Article>();
        for chunk in ids.chunks(BULK_CHUNK) {
            let keys: Vec<u64> = chunk.iter().map(|i| i.handle()).collect();
            let _ = self.container.ensure_resident::<Article>(&keys);
            for k in &keys {
                write(store.elem(*k));
            }
            let _ = self.container.save();
        }
        ids.len()
    }

    // ---- the live surface -----------------------------------------------------------------

    /// The timeline query for a fixed scope/search (the app's reactive form composes
    /// [`timeline_fetch`] with `query_fn` itself).
    pub fn timeline(&self, scope: Scope, search: &str, limit: usize) -> Query<Article> {
        self.container
            .query::<Article>()
            .filter(timeline_fetch(scope, search, limit).pred)
            .sort(Article::published_at().desc())
            .limit(limit)
            .live()
    }

    /// A live badge for a scope — `SELECT COUNT(*)`, no ids.
    pub fn count(&self, scope: Scope) -> CountQuery<Article> {
        self.container
            .query::<Article>()
            .filter(scope_pred(scope))
            .live_count()
    }

    /// A live UNREAD badge for a scope.
    pub fn unread_count(&self, scope: Scope) -> CountQuery<Article> {
        self.container
            .query::<Article>()
            .filter(scope_pred(scope) & Article::is_read().eq(false))
            .live_count()
    }
}

fn now() -> i64 {
    now_unix()
}

/// The wall clock as unix seconds, everywhere the app runs. day-part-timezone rather than
/// `SystemTime::now()`, which aborts on wasm32 — on web this is the page's `Date.now()`.
pub fn now_unix() -> i64 {
    (day_part_timezone::now_epoch_ms() / 1000) as i64
}
