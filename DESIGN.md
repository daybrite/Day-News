# Day News — design

A feed reader on the [Day](https://daybrite.dev) framework, modeled on
[NetNewsWire](https://github.com/Ranchero-Software/NetNewsWire): subscriptions on the left, a
timeline in the middle, the article on the right — collapsing to push navigation on a phone.

Targets: `macos-appkit`, `macos-gtk`, `macos-qt`, `windows-xaml`, `ios-uikit`, `android-mdc`.

> Not `web-dom`: a browser cannot fetch arbitrary feeds (CORS), so the app would have nothing to
> read. Not `harmony-arkui` either — `day-piece-webview` ships no ArkUI renderer, so the article
> pane would be a placeholder. Both are additive later: ArkUI needs only a webview renderer.

## Crates

Split so that everything except the last is testable without a UI or a network.

| crate | owns | depends on |
|---|---|---|
| `daynews-opml` | OPML parse + serialize, nested folders | quick-xml |
| `daynews-feed` | fetching and parsing RSS/RDF/Atom/JSON Feed, normalized | feed-rs, day-part-http |
| `daynews-db` | the SQLite store: schema, migrations, FTS5, queries | diesel, libsqlite3-sys |
| `daynews-core` | the view-model: signals, refresh orchestration, OPML import/export | the three above, day-core |
| `day-news` | the UI | `daynews-core`, day, day-piece-webview |

### Dependency choices

Three non-obvious ones, since the house rule is to justify every dependency:

- **feed-rs.** Syndication is four formats plus a long tail of namespaced extensions and at least
  three date encodings, several written wrong by popular publishers. The 145-feed sample spans
  YouTube (Atom), Reddit, Mastodon, Discourse, Blogspot, Medium and GitHub, which disagree about
  nearly everything. Policy (which field wins, how ids are derived) stays ours in `normalize`.
- **quick-xml.** OPML is XML; hand-rolling means hand-rolling entity decoding and attribute
  quoting. It reads *and* writes, so one dependency covers import and export.
- **libsqlite3-sys with `bundled`, as a DIRECT dependency.** It compiles SQLite from C source,
  which is what lets Android link — the NDK sysroot ships no `-lsqlite3` (Apple SDKs do, so iOS
  would link either way). It also pins one SQLite version and feature set across every target.
  FTS5 availability was verified by test rather than assumed.

## The store

One SQLite file, opened once on the UI thread. Schema in `daynews-db/src/migrations.rs`, applied in
order and tracked with SQLite's own `user_version` — deliberately not `diesel_migrations`, which
wants a directory of files staged beside the binary, awkward where the app bundle is read-only.

Search is an FTS5 **external-content** table over `articles`, kept in step by three triggers, so
the index stores no second copy of every article body. User text is quoted per token with a
prefix `*` on the last one, which makes search feel live and means punctuation is searched for
rather than parsed as query syntax.

The property everything else rests on: **an article already present is left completely alone on
refresh.** That is what preserves read state, and it is why `daynews-feed` works so hard to derive
a stable id (feed id → article URL → hash of title+date).

## Threading

There is none. Day's reactive core is `!Send` and its executor (`day::task`) polls futures on the
UI thread, so the store lives in a `RefCell` and every mutation happens between awaits — no
marshaling, no locks. Only network I/O is off-thread, and the HTTP part hands the response back on
the main thread.

Refresh is sequential on purpose: each `await` returns to the main loop, so rows appear
progressively and a 145-feed import does not open 145 sockets at once.

> [!NOTE]
> **Known limitation.** Parsing and the database writes also run on the UI thread. On a slow
> device a large refresh can block it long enough to be noticeable (on the Android emulator it
> exceeded dayscript's 10-second main-thread timeout twice during a 5-feed seed). The fix is to
> move parse + insert off-thread and marshal results back with `Setter`, or to yield between
> articles. Deferred, not forgotten.

## Reader

The article pane is a native web view pointed at a `file://` document we generate per article.
A `data:` URL would avoid the temp file, but Android's WebView refuses top-level `data:`
navigations (API 30+) and every platform caps their length. The document is self-contained — no
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
- **Read state.** A read article dims its title rather than only dropping its dot; the selected
  row inverts wholesale onto the accent fill, secondary lines included.
- **Hairlines** between rows. At this density adjacent titles and summaries otherwise merge into
  one undifferentiated column of text.
- **A heading** over the list — the scope's name and its unread count — so the pane says where
  you are instead of opening with a bare row of controls.
- **Sidebar glyphs**, drawn as vectors in `resource/images/sidebar_*.png` (template PNGs, tinted
  per theme by each backend). Not SF Symbols: that license does not cover redistributing them
  onto Android, GTK and Qt.
- **Window size.** 1440x900, near NetNewsWire's own. Three panes at 960 leave the timeline and
  the article both too narrow to read. Note that `[window]` in `Day.toml` is inert — only
  `day metadata` reads it; the size that takes effect is the one in `day::launch`.

## Sidebar and menus

The sidebar opens on four smart feeds — Today, All Unread, Starred, All Articles — above one row
per subscription. Unread counts ride in the row's own label (`Today (25)`) because Day's sidebar
item has no badge slot; a real badge is a `NavMenuProps` field plus a renderer in each backend,
and is listed under *Not yet built*. There is likewise no group HEADER over the smart feeds: Day's
`selector` has no non-selectable group row, which on AppKit alone means an `NSOutlineView` group
item and has no counterpart in a GTK `ListBox` or a Qt `QListWidget` without one being designed.

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

- **Subscriptions can have no title.** Three in the sample have never been fetched, so their name
  is empty; readers show the host until the first refresh supplies one.
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

Two of these want Day itself to grow first: a real unread BADGE on a sidebar row, and a
non-selectable group header so "Smart Feeds" can label the block it belongs to.
