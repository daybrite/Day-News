//! The store's contracts, headless: deterministic identity, read-state preservation across
//! refreshes, scoped timelines, two-shadow full-text search, deep cascades, tags, retention,
//! and live count badges.

use day_reactive::Binding;
use daynews_db::{Article, ArticleFields, Db, FeedFields, IncomingArticle, Scope, article_id};

fn item(guid: &str, title: &str, body: &str, published: i64) -> IncomingArticle {
    IncomingArticle {
        guid: guid.into(),
        title: Some(title.into()),
        url: Some(format!("https://e.example/{guid}")),
        author: Some("Ada".into()),
        published: Some(published),
        summary: Some(body.into()),
        content_html: Some(format!("<p>{body}</p>")),
    }
}

fn titles(db: &Db, scope: Scope, search: &str) -> Vec<String> {
    let q = db.timeline(scope, search, 50);
    let store = db.container.cache::<Article>();
    q.ids()
        .iter()
        .map(|id| {
            let _ = db.container.get::<Article>(*id);
            store.with_untracked(|k| {
                k.get(id.handle())
                    .and_then(|a| a.title.clone())
                    .unwrap_or_default()
            })
        })
        .collect()
}

#[test]
fn subscribing_twice_is_idempotent() {
    let db = Db::open_in_memory().unwrap();
    let a = db.add_feed("https://e.example/f", "Example", None);
    let b = db.add_feed("https://e.example/f", "Different name", None);
    assert_eq!(a, b, "same URL must not create a second subscription");
    assert_eq!(db.container.table_count::<daynews_db::Feed>().unwrap(), 1);
}

/// The property the whole reader depends on: re-importing the same items must not resurrect
/// articles the user already read.
#[test]
fn refresh_preserves_read_state_and_adds_only_new() {
    let db = Db::open_in_memory().unwrap();
    let url = "https://e.example/f";
    let f = db.add_feed(url, "Example", None);
    let first = vec![
        item("a", "Alpha", "one", 100),
        item("b", "Beta", "two", 200),
    ];
    assert_eq!(db.upsert_articles(f, url, &first), 2);

    db.set_read(article_id(url, "b"), true);
    let unread = db.unread_count(Scope::All);
    assert_eq!(unread.get_untracked(), 1);

    // The same feed again, plus one new item.
    let second = vec![
        item("a", "Alpha", "one", 100),
        item("b", "Beta", "two", 200),
        item("c", "Gamma", "three", 300),
    ];
    assert_eq!(db.upsert_articles(f, url, &second), 1, "only the new item");
    assert_eq!(db.count(Scope::All).get_untracked(), 3);
    assert_eq!(unread.get_untracked(), 2, "the read article stays read");
}

#[test]
fn timeline_is_newest_first_and_scopes_filter() {
    let db = Db::open_in_memory().unwrap();
    let f1 = db.add_feed("https://a.example/f", "A", None);
    let _f2 = db.add_feed("https://b.example/f", "B", None);
    db.upsert_articles(f1, "https://a.example/f", &[item("1", "Older", "x", 100)]);
    db.upsert_articles(
        daynews_db::feed_id("https://b.example/f"),
        "https://b.example/f",
        &[item("2", "Newer", "y", 900)],
    );

    assert_eq!(titles(&db, Scope::All, ""), ["Newer", "Older"]);
    assert_eq!(titles(&db, Scope::Feed(f1), ""), ["Older"]);
}

#[test]
fn full_text_search_matches_title_and_body_across_two_shadows() {
    let db = Db::open_in_memory().unwrap();
    let url = "https://e.example/f";
    let f = db.add_feed(url, "E", None);
    db.upsert_articles(
        f,
        url,
        &[
            item("1", "Reticulating splines", "nothing here", 100),
            item("2", "Unrelated", "a body mentioning parallax", 200),
        ],
    );

    assert_eq!(titles(&db, Scope::All, "splines"), ["Reticulating splines"]);
    // `parallax` lives only in the body, a different model, reached through the relation
    // crossing in the search fetch.
    assert_eq!(titles(&db, Scope::All, "parallax"), ["Unrelated"]);
    // Live-as-you-type: a partial last token still matches.
    assert_eq!(titles(&db, Scope::All, "retic").len(), 1);
    // Punctuation is searched for, not read as FTS syntax.
    assert_eq!(titles(&db, Scope::All, "\"quoted -thing").len(), 0);
    assert_eq!(titles(&db, Scope::All, "zzzznotfound").len(), 0);
    // Diacritics fold through the declared tokenizer.
    db.upsert_articles(f, url, &[item("3", "École buissonnière", "z", 300)]);
    assert_eq!(titles(&db, Scope::All, "ecole"), ["École buissonnière"]);
}

#[test]
fn deleting_a_feed_cascades_articles_bodies_and_the_index() {
    let db = Db::open_in_memory().unwrap();
    let url = "https://e.example/f";
    let f = db.add_feed(url, "E", None);
    db.upsert_articles(f, url, &[item("1", "Findme", "body", 100)]);
    assert_eq!(titles(&db, Scope::All, "Findme").len(), 1);
    assert!(db.body(article_id(url, "1")).is_some());

    db.delete_feed(f);
    assert_eq!(db.count(Scope::All).get_untracked(), 0, "articles cascaded");
    assert_eq!(
        titles(&db, Scope::All, "Findme").len(),
        0,
        "no phantom FTS rows"
    );
    assert_eq!(
        db.container
            .table_count::<daynews_db::ArticleBody>()
            .unwrap(),
        0,
        "bodies cascaded too"
    );
}

#[test]
fn deleting_a_folder_cascades_through_feeds_to_articles() {
    let db = Db::open_in_memory().unwrap();
    let tech = db.add_folder("Tech");
    let url = "https://a.example/f";
    let f = db.add_feed(url, "A", Some(tech));
    db.upsert_articles(f, url, &[item("1", "in folder", "x", 1)]);
    let _keep = db.add_feed("https://b.example/f", "B", None);

    db.delete_folder(tech);
    assert!(
        db.container.get::<daynews_db::Feed>(f).is_none(),
        "feed went"
    );
    assert_eq!(db.count(Scope::All).get_untracked(), 0, "articles went");
    assert!(
        db.container
            .get::<daynews_db::Feed>(daynews_db::feed_id("https://b.example/f"))
            .is_some(),
        "the top-level feed stayed"
    );
}

#[test]
fn mark_all_read_respects_scope_and_is_reversible() {
    let db = Db::open_in_memory().unwrap();
    let (ua, ub) = ("https://a.example/f", "https://b.example/f");
    let f1 = db.add_feed(ua, "A", None);
    let f2 = db.add_feed(ub, "B", None);
    db.upsert_articles(f1, ua, &[item("1", "a1", "x", 1), item("2", "a2", "x", 2)]);
    db.upsert_articles(f2, ub, &[item("3", "b1", "x", 3)]);
    let unread = db.unread_count(Scope::All);
    assert_eq!(unread.get_untracked(), 3);

    assert_eq!(db.set_read_all(Scope::Feed(f1), true), 2);
    assert_eq!(unread.get_untracked(), 1, "only feed A was marked");
    assert_eq!(db.unread_count(Scope::Feed(f2)).get_untracked(), 1);
    assert_eq!(db.unread_count(Scope::Feed(f1)).get_untracked(), 0);

    db.set_read_all(Scope::All, true);
    assert_eq!(unread.get_untracked(), 0);
    db.set_read_all(Scope::All, false);
    assert_eq!(unread.get_untracked(), 3, "unread-all is reversible");
}

#[test]
fn folders_scope_the_timeline_through_the_relation() {
    let db = Db::open_in_memory().unwrap();
    let tech = db.add_folder("Tech");
    let url = "https://a.example/f";
    let f1 = db.add_feed(url, "A", Some(tech));
    let _f2 = db.add_feed("https://b.example/f", "B", None);
    db.upsert_articles(f1, url, &[item("1", "in folder", "x", 1)]);

    assert_eq!(titles(&db, Scope::Folder(tech), ""), ["in folder"]);
    db.set_read_all(Scope::Folder(tech), true);
    assert_eq!(db.unread_count(Scope::All).get_untracked(), 0);
}

#[test]
fn tags_cross_articles_and_scope_the_timeline() {
    let db = Db::open_in_memory().unwrap();
    let url = "https://e.example/f";
    let f = db.add_feed(url, "E", None);
    db.upsert_articles(
        f,
        url,
        &[item("1", "Tagged", "x", 100), item("2", "Plain", "y", 200)],
    );
    let keep = db.add_tag("keep");
    let a1 = article_id(url, "1");

    db.set_tagged(a1, keep, true);
    assert_eq!(titles(&db, Scope::Tag(keep), ""), ["Tagged"]);
    assert_eq!(db.count(Scope::Tag(keep)).get_untracked(), 1);

    db.set_tagged(a1, keep, false);
    assert_eq!(db.count(Scope::Tag(keep)).get_untracked(), 0);
    // The tag itself survives an untag; deleting the article drops the membership.
    db.set_tagged(a1, keep, true);
    db.delete_feed(f);
    assert_eq!(db.count(Scope::Tag(keep)).get_untracked(), 0);
    assert!(db.container.get::<daynews_db::Tag>(keep).is_some());
}

#[test]
fn retention_prunes_old_read_articles_but_never_starred_or_tagged() {
    let db = Db::open_in_memory().unwrap();
    let url = "https://e.example/f";
    let f = db.add_feed(url, "E", None);
    let old = daynews_db::start_of_today() - 400 * 86_400;
    let items = vec![
        item("old-read", "Old read", "x", old),
        item("old-starred", "Old starred", "x", old),
        item("old-tagged", "Old tagged", "x", old),
        item("old-unread", "Old unread", "x", old),
        item("fresh", "Fresh", "x", daynews_db::start_of_today()),
    ];
    // `first_seen_at` is stamped at insert; backdate it through the field so the pruner sees
    // rows old enough to prune.
    db.upsert_articles(f, url, &items);
    let store = db.container.cache::<Article>();
    for guid in ["old-read", "old-starred", "old-tagged", "old-unread"] {
        let id = article_id(url, guid);
        let _ = db.container.get::<Article>(id);
        use day_reactive::Binding;
        store.elem(id).first_seen_at().write(old);
    }
    db.set_read(article_id(url, "old-read"), true);
    db.set_read(article_id(url, "old-starred"), true);
    db.set_read(article_id(url, "old-tagged"), true);
    db.set_starred(article_id(url, "old-starred"), true);
    let keep = db.add_tag("keep");
    db.set_tagged(article_id(url, "old-tagged"), keep, true);

    assert_eq!(
        db.prune_older_than(90),
        1,
        "only the old READ plain article"
    );
    let left = titles(&db, Scope::All, "");
    assert!(left.contains(&"Old starred".to_string()));
    assert!(left.contains(&"Old tagged".to_string()));
    assert!(
        left.contains(&"Old unread".to_string()),
        "unread is never pruned"
    );
    assert!(left.contains(&"Fresh".to_string()));
    assert!(!left.contains(&"Old read".to_string()));
}

#[test]
fn undated_items_sort_by_first_seen_not_the_epoch() {
    let db = Db::open_in_memory().unwrap();
    let url = "https://e.example/f";
    let f = db.add_feed(url, "E", None);
    let mut undated = item("u", "No date", "x", 0);
    undated.published = None;
    db.upsert_articles(f, url, &[undated]);
    let id = db.timeline(Scope::All, "", 10).first().unwrap();
    let _ = db.container.get::<Article>(id);
    let published = db
        .container
        .cache::<Article>()
        .with_untracked(|k| k.get(id.handle()).map(|a| a.published_at).unwrap_or(0));
    assert!(
        published > 1_600_000_000,
        "undated item got a sensible time, got {published}"
    );
}

#[test]
fn feed_errors_are_recorded_and_cleared_by_a_good_refresh() {
    let db = Db::open_in_memory().unwrap();
    let url = "https://e.example/f";
    let f = db.add_feed(url, "E", None);
    db.set_feed_error(f, "HTTP 404");
    use day_reactive::Binding;
    let feed = db.container.get::<daynews_db::Feed>(f).unwrap();
    assert_eq!(feed.last_error().peek().as_deref(), Some("HTTP 404"));

    db.update_feed_metadata(f, Some("Recovered"), None, None, None);
    assert_eq!(feed.last_error().peek(), None);
    assert_eq!(feed.title().peek(), "Recovered");
    // An empty <title> must not blank a named subscription.
    db.update_feed_metadata(f, Some("   "), None, None, None);
    assert_eq!(feed.title().peek(), "Recovered");
}

#[test]
fn undo_restores_an_unsubscribed_feeds_whole_subtree() {
    let db = Db::open_in_memory().unwrap();
    let stack = db.container.undo(100);
    let url = "https://e.example/f";
    let f = db.add_feed(url, "E", None);
    db.upsert_articles(f, url, &[item("1", "Kept by undo", "body text", 100)]);
    // A turn boundary seals the setup into its own undo unit, as the app's turns do.
    day_reactive::flush_sync();
    assert_eq!(db.count(Scope::All).get_untracked(), 1);

    db.delete_feed(f);
    day_reactive::flush_sync();
    assert_eq!(db.count(Scope::All).get_untracked(), 0);

    assert!(stack.undo());
    assert_eq!(
        db.count(Scope::All).get_untracked(),
        1,
        "the article came back"
    );
    assert!(
        db.container.get::<daynews_db::Feed>(f).is_some(),
        "the feed came back"
    );
    assert!(
        db.body(article_id(url, "1")).is_some(),
        "and the BODY row came back with it"
    );
    assert_eq!(titles(&db, Scope::All, "body"), ["Kept by undo"]);
}

#[test]
fn conditional_check_persists_validators_without_changing_articles() {
    let root = std::env::temp_dir().join(format!("day-news-validators-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let file = root.join("store.sqlite");
    let url = "https://fixture.example/conditional";
    let f;
    {
        let db = Db::open(&file).unwrap();
        f = db.add_feed(url, "Fixture", None);
        db.upsert_articles(
            f,
            url,
            &[item("one", "Fixture article", "Original body", 100)],
        );
        db.set_feed_body_hash(f, Some("synthetic-fingerprint".into()));
        db.set_read(article_id(url, "one"), true);
        db.set_starred(article_id(url, "one"), true);
        db.feed_checked(
            f,
            Some("W/\"revision-1\"".into()),
            Some("Wed, 30 Sep 2026 10:00:00 GMT".into()),
        );
        db.container.save().unwrap();
    }
    {
        let db = Db::open(&file).unwrap();
        assert_eq!(
            db.feed_cache(f).unwrap().body_hash.as_deref(),
            Some("synthetic-fingerprint")
        );
        let validators = db.feed_validators(f).unwrap();
        assert_eq!(validators.0.as_deref(), Some("W/\"revision-1\""));
        db.set_feed_error(f, "synthetic failure");
        db.feed_checked(f, validators.0, validators.1);
        assert_eq!(db.count(Scope::All).get_untracked(), 1);
        assert_eq!(db.unread_count(Scope::All).get_untracked(), 0);
        assert_eq!(db.count(Scope::Starred).get_untracked(), 1);
        assert_eq!(
            db.body(article_id(url, "one")).as_deref(),
            Some("<p>Original body</p>")
        );
        assert!(
            db.container
                .get::<daynews_db::Feed>(f)
                .unwrap()
                .last_error()
                .peek()
                .is_none()
        );
    }
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn identical_metadata_does_not_emit_redundant_field_writes() {
    let db = Db::open_in_memory().unwrap();
    let f = db.add_feed("https://fixture.example/no-op", "Fixture", None);
    db.update_feed_metadata(
        f,
        Some("Fixture"),
        Some("https://fixture.example"),
        None,
        None,
    );
    db.feed_checked(f, Some("\"tag\"".into()), None);
    db.set_feed_body_hash(f, Some("synthetic-hash".into()));
    db.container.save().unwrap();
    let (_, changes) = day_model::record_changes(|| {
        db.update_feed_metadata(
            f,
            Some("Fixture"),
            Some("https://fixture.example"),
            None,
            None,
        );
        db.feed_checked(f, Some("\"tag\"".into()), None);
        db.set_feed_body_hash(f, Some("synthetic-hash".into()));
    });
    // The timestamp may cross a second boundary; every other field must stay quiet.
    assert!(
        changes
            .iter()
            .all(|change| matches!(change.label, "last_fetched_at" | "next_poll_at"))
    );
}

#[test]
fn duplicate_feed_entries_are_inserted_once() {
    let db = Db::open_in_memory().unwrap();
    let url = "https://fixture.example/duplicates";
    let f = db.add_feed(url, "Fixture", None);
    assert_eq!(db.try_upsert_articles(f, url, &[]).unwrap(), 0);
    let items = [
        item("same", "First fixture", "first", 100),
        item("same", "Second fixture", "second", 200),
    ];
    assert_eq!(db.try_upsert_articles(f, url, &items).unwrap(), 1);
    assert_eq!(
        db.body(article_id(url, "same")).as_deref(),
        Some("<p>first</p>")
    );
}

#[test]
fn retry_deadline_survives_reopen_and_success_clears_it() {
    let root = std::env::temp_dir().join(format!("day-news-retry-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let file = root.join("store.sqlite");
    let id;
    {
        let db = Db::open(&file).unwrap();
        id = db.add_feed("https://fixture.example/busy", "Fixture", None);
        db.set_feed_retry_after(id, Some(2_000_000_000));
        db.container.save().unwrap();
    }
    {
        let db = Db::open(&file).unwrap();
        assert_eq!(db.feed_cache(id).unwrap().retry_after, Some(2_000_000_000));
        db.feed_checked(id, None, None);
        assert_eq!(db.feed_cache(id).unwrap().retry_after, None);
    }
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn adaptive_due_times_priority_and_failure_backoff_are_durable() {
    let db = Db::open_in_memory().unwrap();
    let now = daynews_time::now_unix();
    let fast = db.add_feed("https://fixture.example/fast", "Fast", None);
    let slow = db.add_feed("https://fixture.example/monthly", "Monthly", None);
    db.record_publications(fast, (0..12).map(|i| now - i * 4 * 3600));
    db.record_publications(slow, (0..12).map(|i| now - i * 30 * 86400));
    db.feed_checked(fast, None, None);
    db.feed_checked(slow, None, None);
    let fast_due = db
        .container
        .get::<daynews_db::Feed>(fast)
        .unwrap()
        .next_poll_at()
        .peek()
        .unwrap();
    let slow_due = db
        .container
        .get::<daynews_db::Feed>(slow)
        .unwrap()
        .next_poll_at()
        .peek()
        .unwrap();
    assert!((1800..=1802).contains(&(fast_due - now)));
    assert!((86400..=86402).contains(&(slow_due - now)));
    assert!(db.due_feeds(now).unwrap().is_empty());
    assert_eq!(
        db.due_feeds(fast_due)
            .unwrap()
            .iter()
            .map(|f| f.0)
            .collect::<Vec<_>>(),
        vec![fast]
    );
    db.move_feed(slow, 0).unwrap();
    assert_eq!(
        db.due_feeds(slow_due)
            .unwrap()
            .iter()
            .map(|f| f.0)
            .collect::<Vec<_>>(),
        vec![slow, fast]
    );
    db.set_feed_error(fast, "synthetic failure");
    let row = db.container.get::<daynews_db::Feed>(fast).unwrap();
    assert_eq!(row.poll_failures().peek(), 1);
    assert!(row.next_poll_at().peek().unwrap() >= now + 3600);
    db.feed_checked(fast, None, None);
    assert_eq!(row.poll_failures().peek(), 0);
}

#[test]
fn adaptive_history_and_reordered_priority_survive_reopen() {
    let root = std::env::temp_dir().join(format!("daynews-polling-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let file = root.join("store.sqlite");
    let (fast, slow);
    {
        let db = Db::open(&file).unwrap();
        fast = db.add_feed("https://fixture.example/persist-fast", "Fast", None);
        slow = db.add_feed("https://fixture.example/persist-slow", "Slow", None);
        assert!(db.feed_cache(fast).unwrap().needs_publication_sample);
        db.record_publications(fast, [daynews_time::now_unix()]);
        db.record_publications(slow, []);
        db.feed_checked(fast, Some("fixture-etag".into()), None);
        db.feed_checked(slow, None, None);
        db.move_feed(slow, 0).unwrap();
        db.container.save().unwrap();
    }
    {
        let db = Db::open(&file).unwrap();
        assert!(!db.feed_cache(fast).unwrap().needs_publication_sample);
        assert!(!db.feed_cache(slow).unwrap().needs_publication_sample);
        assert_eq!(
            db.feed_cache(fast).unwrap().etag.as_deref(),
            Some("fixture-etag")
        );
        assert!(db.due_feeds(daynews_time::now_unix()).unwrap().is_empty());
        assert_eq!(
            db.due_feeds(i64::MAX)
                .unwrap()
                .iter()
                .map(|f| f.0)
                .collect::<Vec<_>>(),
            vec![slow, fast]
        );
    }
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn dashboards_measure_full_scopes_and_reuse_article_metrics_on_feed_checks() {
    use daynews_db::dashboard::DashboardCache;
    let db = Db::open_in_memory().unwrap();
    let now = daynews_time::now_unix();
    let url = "https://fixture.example/dashboard";
    let feed = db.add_feed(url, "Dashboard fixture", None);
    let other = db.add_feed("https://fixture.example/other", "Other fixture", None);
    db.upsert_articles(
        feed,
        url,
        &[
            item("one", "Short story", "one two three", now),
            item(
                "two",
                "Longer story",
                &"fixture word ".repeat(200),
                now - 86400 * 9,
            ),
        ],
    );
    db.upsert_articles(
        other,
        "https://fixture.example/other",
        &[item("three", "Other story", "hello fixture", now)],
    );
    db.set_starred(article_id(url, "one"), true);
    let mut cache = DashboardCache::default();
    let first = db
        .dashboard_cached(Scope::Feed(feed), now, &mut cache)
        .unwrap();
    assert_eq!(first.feed_id, Some(feed));
    assert_eq!(first.total, 2);
    assert_eq!(first.words, 403);
    assert_eq!(first.authors, 1);
    assert_eq!(first.lengths, [1, 0, 1, 0, 0]);
    assert_eq!(first.days.iter().map(|s| s.1).sum::<u64>(), 2);
    assert_eq!(first.habits.iter().flatten().sum::<u64>(), 2);
    assert_eq!(first.sources, vec![(feed, "Dashboard fixture".into(), 2)]);
    db.record_publications(feed, [now - 60, now - 86400 * 9]);
    db.feed_checked(feed, None, None);
    let checked = db
        .dashboard_cached(Scope::Feed(feed), now, &mut cache)
        .unwrap();
    assert_eq!(checked.words, first.words);
    assert!(checked.feeds[0].next.is_some());
    db.set_read(article_id(url, "one"), true);
    let read = db
        .dashboard_cached(Scope::Feed(feed), now, &mut cache)
        .unwrap();
    assert_eq!(
        read.unread, 1,
        "cached aggregates invalidate on an article edit"
    );
    assert_eq!(db.dashboard(Scope::Unread, now).unwrap().total, 2);
    assert_eq!(db.dashboard(Scope::Starred, now).unwrap().total, 1);
    assert_eq!(db.dashboard(Scope::Today, now).unwrap().total, 2);
    assert_eq!(db.dashboard(Scope::All, now).unwrap().total, 3);
    let tag = db.add_tag("Fixture dashboard tag");
    db.set_tagged(article_id(url, "one"), tag, true);
    db.set_tagged(
        article_id("https://fixture.example/other", "three"),
        tag,
        true,
    );
    let tagged = db.dashboard(Scope::Tag(tag), now).unwrap();
    assert_eq!(tagged.total, 2);
    assert_eq!(tagged.words, 5);
    assert_eq!(tagged.sources.len(), 2);
    db.set_tagged(article_id(url, "one"), tag, false);
    assert_eq!(db.dashboard(Scope::Tag(tag), now).unwrap().total, 1);
    assert_eq!(
        db.dashboard(Scope::Today, now)
            .unwrap()
            .hourly
            .iter()
            .sum::<u64>(),
        2
    );
}

#[test]
fn word_count_migration_is_persistent_and_only_processes_missing_metrics() {
    let root = std::env::temp_dir().join(format!(
        "daynews-dashboard-migration-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let path = root.join("store.sqlite");
    let id;
    {
        let db = Db::open(&path).unwrap();
        let url = "https://fixture.example/migration";
        let feed = db.add_feed(url, "Fixture", None);
        db.upsert_articles(feed, url, &[item("one", "Fixture", "one two three", 100)]);
        id = article_id(url, "one");
        db.container
            .get::<Article>(id)
            .unwrap()
            .word_count()
            .write(None);
        db.container.save().unwrap();
    }
    {
        let db = Db::open(&path).unwrap();
        assert_eq!(
            db.container.get::<Article>(id).unwrap().word_count().peek(),
            Some(3)
        );
        assert_eq!(db.dashboard(Scope::All, 200).unwrap().words, 3);
    }
    {
        let db = Db::open(&path).unwrap();
        assert_eq!(
            db.container.get::<Article>(id).unwrap().word_count().peek(),
            Some(3)
        );
    }
    std::fs::remove_dir_all(root).unwrap();
}
