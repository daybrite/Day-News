//! The store's contracts, headless: deterministic identity, read-state preservation across
//! refreshes, scoped timelines, two-shadow full-text search, deep cascades, tags, retention,
//! and live count badges.

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
    // `parallax` lives only in the BODY — a different model, reached through the relation
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
    // The tag itself survives an untag; deleting the ARTICLE drops the membership.
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
    // genuinely old rows.
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
