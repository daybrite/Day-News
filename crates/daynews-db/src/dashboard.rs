//! Bounded dashboard projections: scan article metadata, never fault article bodies.
use crate::{Db, Feed, Scope, Value};
use day_persistence::DbError;
use std::collections::{BTreeMap, HashSet};

#[derive(Clone, Debug, PartialEq)]
pub struct PollingFeed {
    pub id: u64,
    pub title: String,
    pub interval: i64,
    pub next: Option<i64>,
    pub retry_after: Option<i64>,
    pub last_checked: Option<i64>,
    pub failed: bool,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Dashboard {
    pub feed_id: Option<u64>,
    pub habits: [[u64; 6]; 7],
    source_counts: BTreeMap<u64, u64>,
    pub title: String,
    pub description: Option<String>,
    pub site_url: Option<String>,
    pub feed_url: Option<String>,
    pub total: u64,
    pub unread: u64,
    pub starred: u64,
    pub words: u64,
    pub authors: usize,
    pub latest: Option<i64>,
    pub earliest: Option<i64>,
    pub lengths: [u64; 5],
    pub ages: [u64; 5],
    pub hourly: [u64; 24],
    /// At most 366 day bins, regardless of library size.
    pub days: Vec<(i64, u64)>,
    pub sources: Vec<(u64, String, u64)>,
    pub feeds: Vec<PollingFeed>,
}

/// Words estimated from visible stored text; inline markup does not split a word.
pub fn word_count(html: &str) -> u32 {
    let mut text = String::new();
    let mut tag = String::new();
    let mut in_tag = false;
    let mut skip = false;
    for c in html.chars() {
        match c {
            '<' => {
                in_tag = true;
                tag.clear();
            }
            '>' if in_tag => {
                in_tag = false;
                let name = tag
                    .split_whitespace()
                    .next()
                    .unwrap_or("")
                    .to_ascii_lowercase();
                match name.trim_end_matches('/') {
                    "script" | "style" => skip = true,
                    "/script" | "/style" => skip = false,
                    "p" | "/p" | "div" | "/div" | "br" | "li" | "/li" | "h1" | "/h1" | "h2"
                    | "/h2" => text.push(' '),
                    _ => {}
                }
            }
            _ if in_tag => tag.push(c),
            _ if !skip => text.push(c),
            _ => {}
        }
    }
    // Common non-breaking/spacing entities must be boundaries, not artificial words.
    let text = text
        .replace("&nbsp;", " ")
        .replace("&#160;", " ")
        .replace("&#xA0;", " ")
        .replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&mdash;", " ")
        .replace("&ndash;", " ");
    text.split_whitespace()
        .filter(|word| word.chars().any(char::is_alphanumeric))
        .count()
        .min(u32::MAX as usize) as u32
}

#[derive(Default)]
pub struct DashboardCache {
    key: Option<(u64, i64, Scope)>,
    articles: Dashboard,
}
impl Db {
    pub fn dashboard(&self, scope: Scope, now: i64) -> Result<Dashboard, DbError> {
        self.dashboard_cached(scope, now, &mut DashboardCache::default())
    }
    pub fn dashboard_cached(
        &self,
        scope: Scope,
        now: i64,
        cache: &mut DashboardCache,
    ) -> Result<Dashboard, DbError> {
        let (predicate, parameters) = match scope {
            Scope::All => ("1", vec![]),
            Scope::Unread => ("a.is_read = 0", vec![]),
            Scope::Starred => ("a.is_starred = 1", vec![]),
            Scope::Today => (
                "a.published_at >= ? AND a.published_at <= ?",
                vec![Value::Int(daynews_time::start_of_day(now)), Value::Int(now)],
            ),
            Scope::Feed(id) => ("a.feed = ?", vec![Value::Int(id as i64)]),
            Scope::Folder(id) => (
                "a.feed IN (SELECT id FROM feeds WHERE folder = ?)",
                vec![Value::Int(id as i64)],
            ),
            Scope::Tag(id) => (
                "a.id IN (SELECT article FROM article_tags WHERE tag = ?)",
                vec![Value::Int(id as i64)],
            ),
        };
        let key = (
            self.container.cache::<crate::Article>().version(),
            daynews_time::start_of_day(now),
            scope,
        );
        let mut result = if cache.key == Some(key) {
            cache.articles.clone()
        } else {
            let mut result = Dashboard::default();
            let mut days = BTreeMap::<i64, u64>::new();
            let mut sources = BTreeMap::<u64, u64>::new();
            let mut authors = HashSet::new();
            let today = daynews_time::start_of_day(now);
            self.container.try_with_connection(|conn| conn.query(
            &format!("SELECT a.feed, a.published_at, a.word_count, a.is_read, a.is_starred, a.author FROM articles a WHERE {predicate}"),
            &parameters, &mut |row| {
                let feed = row.get(0).as_int().unwrap_or(0) as u64;
                let date = row.get(1).as_int().unwrap_or(0);
                let words = row.get(2).as_int().unwrap_or(0).max(0) as u64;
                result.total += 1;
                result.unread += u64::from(row.get(3).as_int().unwrap_or(0) == 0);
                result.starred += u64::from(row.get(4).as_int().unwrap_or(0) != 0);
                result.words += words;
                if let Ok(author) = row.get(5).as_text() && !author.trim().is_empty() { authors.insert(author.to_owned()); }
                result.lengths[match words { 0..=99 => 0, 100..=299 => 1, 300..=699 => 2, 700..=1499 => 3, _ => 4 }] += 1;
                let age = now.saturating_sub(date).max(0);
                result.ages[match age / 86400 { 0 => 0, 1..=6 => 1, 7..=29 => 2, 30..=89 => 3, _ => 4 }] += 1;
                *sources.entry(feed).or_default() += 1;
                if date > 0 && date <= now {
                    result.latest = Some(result.latest.unwrap_or(date).max(date));
                    result.earliest = Some(result.earliest.unwrap_or(date).min(date));
                    let local = date + i64::from(daynews_time::local_offset_seconds(date).unwrap_or(0));
                    result.habits[(local.div_euclid(86400)+3).rem_euclid(7) as usize][local.rem_euclid(86400) as usize / 14400] += 1;
                    if date >= today { result.hourly[local.rem_euclid(86400) as usize / 3600] += 1; }
                    if date >= now.saturating_sub(365 * 86400) {
                        *days.entry(local.div_euclid(86400) * 86400).or_default() += 1;
                    }
                }
            }))?;
            result.authors = authors.len();
            result.days = days.into_iter().collect();
            result.source_counts = sources;
            cache.key = Some(key);
            cache.articles = result.clone();
            result
        };
        let feeds = self
            .container
            .query::<Feed>()
            .sort(Feed::position().asc())
            .sort(Feed::title().asc())
            .live()
            .try_collect()?;
        for feed in feeds {
            let relevant = match scope {
                Scope::Feed(id) => id == feed.id,
                Scope::All | Scope::Unread | Scope::Today | Scope::Starred => true,
                _ => result.source_counts.contains_key(&feed.id),
            };
            if let Scope::Feed(id) = scope
                && id == feed.id
            {
                result.feed_id = Some(feed.id);
                result.title = feed.title.clone();
                result.description = feed.description.clone();
                result.site_url = feed.site_url.clone();
                result.feed_url = Some(feed.feed_url.clone());
            }
            if let Some(count) = result.source_counts.get(&feed.id) {
                result.sources.push((feed.id, feed.title.clone(), *count));
            }
            if relevant {
                result.feeds.push(PollingFeed {
                    id: feed.id,
                    title: feed.title,
                    interval: crate::polling::interval(feed.publication_history.as_deref(), now),
                    next: feed.next_poll_at,
                    retry_after: feed.retry_after,
                    last_checked: feed.last_fetched_at,
                    failed: feed.last_error.is_some(),
                });
            }
        }
        result.sources.sort_by(|a, b| {
            b.2.cmp(&a.2)
                .then_with(|| a.1.cmp(&b.1))
                .then_with(|| a.0.cmp(&b.0))
        });
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn conditional_metadata_changes_do_not_rescan_dashboard_articles() {
        use std::sync::{Arc, Mutex};
        let statements = Arc::new(Mutex::new(Vec::<String>::new()));
        let recorder = statements.clone();
        let db = Db::from_driver(
            day_persistence::Sqlite::memory()
                .trace_sql(move |sql| recorder.lock().unwrap().push(sql.to_owned())),
        )
        .unwrap();
        let url = "https://fixture.example/dashboard-cache";
        let feed = db.add_feed(url, "Fixture", None);
        db.upsert_articles(
            feed,
            url,
            &[crate::IncomingArticle {
                guid: "fixture-one".into(),
                content_html: Some("<p>A synthetic fixture.</p>".into()),
                published: Some(100),
                title: None,
                url: None,
                author: None,
                summary: None,
            }],
        );
        let mut cache = DashboardCache::default();
        let first = db
            .dashboard_cached(Scope::Feed(feed), 200, &mut cache)
            .unwrap();
        assert_eq!(first.total, 1);
        db.feed_checked(feed, None, None);
        statements.lock().unwrap().clear();
        let checked = db
            .dashboard_cached(Scope::Feed(feed), 200, &mut cache)
            .unwrap();
        assert_eq!(checked.words, first.words);
        assert!(
            !statements
                .lock()
                .unwrap()
                .iter()
                .any(|sql| sql.contains("FROM articles a WHERE")),
            "a successful conditional check must reuse article aggregates"
        );
        db.set_read(crate::article_id(url, "fixture-one"), true);
        statements.lock().unwrap().clear();
        let changed = db
            .dashboard_cached(Scope::Feed(feed), 200, &mut cache)
            .unwrap();
        assert_eq!(changed.unread, 0);
        assert!(
            statements
                .lock()
                .unwrap()
                .iter()
                .any(|sql| sql.contains("FROM articles a WHERE"))
        );
    }
    #[test]
    fn publisher_identity_survives_duplicate_titles_and_cached_scope_changes() {
        let db = Db::from_driver(day_persistence::Sqlite::memory()).unwrap();
        let mut ids = Vec::new();
        for url in ["https://fixture.example/one", "https://fixture.example/two"] {
            let id = db.add_feed(url, "Same fixture title", None);
            db.upsert_articles(
                id,
                url,
                &[crate::IncomingArticle {
                    guid: "fixture".into(),
                    published: Some(100),
                    content_html: None,
                    title: None,
                    url: None,
                    author: None,
                    summary: None,
                }],
            );
            ids.push(id);
        }
        let mut cache = DashboardCache::default();
        let aggregate = db.dashboard_cached(Scope::All, 200, &mut cache).unwrap();
        assert_eq!(aggregate.feed_id, None);
        assert_eq!(aggregate.sources.len(), 2);
        assert_ne!(aggregate.sources[0].0, aggregate.sources[1].0);
        for id in ids {
            let feed = db
                .dashboard_cached(Scope::Feed(id), 200, &mut cache)
                .unwrap();
            assert_eq!(feed.feed_id, Some(id));
            assert_eq!(feed.sources, vec![(id, "Same fixture title".into(), 1)]);
        }
    }
    #[test]
    fn visible_text_word_counts() {
        assert_eq!(
            word_count(
                "<p>Hello <b>beautiful</b> world.</p><p>Second&nbsp;paragraph.</p><script>ignored text</script>"
            ),
            5
        );
        assert_eq!(word_count("hel<b>lo</b> &amp; goodbye"), 2);
        assert_eq!(word_count(""), 0);
    }
}
