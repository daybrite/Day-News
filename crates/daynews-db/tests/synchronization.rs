use daynews_db::{
    Article, ArticleFields, Db,
    sync::{Account, EntryLink},
};
use daynews_sync::{
    engine::{Storage, synchronize},
    fixture::Server,
    *,
};
use futures_util::FutureExt;
use std::{
    future::Future,
    sync::Arc,
    task::{Context, Poll, Wake, Waker},
};
struct Thread(std::thread::Thread);
impl Wake for Thread {
    fn wake(self: Arc<Self>) {
        self.0.unpark();
    }
}
fn wait<T>(future: impl Future<Output = T>) -> T {
    let w = Waker::from(Arc::new(Thread(std::thread::current())));
    let mut f = std::pin::pin!(future);
    loop {
        match f.as_mut().poll(&mut Context::from_waker(&w)) {
            Poll::Ready(v) => return v,
            Poll::Pending => std::thread::park(),
        }
    }
}
struct Replica {
    db: Db,
    account: u64,
}
impl Replica {
    fn new() -> Self {
        Self::from_db(Db::open_in_memory().unwrap())
    }
    fn from_db(db: Db) -> Self {
        let account = db
            .sync_connect("feedbin", "fixture@example.invalid")
            .unwrap();
        Self { db, account }
    }
    fn article(&self, remote: &str) -> u64 {
        self.db
            .container
            .query::<EntryLink>()
            .live()
            .try_collect()
            .unwrap()
            .into_iter()
            .find(|e| e.remote == remote)
            .unwrap()
            .article
    }
    fn read(&self, remote: &str) -> bool {
        use day_reactive::Binding;
        self.db
            .container
            .get::<Article>(self.article(remote))
            .unwrap()
            .is_read()
            .peek()
    }
    fn set_read(&self, remote: &str, read: bool) {
        self.db.set_read(self.article(remote), read).unwrap();
        self.db.container.save().unwrap();
    }
    fn saved<T>(&self, result: std::result::Result<T, day_persistence::DbError>) -> Result<T> {
        let v = result.map_err(|_| Error::Storage)?;
        self.db.container.save().map_err(|_| Error::Storage)?;
        Ok(v)
    }
}
impl Storage for Replica {
    fn checkpoint(&self) -> Call<'_, Option<String>> {
        async {
            use day_reactive::Binding;
            use daynews_db::sync::AccountFields;
            Ok(self
                .db
                .container
                .get::<Account>(self.account)
                .and_then(|a| a.checkpoint().peek()))
        }
        .boxed_local()
    }

    fn begin_entries(&self) -> Call<'_, bool> {
        async { self.saved(self.db.sync_begin_entries(self.account)) }.boxed_local()
    }
    fn pending(&self) -> Call<'_, Vec<Operation>> {
        async { self.saved(self.db.sync_pending(self.account)) }.boxed_local()
    }
    fn outcomes(&self, o: Vec<Outcome>) -> Call<'_, ()> {
        async move { self.saved(self.db.sync_outcomes(self.account, o)) }.boxed_local()
    }
    fn subscriptions(&self, c: Changes<Subscription>) -> Call<'_, ()> {
        async move { self.saved(self.db.sync_apply_subscriptions(self.account, c)) }.boxed_local()
    }
    fn missing(&self, ids: Vec<String>) -> Call<'_, Vec<String>> {
        async move { Ok(self.db.sync_missing(self.account, ids)) }.boxed_local()
    }
    fn entries(&self, e: Vec<Entry>, s: ReadChanges) -> Call<'_, bool> {
        async move { self.saved(self.db.sync_import_entries(self.account, e, &s)) }.boxed_local()
    }
    fn reading(&self, s: ReadChanges) -> Call<'_, ()> {
        async move { self.saved(self.db.sync_apply_read(self.account, s)) }.boxed_local()
    }
    fn finished(&self, checkpoint: Option<String>) -> Call<'_, ()> {
        async move {
            use day_reactive::Binding;
            use daynews_db::sync::AccountFields;
            let a = self.db.container.get::<Account>(self.account).unwrap();
            a.last_sync().write(Some(daynews_time::now_unix()));
            a.checkpoint().write(checkpoint);
            a.importing().write(false);
            self.saved(Ok(()))
        }
        .boxed_local()
    }
}

#[test]
fn two_installations_read_unread_and_restart_through_real_http_interface() {
    let server = Server::new();
    let provider = server.provider();
    let a = Replica::new();
    let b = Replica::new();
    wait(provider.authenticate()).unwrap();
    wait(synchronize(&provider, &a)).unwrap();
    wait(synchronize(&provider, &b)).unwrap();
    assert!(!a.read("1001"));
    assert!(!b.read("1001"));
    a.set_read("1001", true);
    assert_eq!(a.db.sync_pending_count(a.account), 1);
    wait(synchronize(&provider, &a)).unwrap();
    wait(synchronize(&provider, &b)).unwrap();
    assert!(b.read("1001"));
    assert!(!server.unread().contains(&1001));
    b.set_read("1001", false);
    wait(synchronize(&provider, &b)).unwrap();
    wait(synchronize(&provider, &a)).unwrap();
    assert!(!a.read("1001"));
    assert_eq!(a.db.sync_pending_count(a.account), 0);
    assert!(
        server
            .simulation
            .requests()
            .iter()
            .any(|r| r.method == "POST" && r.url.ends_with("unread_entries/delete.json"))
    );
    assert!(
        server
            .simulation
            .requests()
            .iter()
            .any(|r| r.url.contains("page=2"))
    );
}

#[test]
fn unchanged_read_snapshot_does_not_dirty_library_rows() {
    let server = Server::new();
    let p = server.provider();
    let a = Replica::new();
    wait(synchronize(&p, &a)).unwrap();
    let entries = a.db.container.cache::<EntryLink>();
    let articles = a.db.container.cache::<Article>();
    let before = (entries.version(), articles.version());
    a.db.sync_apply_read(a.account, wait(p.read_state()).unwrap())
        .unwrap();
    assert_eq!(before, (entries.version(), articles.version()));
}

#[test]
fn acknowledgement_cannot_clear_newer_local_intent() {
    let server = Server::new();
    let p = server.provider();
    let a = Replica::new();
    wait(synchronize(&p, &a)).unwrap();
    a.set_read("1001", true);
    let sent = a.db.sync_pending(a.account).unwrap();
    a.set_read("1001", false);
    let outcomes = wait(p.apply(sent)).unwrap();
    a.db.sync_outcomes(a.account, outcomes).unwrap();
    assert_eq!(a.db.sync_pending_count(a.account), 1);
    a.db.sync_apply_read(a.account, ReadChanges::UnreadSnapshot(vec![]))
        .unwrap();
    assert!(!a.read("1001"));
    wait(synchronize(&p, &a)).unwrap();
    assert!(server.unread().contains(&1001));
}

#[test]
fn partial_acknowledgement_retains_failure_and_downloads_continue() {
    let server = Server::new();
    let p = server.provider();
    let a = Replica::new();
    wait(synchronize(&p, &a)).unwrap();
    a.set_read("1001", true);
    a.set_read("1002", true);
    server.reject(1001);
    let report = wait(synchronize(&p, &a)).unwrap();
    assert_eq!(report.acknowledged, 1);
    assert_eq!(report.pending_error, Some(Error::Rejected));
    assert_eq!(a.db.sync_pending_count(a.account), 1);
    assert!(a.read("1001"));
    assert!(server.unread().contains(&1001));
    assert!(!server.unread().contains(&1002));
}

#[test]
fn offline_intent_survives_database_reopen() {
    let path = std::env::temp_dir().join(format!(
        "daynews-sync-{}-{}.sqlite",
        std::process::id(),
        daynews_time::now_epoch_ms()
    ));
    let server = Server::new();
    let p = server.provider();
    {
        let a = Replica::from_db(Db::open(&path).unwrap());
        wait(synchronize(&p, &a)).unwrap();
        a.set_read("1001", true);
    }
    {
        let b = Replica::from_db(Db::open(&path).unwrap());
        assert_eq!(b.db.sync_pending_count(b.account), 1);
        wait(synchronize(&p, &b)).unwrap();
        assert!(!server.unread().contains(&1001));
    }
    let _ = std::fs::remove_file(path);
}

#[test]
fn removal_does_not_resurrect_from_an_offline_replica() {
    use day_reactive::Binding;
    let server = Server::new();
    let p = server.provider();
    let a = Replica::new();
    let b = Replica::new();
    wait(synchronize(&p, &a)).unwrap();
    wait(synchronize(&p, &b)).unwrap();
    let feed =
        a.db.container
            .query::<daynews_db::Feed>()
            .live()
            .try_collect()
            .unwrap()[0]
            .id;
    a.db.delete_feed(feed).unwrap();
    wait(synchronize(&p, &a)).unwrap();
    wait(synchronize(&p, &b)).unwrap();
    assert!(matches!(wait(p.subscriptions()).unwrap(),Changes::Snapshot(v) if v.is_empty()));
    use daynews_db::FeedFields;
    assert!(
        b.db.container
            .get::<daynews_db::Feed>(feed)
            .unwrap()
            .unsubscribed()
            .peek()
    );
    assert!(b.db.container.get::<Account>(b.account).is_some());
}

#[test]
fn undo_before_and_after_acknowledgement_queues_the_restored_state() {
    use day_reactive::Binding;
    let server = Server::new();
    let p = server.provider();
    let a = Replica::new();
    wait(synchronize(&p, &a)).unwrap();
    a.set_read("1001", true);
    // The undo stack restores model fields; reconciliation must replace the pending read.
    a.db.container
        .get::<Article>(a.article("1001"))
        .unwrap()
        .is_read()
        .write(false);
    a.db.sync_after_undo().unwrap();
    wait(synchronize(&p, &a)).unwrap();
    assert!(server.unread().contains(&1001));
    a.set_read("1001", true);
    wait(synchronize(&p, &a)).unwrap();
    a.db.container
        .get::<Article>(a.article("1001"))
        .unwrap()
        .is_read()
        .write(false);
    a.db.sync_after_undo().unwrap();
    wait(synchronize(&p, &a)).unwrap();
    assert!(server.unread().contains(&1001));
}

#[test]
fn delete_and_rename_before_subscription_ack_are_not_lost() {
    let server = Server::new();
    let p = server.provider();
    let a = Replica::new();
    let feed =
        a.db.add_feed("https://fixture.invalid/new.xml", "New", None);
    a.db.sync_adopt_local(feed).unwrap();
    a.db.rename_feed(feed, "Renamed while offline").unwrap();
    wait(synchronize(&p, &a)).unwrap();
    wait(synchronize(&p, &a)).unwrap();
    assert!(
        matches!(wait(p.subscriptions()).unwrap(),Changes::Snapshot(v) if v.iter().any(|s|s.title=="Renamed while offline"))
    );
    let other =
        a.db.add_feed("https://fixture.invalid/deleted.xml", "Deleted", None);
    a.db.sync_adopt_local(other).unwrap();
    a.db.delete_feed(other).unwrap();
    wait(synchronize(&p, &a)).unwrap();
    wait(synchronize(&p, &a)).unwrap();
    assert!(
        matches!(wait(p.subscriptions()).unwrap(),Changes::Snapshot(v) if !v.iter().any(|s|s.url.ends_with("deleted.xml")))
    );
}

#[test]
fn provider_neutral_ids_and_deltas_do_not_remove_unmentioned_subscriptions() {
    let a = Replica::new();
    let sub = Subscription {
        id: "opaque:subscription/a".into(),
        feed_id: "urn:feed:x".into(),
        url: "https://fixture.invalid/other.xml".into(),
        title: "Other protocol".into(),
        site_url: None,
    };
    a.db.sync_apply_subscriptions(a.account, Changes::Snapshot(vec![sub.clone()]))
        .unwrap();
    a.db.sync_import_entries(
        a.account,
        vec![Entry {
            id: "opaque:entry/a".into(),
            feed_id: sub.feed_id.clone(),
            title: None,
            url: None,
            author: None,
            content: None,
            summary: None,
            published: 0,
        }],
        &ReadChanges::Delta(vec![]),
    )
    .unwrap();
    a.db.sync_apply_read(
        a.account,
        ReadChanges::Delta(vec![ReadState {
            id: "opaque:entry/a".into(),
            read: true,
        }]),
    )
    .unwrap();
    assert!(a.read("opaque:entry/a"));
    a.db.sync_apply_subscriptions(
        a.account,
        Changes::Delta {
            upserts: vec![],
            removed: vec![],
        },
    )
    .unwrap();
    use day_reactive::Binding;
    use daynews_db::FeedFields;
    let feed = daynews_db::feed_id(&sub.url);
    assert!(
        !a.db
            .container
            .get::<daynews_db::Feed>(feed)
            .unwrap()
            .unsubscribed()
            .peek()
    );
}

#[test]
fn failed_worker_transaction_rolls_back_article_and_outbox_together() {
    use day_persistence::{DatabaseWorker, DbError};
    use day_reactive::Binding;
    let w = wait(DatabaseWorker::open(|| {
        Db::open_in_memory().map(|d| d.container)
    }))
    .unwrap();
    let article = wait(w.write(|c| {
        let db = Db {
            container: c.clone(),
        };
        let a = db.sync_connect("fixture", "transaction@example.invalid")?;
        db.sync_apply_subscriptions(
            a,
            Changes::Snapshot(vec![Subscription {
                id: "sub".into(),
                feed_id: "feed".into(),
                url: "https://fixture.invalid/transaction".into(),
                title: "Fixture".into(),
                site_url: None,
            }]),
        )?;
        db.sync_import_entries(
            a,
            vec![Entry {
                id: "entry".into(),
                feed_id: "feed".into(),
                title: None,
                url: None,
                author: None,
                content: None,
                summary: None,
                published: 0,
            }],
            &ReadChanges::UnreadSnapshot(vec!["entry".into()]),
        )?;
        Ok(c.query::<EntryLink>().live().try_collect()?[0].article)
    }))
    .unwrap();
    let failed: std::result::Result<(), _> = wait(w.write(move |c| {
        Db {
            container: c.clone(),
        }
        .set_read(article, true)?;
        Err(DbError::driver("synthetic transaction failure"))
    }));
    assert!(failed.is_err());
    wait(w.read(move |c| {
        assert!(!c.try_get::<Article>(article)?.unwrap().is_read().peek());
        assert_eq!(c.table_count::<daynews_db::sync::Intent>()?, 0);
        Ok(())
    }))
    .unwrap();
    wait(w.close()).unwrap();
}

#[test]
fn real_undo_stack_preserves_acknowledgements_and_reconciles_unsubscribe() {
    use day_reactive::Binding;
    use daynews_db::FeedFields;
    let server = Server::new();
    let p = server.provider();
    let a = Replica::new();
    wait(synchronize(&p, &a)).unwrap();
    let stack = a.db.container.undo(20);
    a.set_read("1001", true);
    day_reactive::flush_sync();
    day_model::with_author("sync", || wait(synchronize(&p, &a))).unwrap();
    assert_eq!(a.db.sync_pending_count(a.account), 0);
    assert!(stack.undo());
    a.db.sync_after_undo().unwrap();
    day_model::with_author("sync", || wait(synchronize(&p, &a))).unwrap();
    assert!(server.unread().contains(&1001));
    let feed = daynews_db::feed_id("https://fixture.invalid/feed.xml");
    a.db.delete_feed(feed).unwrap();
    day_reactive::flush_sync();
    day_model::with_author("sync", || wait(synchronize(&p, &a))).unwrap();
    assert!(stack.undo());
    a.db.sync_after_undo().unwrap();
    day_model::with_author("sync", || wait(synchronize(&p, &a))).unwrap();
    assert!(
        !a.db
            .container
            .get::<daynews_db::Feed>(feed)
            .unwrap()
            .unsubscribed()
            .peek()
    );
    assert!(matches!(wait(p.subscriptions()).unwrap(),Changes::Snapshot(v) if !v.is_empty()));
}

/// A deliberately non-Feedbin protocol with string IDs, read deltas and opaque checkpoints.
struct DeltaProvider {
    restored: std::cell::RefCell<Option<String>>,
    fail: std::cell::Cell<bool>,
}
impl Provider for DeltaProvider {
    fn kind(&self) -> &'static str {
        "fixture-delta"
    }
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            subscriptions: true,
            read_state: true,
            entries: true,
        }
    }
    fn restore_checkpoint(&self, checkpoint: Option<String>) -> Result<()> {
        *self.restored.borrow_mut() = checkpoint;
        Ok(())
    }
    fn checkpoint(&self) -> Option<String> {
        Some("opaque/version:2".into())
    }
    fn authenticate(&self) -> Call<'_, ()> {
        async { Ok(()) }.boxed_local()
    }
    fn subscriptions(&self) -> Call<'_, Changes<Subscription>> {
        async {
            if self.restored.borrow().is_some() {
                return Ok(Changes::Unchanged);
            }
            Ok(Changes::Snapshot(vec![Subscription {
                id: "subscription:other".into(),
                feed_id: "feed:other".into(),
                url: "https://fixture.invalid/delta".into(),
                title: "Delta fixture".into(),
                site_url: None,
            }]))
        }
        .boxed_local()
    }
    fn entries(&self, _cursor: Option<String>) -> Call<'_, Page<Entry>> {
        async {
            if self.fail.replace(false) {
                return Err(Error::Network);
            }
            Ok(Page {
                total_pages: None,
                values: if self.restored.borrow().is_some() {
                    vec![]
                } else {
                    vec![Entry {
                        id: "entry:other".into(),
                        feed_id: "feed:other".into(),
                        title: None,
                        url: None,
                        author: None,
                        content: None,
                        summary: None,
                        published: 0,
                    }]
                },
                next: None,
            })
        }
        .boxed_local()
    }
    fn entries_by_id(&self, _ids: Vec<String>) -> Call<'_, Vec<Entry>> {
        async { Ok(vec![]) }.boxed_local()
    }
    fn read_state(&self) -> Call<'_, ReadChanges> {
        async {
            Ok(ReadChanges::Delta(vec![ReadState {
                id: "entry:other".into(),
                read: true,
            }]))
        }
        .boxed_local()
    }
    fn apply(&self, operations: Vec<Operation>) -> Call<'_, Vec<Outcome>> {
        async move {
            Ok(operations
                .into_iter()
                .map(|o| Outcome::Applied {
                    revision: o.revision,
                    subscription: None,
                })
                .collect())
        }
        .boxed_local()
    }
}
#[test]
fn alternate_protocol_checkpoints_advance_only_after_complete_import() {
    let a = Replica::new();
    let p = DeltaProvider {
        restored: Default::default(),
        fail: std::cell::Cell::new(true),
    };
    assert!(matches!(wait(synchronize(&p, &a)), Err(Error::Network)));
    assert_eq!(wait(a.checkpoint()).unwrap(), None);
    wait(synchronize(&p, &a)).unwrap();
    assert!(a.read("entry:other"));
    assert_eq!(
        wait(a.checkpoint()).unwrap().as_deref(),
        Some("opaque/version:2")
    );
    wait(synchronize(&p, &a)).unwrap();
    assert_eq!(p.restored.borrow().as_deref(), Some("opaque/version:2"));
}

#[test]
fn committed_entry_validator_skips_bodies_but_new_subscriptions_invalidate_it() {
    let server = Server::new();
    let p = server.provider();
    let a = Replica::new();
    assert_eq!(wait(synchronize(&p, &a)).unwrap().entries, 2);
    assert_eq!(wait(synchronize(&p, &a)).unwrap().entries, 0);
    server.set_read(1001, true);
    assert_eq!(wait(synchronize(&p, &a)).unwrap().entries, 0);
    assert!(a.read("1001"));
    let f =
        a.db.add_feed("https://fixture.invalid/another", "Another", None);
    a.db.sync_adopt_local(f).unwrap();
    assert_eq!(wait(synchronize(&p, &a)).unwrap().entries, 2);
}

#[test]
fn cancelled_import_reports_progress_and_resumes_without_advancing_checkpoint() {
    use daynews_sync::{engine::synchronize_with_progress, progress::Phase};
    let server = Server::new();
    let p = server.provider();
    let a = Replica::new();
    let events = std::cell::RefCell::new(Vec::new());
    let result = wait(synchronize_with_progress(&p, &a, |progress| {
        let cancel = progress.phase == Phase::Articles && progress.completed == 1;
        events.borrow_mut().push(progress);
        if cancel {
            Err(Error::Cancelled)
        } else {
            Ok(())
        }
    }));
    assert!(matches!(result, Err(Error::Cancelled)));
    assert_eq!(wait(a.checkpoint()).unwrap(), None);
    assert_eq!(a.db.container.query::<Article>().live().ids().len(), 1);
    let events = events.into_inner();
    let downloaded = events.last().unwrap();
    assert_eq!(downloaded.total, Some(2));
    assert_eq!(downloaded.articles, 1);
    assert_eq!(downloaded.feed.as_deref(), Some("Sync fixture"));
    assert!(!events.iter().any(|p| p.phase == Phase::Complete));
    wait(synchronize(&p, &a)).unwrap();
    assert_eq!(a.db.container.query::<Article>().live().ids().len(), 2);
    assert!(wait(a.checkpoint()).unwrap().is_some());
}

#[test]
fn reset_discards_pending_uploads_and_reimports_remote_state_without_remote_deletions() {
    use day_reactive::Binding;
    use daynews_db::{
        ArticleBody, Feed, Folder, Tag,
        sync::{Intent, SubscriptionLink},
    };
    let server = Server::new();
    let p = server.provider();
    let a = Replica::new();
    wait(synchronize(&p, &a)).unwrap();
    a.set_read("1001", true); // deliberately never uploaded
    a.db.container
        .get::<Article>(a.article("1001"))
        .unwrap()
        .is_starred()
        .write(true);
    a.db.add_folder("Synthetic local folder");
    a.db.add_feed(
        "https://fixture.invalid/local-only",
        "Synthetic local feed",
        None,
    );
    a.db.container.insert(Tag {
        id: 123,
        name: "Synthetic local tag".into(),
        ..Default::default()
    });
    a.db.container.save().unwrap();
    let request_count = server.simulation.requests().len();
    a.saved(a.db.sync_reset_local(a.account)).unwrap();
    macro_rules! empty {
        ($model:ty) => {
            assert!(a.db.container.query::<$model>().live().ids().is_empty());
        };
    }
    empty!(Article);
    empty!(ArticleBody);
    empty!(Feed);
    empty!(Folder);
    empty!(Tag);
    empty!(Intent);
    empty!(SubscriptionLink);
    empty!(EntryLink);
    assert_eq!(wait(a.checkpoint()).unwrap(), None);
    assert_eq!(a.db.sync_accounts().unwrap().len(), 1);
    assert!(a.db.sync_accounts().unwrap()[0].connected);
    assert!(server.unread().contains(&1001));
    wait(synchronize(&p, &a)).unwrap();
    assert_eq!(a.db.container.query::<Article>().live().ids().len(), 2);
    assert!(!a.read("1001"));
    assert_eq!(a.db.sync_pending_count(a.account), 0);
    assert!(
        server.simulation.requests()[request_count..]
            .iter()
            .all(|r| r.method == "GET")
    );
}

#[test]
fn interrupted_incremental_scan_does_not_stop_at_a_previously_imported_first_page() {
    use day_reactive::Binding;
    use daynews_db::sync::AccountFields;
    let server = Server::new();
    let p = server.provider();
    let a = Replica::new();
    wait(synchronize(&p, &a)).unwrap();
    // Model an interrupted later scan: its first page is cached, but an older read
    // article has not been received. It cannot be rescued by the missing-unread pass.
    server.set_read(1002, true);
    a.db.container.delete::<Article>(a.article("1002")).unwrap();
    let account = a.db.container.get::<Account>(a.account).unwrap();
    account.importing().write(true);
    account.checkpoint().write(None);
    a.db.container.save().unwrap();
    wait(synchronize(&p, &a)).unwrap();
    assert_eq!(a.db.container.query::<Article>().live().ids().len(), 2);
    assert!(a.read("1002"));
    assert!(!account.importing().peek());
}
