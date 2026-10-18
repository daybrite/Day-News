//! Scheduling-independent sync cycle, shared by production and two-installation tests.
use crate::*;

pub trait Storage {
    fn checkpoint(&self) -> Call<'_, Option<String>>;
    /// Mark a durable import in progress; return true when resuming an interrupted scan.
    fn begin_entries(&self) -> Call<'_, bool>;
    fn pending(&self) -> Call<'_, Vec<Operation>>;
    fn outcomes(&self, outcomes: Vec<Outcome>) -> Call<'_, ()>;
    fn subscriptions(&self, changes: Changes<Subscription>) -> Call<'_, ()>;
    fn missing(&self, ids: Vec<String>) -> Call<'_, Vec<String>>;
    fn entries(&self, entries: Vec<Entry>, state: ReadChanges) -> Call<'_, bool>;
    fn reading(&self, state: ReadChanges) -> Call<'_, ()>;
    fn finished(&self, checkpoint: Option<String>) -> Call<'_, ()>;
}

#[derive(Default, Debug)]
pub struct Report {
    pub entries: usize,
    pub acknowledged: usize,
    pub pending_error: Option<Error>,
}

/// A failed status write does not prevent article downloads. Outstanding local intents
/// overlay remote state in Storage. Checkpoints advance only inside successful transactions.
pub async fn synchronize(provider: &dyn Provider, store: &dyn Storage) -> Result<Report> {
    synchronize_with_progress(provider, store, |_| Ok(())).await
}

/// Progress is emitted at request/transaction boundaries. A cancelled observer can stop
/// before the next request; callers may additionally drop this future to abort active I/O.
pub async fn synchronize_with_progress(
    provider: &dyn Provider,
    store: &dyn Storage,
    observe: impl Fn(progress::Progress) -> Result<()>,
) -> Result<Report> {
    use progress::{Phase, Progress};
    let mut progress = Progress::default();
    observe(progress.clone())?;
    let capabilities = provider.capabilities();
    if !(capabilities.subscriptions && capabilities.read_state && capabilities.entries) {
        return Err(Error::Unsupported);
    }
    provider.restore_checkpoint(store.checkpoint().await?)?;
    let mut report = Report::default();
    let operations = store.pending().await?;
    progress.phase = Phase::Uploading;
    progress.total = Some(operations.len());
    for chunk in operations.chunks(1000) {
        observe(progress.clone())?;
        match provider.apply(chunk.to_vec()).await {
            Ok(outcomes) => {
                for result in &outcomes {
                    match result {
                        Outcome::Applied { .. } => report.acknowledged += 1,
                        Outcome::Failed { error, .. } => report.pending_error = Some(error.clone()),
                    }
                }
                store.outcomes(outcomes).await?;
            }
            Err(Error::Authentication) => return Err(Error::Authentication),
            Err(e) => report.pending_error = Some(e),
        }
        progress.completed += chunk.len();
        observe(progress.clone())?;
    }
    progress = Progress {
        phase: Phase::Subscriptions,
        ..Default::default()
    };
    observe(progress.clone())?;
    let subscriptions = provider.subscriptions().await?;
    let titles: std::collections::HashMap<_, _> = match &subscriptions {
        Changes::Snapshot(rows) | Changes::Delta { upserts: rows, .. } => rows
            .iter()
            .map(|s| (s.feed_id.clone(), s.title.clone()))
            .collect(),
        Changes::Unchanged => Default::default(),
    };
    progress.subscriptions = titles.len();
    store.subscriptions(subscriptions).await?;
    observe(progress.clone())?;
    progress.phase = Phase::ReadState;
    observe(progress.clone())?;
    let state = provider.read_state().await?;
    // Reading state has priority over history downloads.
    store.reading(state.clone()).await?;
    let resuming = store.begin_entries().await?;
    let mut cursor = None;
    let mut visited = std::collections::HashSet::new();
    progress.phase = Phase::Articles;
    observe(progress.clone())?;
    loop {
        let page = provider.entries(cursor).await?;
        progress.total = page.total_pages.or(progress.total);
        progress.feed = page
            .values
            .first()
            .and_then(|e| titles.get(&e.feed_id))
            .cloned();
        report.entries += page.values.len();
        let known = store.entries(page.values, state.clone()).await? && !resuming;
        progress.completed += 1;
        progress.articles = report.entries;
        cursor = page.next;
        if known || cursor.is_none() {
            progress.total = Some(progress.completed);
        }
        observe(progress.clone())?;
        if known || cursor.is_none() {
            break;
        }
        if !visited.insert(cursor.clone()) {
            return Err(Error::InvalidResponse);
        }
    }
    if let ReadChanges::UnreadSnapshot(ids) = &state {
        let missing = store.missing(ids.clone()).await?;
        progress.phase = Phase::MissingArticles;
        progress.completed = 0;
        progress.total = Some(missing.len());
        progress.feed = None;
        for chunk in missing.chunks(100) {
            observe(progress.clone())?;
            let entries = provider.entries_by_id(chunk.to_vec()).await?;
            report.entries += entries.len();
            store.entries(entries, state.clone()).await?;
            progress.completed += chunk.len();
            progress.articles = report.entries;
            observe(progress.clone())?;
        }
    }
    progress.phase = Phase::Finalizing;
    observe(progress.clone())?;
    // An article can be read on another device while its body is downloading.
    store.reading(provider.read_state().await?).await?;
    if report.pending_error.is_none() {
        store.finished(provider.checkpoint()).await?;
        progress.phase = Phase::Complete;
        observe(progress)?;
    }
    Ok(report)
}
