# Day News — design

A feed reader on the [Day](https://daybrite.dev) framework, modeled on
[NetNewsWire](https://github.com/Ranchero-Software/NetNewsWire): subscriptions on the left, a
timeline in the middle, the article on the right — collapsing to push navigation on a phone.

Targets: `macos-appkit`, `macos-gtk`, `macos-qt`, `windows-xaml`, `ios-uikit`, `android-mdc`,
`web-dom`, `harmony-arkui`.

> `web-dom` builds and runs the whole shell, but a browser may only fetch feeds that send CORS
> headers, and most publishers do not — so the web build reads what it is allowed to reach
> rather than any URL you paste. Its article pane is also blank until the reader can hand the
> web view HTML directly (see *Reader*). `harmony-arkui` joined when `day-piece-webview` grew
> its ArkUI renderer — the app builds, installs, and runs in the collapsed phone layout on the
> Oniro emulator. Whether ArkWeb serves the reader's `file://` document is not yet verified;
> the walkthrough's `web_eval` check answers that on the CI emulator leg.

## Crates

Split so that everything except the last is testable without a UI or a network.

The libraries live under `crates/`; the UI crate is the repository root.

| crate | owns | depends on |
|---|---|---|
| `crates/daynews-opml` | OPML parse + serialize, nested folders | quick-xml |
| `crates/daynews-feed` | fetching and parsing RSS/RDF/Atom/JSON Feed, normalized | feed-rs, day-part-http, day-part-timezone |
| `crates/daynews-db` | the store: models, relations, FTS5, queries | day-persistence, day-model, day-macros, day-part-timezone |
| `crates/daynews-core` | the view-model: signals, refresh orchestration, OPML import/export | the three above, day-core |
| `day-news` | the UI | `daynews-core`, day, day-piece-webview |

### Dependency choices

Three non-obvious ones, since the house rule is to justify every dependency:

- **feed-rs.** Syndication is four formats plus a long tail of namespaced extensions and at least
  three date encodings, several written wrong by popular publishers. The feeds this was built
  against span YouTube (Atom), Reddit, Mastodon, Discourse, Blogspot, Medium and GitHub, which
  disagree about nearly everything. Policy (which field wins, how ids are derived) stays ours
  in `normalize`.
- **quick-xml.** OPML is XML; hand-rolling means hand-rolling entity decoding and attribute
  quoting. It reads *and* writes, so one dependency covers import and export.
- **day-part-timezone, for the clock.** "Today" is a claim about the reader's calendar, so the
  cut-off is local midnight — which needs the machine's UTC offset and its DST rules. It also
  supplies the one wall clock that works everywhere the app runs: `SystemTime::now()` aborts on
  `wasm32`, where the page's own clock answers instead.

SQLite itself is no longer a direct dependency: day-persistence bundles the engine (compiled
from C source, which is what lets Android link — the NDK sysroot ships no `-lsqlite3`) and
owns the FTS5 build.

## The store

One SQLite file, opened lazily through day-persistence's engine
([docs/persistence.md](https://daybrite.dev/docs/persistence)). The schema is *declared*, not
migrated by hand: `#[derive(Model)]` on each type in `crates/daynews-db/src/lib.rs` states the
tables, relations and their delete rules, and the engine applies the schema and its migrations
itself — no directory of `.sql` files staged beside a read-only app bundle.

Search is FTS5 over generated shadow tables — an `fts(...)` attribute on the model declares
which columns are indexed, and the engine keeps them in step, so nothing hand-writes triggers.
Titles, authors and summaries are indexed beside the bodies, which live in their own row so a
timeline window faults metadata only. User text is quoted per token with a prefix `*` on the
last one, which makes search feel live and means punctuation is searched for rather than parsed
as query syntax.

The property everything else rests on: **an article already present is left completely alone on
refresh.** That is what preserves read state, and it is why `daynews-feed` works so hard to derive
a stable id (feed id → article URL → hash of title+date).

## Threading

There is none. Day's reactive core is `!Send` and its executor (`day::task`) polls futures on the
UI thread, so the store lives in a `RefCell` and every mutation happens between awaits — no
marshaling, no locks. Only network I/O is off-thread, and the HTTP part hands the response back on
the main thread.

Refresh is sequential on purpose: each `await` returns to the main loop, so rows appear
progressively and importing a large subscription list does not open one socket per feed at once.

> [!NOTE]
> **Known limitation.** Parsing and the database writes also run on the UI thread. On a slow
> device a large refresh can block it long enough to be noticeable (on the Android emulator it
> exceeded dayscript's 10-second main-thread timeout twice during a 5-feed seed). The fix is to
> move parse + insert off-thread and marshal results back with `Setter`, or to yield between
> articles. Deferred, not forgotten.

## Reader

The article pane is a native web view pointed at a `file://` document we generate per article.
A `data:` URL would avoid the temp file, but Android's WebView refuses top-level `data:`
navigations (API 30+) and every platform caps their length. Android needed one more thing:
API 30 also turned `WebSettings.setAllowFileAccess` off, refusing even the app's own file, so
day-piece-webview re-enables it for a web view the app itself pointed at a `file://` URL —
the switches that would let a page read OTHER files stay off. The web build has no filesystem
to write to at all, which is why its reader is blank until the piece grows a way to hand the
view HTML directly. The document is self-contained — no
external CSS or fonts — so it renders identically offline and leaks no reading activity to third
parties. Feed HTML is sanitized at the parse boundary rather than trusting the renderer.

## Looking like a reader

The layout was measured against NetNewsWire's own macOS window rather than from memory. What
that comparison changed:

- **Row order.** Title, then summary, then a feed·date footer — headline first, provenance last.
  The feed name had been on top, which buried the one line a reader actually scans.
- **Type.** Day's semantic steps only (`Body` / `Footnote` / `Caption`), never a hardcoded point
  size: those steps follow the reader's accessibility text-size setting, and a timeline that
  ignores it is unusable for the people who change it.
- **Read state.** A read article dims its title rather than only dropping its dot. Selection is
  the platform's own: the timeline is a native `list`, so the table draws the highlight and owns
  the arrow keys, and rows keep their content colors instead of inverting by hand.
- **Separators** between rows, drawn by the HOST at the row boundary (`.separators(true)`). At
  this density adjacent titles and summaries otherwise merge into one undifferentiated column of
  text — but a hairline drawn *inside* the row is the wrong tool: it never lines up with the
  native selection, and it sits still while a swipe slides the row past it.
- **Swipe actions** on both edges where the platform has them (macOS row actions, iOS swipe
  actions): trailing toggles read with a filled/outlined circle, leading stars. The offer is
  pulled at gesture time, so the button names the flip it is about to make.
- **A heading** over the list — the scope's name and its unread count — so the pane says where
  you are instead of opening with a bare row of controls.
- **Sidebar glyphs**, drawn as vectors in `resource/images/sidebar_*.png` (template PNGs, tinted
  by the app per row — a warm sun, a blue unread dot, a gold star — so the smart feeds read
  apart at a glance). Not SF Symbols: that license does not cover redistributing them onto
  Android, GTK and Qt.
- **Window size.** 1440x900, near NetNewsWire's own. Three panes at 960 leave the timeline and
  the article both too narrow to read. Note that `[window]` in `Day.toml` is inert — only
  `day metadata` reads it; the size that takes effect is the one in `day::launch`.

## Sidebar and menus

The sidebar opens on four smart feeds — Today, All Unread, Starred, All Articles — above one row
per subscription and one per tag. Unread counts are real badges (`.badge(…)`), right-aligned and
de-emphasized by each toolkit, and the three blocks sit under their own `.section(…)` headers;
both were gaps in Day's `selector` when this app started and were built in the framework rather
than faked in the row label.

`watch` fires on CHANGE, so the shell applies the opening scope itself (`OPENING_SECTION`).
Without that the sidebar highlighted Today while the timeline still showed the scope the store
opened with — invisible on a store whose newest unread articles are all from today, and obvious
on a stale one.

Desktop gets a File menu (New Feed, New Folder, New Window, Refresh, Import/Export Subscriptions,
Close Window) and a Go menu (Next Unread ⌘/, then the three smart feeds). These are one
`app_menu_reactive` model, so all four desktop toolkits get the same bar from the same code and
`dayscript/menus.yaml` drives them by Fluent key on every one. Next Unread walks the visible
timeline forward and wraps, then falls back to any unread article, so it keeps working when the
current scope is exhausted.

## What real feeds taught us

Each of these is a test, because each was a wrong assumption first:

- **Subscriptions can have no title.** A subscription that has never been fetched records no
  name at all, so readers show the host until the first refresh supplies one. The vendored OPML
  fixtures include that state, and `daynews-opml`'s tests assert the fallback.
- **Microblog items have no `<title>` at all.** Mastodon posts carry only a body, so a title is
  derived from the content.
- **Feed text is escaped twice.** A description holding escaped HTML has its own entities escaped
  again, so `&` arrives as `&amp;amp;`. The second decode is applied only when tags were actually
  found, so prose that merely mentions `&amp;` is left alone.
- **WordPress lists the feed itself first**, so "open website" needs a link that is not the feed's
  own URL.

## Not yet built

Cloud sync (iCloud/Feedbin/Reader), reader view / article extraction, per-feed refresh intervals,
starred-article sync, images cached offline, folder editing in the UI (import creates folders,
but there is no rename/move), and article pagination beyond the 500-row timeline cap.

Two want Day itself to grow first: an HTML-content API on `day-piece-webview`, without which
the web build's article pane stays blank (it has no filesystem for the generated document), and
self-sizing list rows — `RowHeight::Automatic` is a fixed default on every backend today, which
is why the timeline pins a uniform pitch and why a wrapped title can clip its footer on Android.
