//! Provider-independent synchronization contracts. IDs and cursors belong to the provider;
//! article caches, account state and durable user intent belong to the application.
use futures_util::future::LocalBoxFuture;
use serde::{Deserialize, Serialize};

pub mod engine;
pub mod feedbin;
pub mod fixture;
pub mod progress;

pub type Result<T> = std::result::Result<T, Error>;
pub type Call<'a, T> = LocalBoxFuture<'a, Result<T>>;

/// Protocol details are deliberately absent from errors exposed to the UI. Never carry a
/// response body, URL, Authorization header, or password into a diagnostic.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    Authentication,
    Network,
    Retry { after_seconds: u64 },
    Rejected,
    InvalidResponse,
    Unsupported,
    Storage,
    Cancelled,
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "sync: {self:?}")
    }
}
impl std::error::Error for Error {}

/// No Debug or Serialize: secrets must never enter logs, snapshots or operation journals.
#[derive(Clone)]
pub struct Credentials {
    pub username: String,
    pub password: String,
}

#[derive(Clone, Copy, Debug)]
pub struct Capabilities {
    pub subscriptions: bool,
    pub read_state: bool,
    pub entries: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Subscription {
    pub id: String,
    pub feed_id: String,
    pub url: String,
    pub title: String,
    pub site_url: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Entry {
    pub id: String,
    pub feed_id: String,
    pub title: Option<String>,
    pub url: Option<String>,
    pub author: Option<String>,
    pub content: Option<String>,
    pub summary: Option<String>,
    pub published: i64,
}

/// Only a fully downloaded snapshot can establish absence. A delta must name removals.
#[derive(Clone, Debug)]
pub enum Changes<T> {
    Snapshot(Vec<T>),
    Delta {
        upserts: Vec<T>,
        removed: Vec<String>,
    },
    Unchanged,
}

#[derive(Clone, Debug)]
pub struct Page<T> {
    pub values: Vec<T>,
    pub next: Option<String>,
    /// Optional total page count supplied by the service.
    pub total_pages: Option<usize>,
}

#[derive(Clone, Debug)]
pub enum ReadChanges {
    /// All currently unread accessible entries. Other KNOWN accessible entries are read.
    UnreadSnapshot(Vec<String>),
    Delta(Vec<ReadState>),
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReadState {
    pub id: String,
    pub read: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum Mutation {
    SetRead(ReadState),
    Subscribe { url: String },
    Unsubscribe { subscription: String },
    Rename { subscription: String, title: String },
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Operation {
    pub revision: u64,
    pub mutation: Mutation,
}

#[derive(Clone, Debug)]
pub enum Outcome {
    Applied {
        revision: u64,
        subscription: Option<Subscription>,
    },
    Failed {
        revision: u64,
        error: Error,
    },
}

/// Object-safe and executor-independent. Another service implements this interface without
/// changing application commands, journals, database schemas, or reconciliation policy.
pub trait Provider {
    fn kind(&self) -> &'static str;
    fn capabilities(&self) -> Capabilities;
    /// Restore the last fully committed opaque delta token before each cycle. A failed cycle
    /// may have advanced in-memory state, but must restart from the durable checkpoint.
    fn restore_checkpoint(&self, checkpoint: Option<String>) -> Result<()> {
        if checkpoint.is_some() {
            Err(Error::Unsupported)
        } else {
            Ok(())
        }
    }
    /// A provider may return a token only after all pages contributing to it were delivered.
    fn checkpoint(&self) -> Option<String> {
        None
    }
    fn authenticate(&self) -> Call<'_, ()>;
    fn subscriptions(&self) -> Call<'_, Changes<Subscription>>;
    fn entries(&self, cursor: Option<String>) -> Call<'_, Page<Entry>>;
    fn entries_by_id(&self, ids: Vec<String>) -> Call<'_, Vec<Entry>>;
    fn read_state(&self) -> Call<'_, ReadChanges>;
    fn apply(&self, operations: Vec<Operation>) -> Call<'_, Vec<Outcome>>;
}
