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
| `crates/daynews-time` | the wall clock and the reader's UTC offset, asked of the host | libc (unix), day-dom (web) |
| `crates/daynews-feed` | fetching and parsing RSS/RDF/Atom/JSON Feed, normalized | feed-rs, day-part-http, `daynews-time` |
| `crates/daynews-db` | the store: models, relations, FTS5, queries | day-persistence, day-model, day-macros, `daynews-time` |
| `crates/daynews-core` | the view-model: signals, refresh orchestration, OPML import/export | `daynews-db`, `daynews-feed`, `daynews-opml`, day-core |
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
- **No time-zone database at all**, which is why `crates/daynews-time` exists. "Today" is a claim
  about the reader's calendar, so the cut-off is local midnight — which needs the machine's UTC
  offset and its DST rules — and the app also needs a wall clock everywhere it runs, since
  `SystemTime::now()` aborts on `wasm32`. Both come from the host, which already has the rules
  installed and keeps them current: POSIX `localtime_r` (macOS, Linux, iOS, Android, OpenHarmony),
  Win32's `SystemTimeToTzSpecificLocalTime`, and the browser's `Intl` behind day-dom's `tzoffset`
  page fact on web. That is ~150 lines and one `libc` edge, against ~200 KB of bundled IANA data
  that would need re-releasing every time a government moves a clock.

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

Native builds own the database through `day-persistence::DatabaseWorker`. Open/migrate,
article/body materialization, FTS, retention, OPML import, read/star edits, undo and feed commits
run on that worker. The UI holds owned sidebar/timeline/reader values in signals. It does not
share the worker's reactive model handles. The web build retains the compatible synchronous
OPFS implementation in `daynews-core::legacy` until an async transferable protocol exists.

Four network exchanges can run concurrently. Parsing HTTP responses already runs off the UI
thread; parsed items move into one atomic worker transaction with metadata and HTTP validators.
The import checks subscription existence at commit time so a late response cannot resurrect an
unsubscribed feed. Existing articles retain their read status and bodies. Imports and retention
are excluded from user undo. An existence-query failure aborts import rather than overwriting
stored read state. Status translations are formatted on the UI thread.

Each window owns cancellable subscriptions for its scope/search and selected article. Retraction
of an effect aborts the old receiver before a new one can publish; completions capture the
originating scene. Reader content stays visible while the next selected article loads, then
the worker snapshot replaces it without an intermediate empty state. Explicitly closing the
reader or changing scope still clears it. Next Unread waits for its cross-scope candidate
before changing the scene. Sidebar filtering uses `Nav::retain_selection_when` so hiding the
last-read feed preserves its current document and toolbar. Sidebar unread
counts use one indexed grouped SQL query, replacing one query per feed. The `(feed,is_read)`
index supports these counts. Projection equality and latest-value delivery reduce unnecessary UI
work; SQL tracing is installed only when trace logging is actually enabled or
`DAY_NEWS_LOG_SQL` is set. The latter writes engine-traced, parameter-expanded statements to
stderr in both debug and release builds, independently of `DAY_LOG`, including worker queries,
transactions, and migrations. Unset it for normal operation without trace formatting overhead.

Find Articles (Cmd-F) activates the native search field at the right of the desktop toolbar.
Its edits drive the same FTS query as programmatic search. Title and body indexes are queried
on the worker. Row scrolling follows a changed selected article ID; inserting newer rows above
the selection does not repeatedly jump the scrolling list.

The worker drains accepted edits at termination. Errors are delivered to the UI instead of
being reported as success. Unit tests cover search punctuation/title/body matches, concurrent
refresh/read edits, per-search subscriptions, sticky unread rows, sidebar counts and deletion
of the currently displayed article. Framework tests cover transaction/cancellation/close races.
The synchronous database API is still used by the headless store tests and the web tier.

## Reader

The article pane is a native web view pointed at a `file://` document we generate per reader pane.
Each pane reuses and cleans up its own temporary file, so windows showing the same article in
different reading modes cannot overwrite one another's content.
A `data:` URL would avoid the temp file, but Android's WebView refuses top-level `data:`
navigations (API 30+) and every platform caps their length. Android needed one more thing:
API 30 also turned `WebSettings.setAllowFileAccess` off, refusing even the app's own file, so
day-piece-webview re-enables it for a web view the app itself pointed at a `file://` URL —
the switches that would let a page read OTHER files stay off. The web build has no filesystem
to write to at all, which is why its reader is blank until the piece grows a way to hand the
view HTML directly. The document is self-contained — no
external CSS or fonts — so it renders identically offline and leaks no reading activity to third
parties. Feed HTML is sanitized at the parse boundary rather than trusting the renderer.

### Reader View

`ArticleExtractor` (`src/extraction.rs`) returns an owned `ExtractedArticle` from an article URL.
It is independent of selection, commands, database storage, and presentation; an authorized
HTTP service or an offscreen engine can replace `LocalReadability`. The current provider uses
Day's native HTTP stack with the DayNews user agent, validates status/content type, decodes the
response charset, and resolves links against the final redirect URL. It rejects documents above
5 MiB before parsing and limits Readability to 50,000 elements. This is a parse limit, not a
streaming download limit. The command has a 30-second deadline.

Bundled Readability 0.6.0 and DOMPurify 3.4.16 run in the existing reader's JS engine on a
detached document; the publisher page is never navigated to or executed. HTML is sanitized both
before extraction and before returning it to the reader. All bundled scripts use generated
asset accessors. JS-only and authenticated pages are not supported, and web HTTP requests
remain subject to CORS. A backend without JS evaluation disables the command.

`ReaderView` is ambient per-window state. The command toggles a temporary full-text overlay;
it never writes over stored RSS content. Repeating it while loading cancels, while repeating it
after completion switches between the two versions without refetching. Changing article ID or
URL cancels the task and clears the overlay. Window disposal cancels pending work. Errors leave
the RSS version visible, with localized retry guidance. Reader styles and external-link handling
are shared by both modes. Command-Shift-R belongs to Reader View; Refresh Feed no longer claims
that shortcut. A toolbar command exposes the same action on mobile.

NetNewsWire uses signed Feedbin extraction requests with a client ID and secret; no unauthenticated
or User-Agent-whitelist integration is provided. Feedbin access requires credentials authorized
for this app. Provider replacement requires no change to the reader state or rendering.

`tests/reader-extraction.cjs` exercises the bundled scripts with synthetic publication HTML.
`tests/reader-view-server.py` plus `dayscript/reader-view.yaml` cover native success, cached
toggle, cancellation on selection, and HTTP failure using an isolated database.

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
both were gaps in Day's `nav` when this app started and were built in the framework rather
than faked in the row label.

`watch` fires on CHANGE, so the shell applies the opening scope itself (`OPENING_SECTION`).
Without that the sidebar highlighted Today while the timeline still showed the scope the store
opened with — invisible on a store whose newest unread articles are all from today, and obvious
on a stale one.

Desktop gets a File menu (New Feed, New Folder, New Window, Refresh, Import/Export Subscriptions,
Close Window), a Go menu (Previous/Next Article ⌘[/⌘], Next Unread ⌘/, then the three smart feeds),
a Feed menu (Refresh Feed, Mark All as Read, Unsubscribe) and an Article menu. These are one `app_menu_reactive` model,
so all four desktop toolkits get the same bar from the same code, and dayscript drives them by
Fluent key on every one. Next Unread walks the visible timeline forward and wraps, then falls back
to any unread article, so it keeps working when the current scope is exhausted.

The Feed menu acts on the feed the sidebar has selected, and does nothing when a smart feed, a tag
or a page is selected, the same rule the Article menu follows with no open article. Each feed row
also carries a context menu with the same three commands (`item(…).context_menu(…)`), aimed at the
row that was right-clicked or long-pressed. Both call the same daynews-core functions, so the
walkthrough's Feed-menu steps cover the row menu's actions too; `menu:` steps reach only the app
menu, so the row menu itself is checked by hand.

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
- **The whole article can live in `description`.** RSS 2.0 allows HTML there, and many feeds send
  no `content:encoded`, so an HTML description with nothing richer beside it becomes the article
  body; flattened to text, the reader ran every paragraph together. A plain-text description stays
  a summary.
- **`&#149;` means a bullet.** Numeric references 128–159 name C1 control characters in
  Unicode, but publishers write them for the Windows-1252 characters at those byte positions
  (Merriam-Webster separates its pronunciations with `&#149;`), and browsers read them that way.
  The XML layer resolves the reference before the parser sees it, so `daynews-feed` remaps the
  raw controls in titles, summaries and bodies; a control left alone draws as a box on Android.

## Not yet built

Cloud sync (iCloud/Feedbin/Reader), per-feed refresh intervals,
starred-article sync, images cached offline, folder editing in the UI (import creates folders,
but there is no rename/move), and article pagination beyond the 500-row timeline cap.

Two want Day itself to grow first: an HTML-content API on `day-piece-webview`, without which
the web build's article pane stays blank (it has no filesystem for the generated document), and
self-sizing list rows — `RowHeight::Automatic` is a fixed default on every backend today, which
is why the timeline pins a uniform pitch and why a wrapped title can clip its footer on Android.

### Feed refresh and sidebar identity

`daynews-feed::fetch_cached_with_progress` returns either a parsed feed with replacement HTTP
validators and a SHA-256 fingerprint, or `NotModified`. A 304 merges supplied validators with
stored ones; a byte-identical 200 skips parsing, sanitization and article existence queries,
but still replaces validators (and clears ones absent from that 200). This avoids stale
validators when a publisher changes ETag/Last-Modified without changing feed bytes. `daynews-core` stores those validators on
`Feed` alongside the articles, using nullable columns for existing libraries. 304 updates the
last-check timestamp and clears errors without touching bodies/read/star state. Failed HTTP
or parsing attempts never advance validators or fingerprints. The fingerprint is persisted
atomically with successful article imports, and nullable fields migrate existing libraries.
Equal metadata values do not emit redundant model writes. Empty imports skip their existence
query; duplicate GUIDs within one response are inserted once. HTTP 429/503 with a valid
Retry-After (seconds or HTTP date) persists a deadline; automatic and manual feed checks skip
that origin until the deadline expires, including across restarts. A successful check clears
it. This is per subscription, not a host-wide circuit breaker. The shared HTTP client reuses connections;
native parsing and sanitization use worker threads. Four shared permits bound all feed
requests, while an in-flight ID set prevents overlapping subscription/manual refreshes.
Deleted subscriptions are checked before requests and before storing results. RAII clears
activity and releases permits on completion or cancellation. `SheetsState::updating_feeds`
is an active-ID map whose optional fraction drives `Nav::icon_progress`, separately from
row labels, badges and icons. The icon overlay spins while connecting or when the body length
is unknown/encoded, then fills a circular ring for a trustworthy decoded content length.
Progress updates are throttled to 100 ms (mode changes and completion are immediate).
Finishing, failure, removal and cancellation clear the overlay without changing row geometry.
AppKit/UIKit animate only native Core Animation layers; download ticks never rebuild rows.
Other toolkits currently omit this optional per-icon decoration; aggregate refresh remains
visible on every platform.

`feed_icons` owns discovery, the local thumbnail cache and its separate four-worker queue.
All cached thumbnails are loaded before queued network discovery. Each source consists of
feed/home/icon URLs; source changes invalidate queued results. Batched publications avoid a
sidebar rebuild for every individual image. Untrusted responses and decoded image dimensions
are bounded. Successful thumbnails remain visible after transient rediscovery errors.
AppKit/UIKit display absolute-file icons in original color; generated bundled RSS symbols
remain the fallback. `feed_list` owns the persistent unread-only signal used by both the
menu and toolbar. Filtering changes visible subscription rows, not the scene's article scope.


## Worker migration validation (2026-10-01)

The migration was checked against isolated test libraries, not the user's active database.
Day-News's AppKit walkthrough passed 127 executed steps (2 platform skips); UIKit passed 97
(32 skips). Native AppKit Cmd-F and actual keyboard typing were verified separately, since
synthetic toolbar events alone do not exercise NSSearchField's action delivery.

Day-News builds passed for AppKit, UIKit, Android, GTK, Qt, web and HarmonyOS. Harmony was
compile/package checked only; no emulator was run. Its local build needed NODE_PATH pointing
to the installed Hvigor modules and a temporary versioned SDK root because the machine's SDK
symlink was stale. Windows/XAML was not built on this macOS host.

Framework validation includes 38 worker tests, plus existing model, persistence, core and spec
suites. The worker suite also passed ten consecutive runs to vary thread scheduling. Day-News's
77 headless tests include six native worker/projection regressions. The Showcase query script
passed 48 steps; Sketch's editor script passed 640 executed steps (15 skips). Stanza-Redux's
27 tests, Sketch's 109 tests and day-lite's 13 tests passed. Compatibility checks also passed
for App-Fair, Games-Fair, Day-Tunes, Day-Bench, Day-Rise, Day-Skies and Day-Trader. App-Fair needed
small updates from the retired selector/stack builders to nav/nav_stack and an explicit image
conversion. These checks do not imply that the apps retaining synchronous containers have
migrated every query to a worker.

### Reading navigation and link actions

Previous/Next Article resolve against the current scene's lightweight timeline snapshot,
including search filters, and use the selected ID rather than the asynchronously loaded body.
Rapid commands therefore advance from the latest selection. They never wrap or cross scopes;
missing selections disable movement, while no selection starts at the first/last visible row.
The existing selection binding scrolls and highlights the native row. Both commands live in
Go and in the reader toolbar; Next Unread has a distinct double-chevron image.

Copy Article Link uses the displayed article URL and Day's asynchronous clipboard API, started
inside the user action for browser permissions. It is available in Article and the secondary
reader toolbar. Article context menus are built when summoned, show state-appropriate actions,
and snapshot the clicked row's ID and URL so recycled cells cannot retarget an open menu.
Clipboard failures use a localized alert. Missing/blank URLs disable link commands.

Refresh cache regressions live in `crates/daynews-feed/tests/conditional.rs` (real loopback
requests with validator rotation on identical 200 and 304), the feed crate's response tests,
and `crates/daynews-db/tests/store.rs` (durability and unchanged-field writes). Run
`dayscript/conditional-cache.yaml` against `tests/feed-refresh-server.py` with an isolated
`DAY_NEWS_DATA_DIR`; `/stats` verifies request counts and the busy-origin pause.

Conditional requests save response bodies, not the request itself. Servers without validators
still transfer bodies, although equal fingerprints avoid repeat imports. Last-check metadata
is still durably recorded and native worker observations may recompute unchanged projections.
The current policy does not yet skip automatic polls based on HTTP freshness or adapt the
interval to a feed's publishing frequency. Web cross-origin validators require the publisher
(or proxy) to expose ETag/Last-Modified through CORS and allow conditional request headers.

### Adaptive refresh and feed groups

Refresh Feeds now defaults to Automatic; existing explicitly chosen intervals remain intact.
A minute tick queries indexed per-feed deadlines, not articles. Successful imports retain
32 distinct valid publication dates independently of article retention. The median of the
last eight publication gaps determines cadence; recent frequent feeds poll every 30 minutes,
daily feeds about every four hours, and weekly/monthly or stale feeds at most daily. Missing
dates default to two hours. Quiet feeds gradually slow, new publications accelerate polling,
and failures back off exponentially (one hour initially, capped at a day). Retry-After remains
a minimum delay for both automatic and manual requests. Deadlines and samples survive restart.
Existing caches without samples receive one unconditional download to initialize the policy,
then resume conditional requests and unchanged-body shortcuts.

Group by Feeds is a persisted preference, also available in the Feed menu and desktop list
toolbar. The existing bounded result window is stably sorted by subscription priority, with
chronological articles inside each group. A name/icon header marks each group and an overlay
follows the native list's first-visible-row signal. Uniform recycling rows reserve header
space; headers do not add selectable records or change article indexes.

Subscriptions exposes native list reordering and localized Move Up/Down controls on every
platform. Moves persist contiguous positions without changing folders. Sidebar order, group
order and the bounded four-request refresh queue share that priority. Native drag support
remains toolkit-dependent; the explicit controls work wherever dragging is unavailable.
Regression coverage lives in daynews-db polling/store tests and the grouped-feed DayScript.

### Scope dashboards

An unselected reader is a native viewport-sized dashboard built from day-piece-charts.
Individual feeds show publication rhythm, estimated word-count distribution, weekday/hour
publishing habits, contributor counts, reading time, saved/read share and source metadata.
Today shows local-hour arrivals; Unread emphasizes backlog age; Starred maps the saved
collection over publication time. Smart-feed dashboards add a publisher donut and counts.
Publisher legend links select their feed in the sidebar and navigate to its overview; hovering
a link highlights its row and matching donut segment. Stable feed IDs keep identically named
publishers separate. The shared day-piece-charts interaction grammar binds the legend and donut to one
series-projected point parameter. Conditional opacity provides hover emphasis; registered
link handlers select a publisher from either surface, with pointer cursors on its hit target. Publication plots offer date/count guides, length bars offer
category/count guides through composable Inspect bindings, and heatmap cells annotate weekday, time and article count on hover
or touch. These readouts use the plotted metadata without further database queries.
All plots describe the full selected scope, independently of the 500-row timeline window
and search. Empty scopes display honest zeroes rather than synthetic chart data. The Feed
overview toolbar action makes dashboards reachable on compact navigation stacks and lets
a reader return from an article to its overview. Selecting an article disposes the dashboard.

Native dashboard projections live on the existing database worker; browser projections use
the existing OPFS transport. Word counts are computed at article import and backfilled once
for older rows in batches of 128, with an indexed nullable metric. Charts never fetch bodies.
A per-dashboard cache keyed by article-store version, scope and local day reuses article
aggregates when a conditional feed check changes only scheduling metadata. Feed deadlines
continue to update. The dashboard owns a scope-lifetime one-second clock for countdown text;
ordinary countdown ticks perform no SQL or HTTP work. A local-midnight change refreshes
the projection once so Today and age bins stay current. Publication bins use host-local
time, and axes, numbers,
units, headings and countdown components use generated localized accessors. Reading time
is an estimate at 240 words/minute, measured from stored article text, not a downloaded full
web page; HTML markup and scripts are excluded from the word estimate.

The layout divides the offered content bounds among header, metrics, three flexible chart
cards and forecast. It has no scroll container, and layout never writes reactive state.
Length distributions use horizontal bars to keep bucket labels readable in narrow panes;
day-piece-charts thins colliding axis labels against measured text bounds. Charts use its
built-in spring transitions for initial appearance and incoming metadata, growing bars,
areas and wedges from their baselines and blending heat-map colors. Countdown ticks do not
restart chart transitions. Walkthrough captures wait for the entrance to settle. Very small panes
necessarily compress chart detail. Vector glyphs use generated resources
and theme tint; charts use Day's common canvas implementation and light/dark chrome.
Group by Feeds defaults on when no preference is present; explicit off remains respected.
Floating group headings use a translucent theme-tinted surface, rounded border and a tinted
vector fallback when the publication has no icon.

Refresh Now appears on individual feed dashboards and refreshes only the feed identity
stored in that dashboard projection. Smart-feed overviews do not offer this single-source action. The button disables while a refresh is active and the
forecast updates from the resulting stored scheduling metadata. Card borders use nested
background layers, not foreground canvases, so native mouse hit testing reaches controls
and interactive charts. DayScript taps dispatch actions directly and do not test occlusion.

Refresh forecasts use the exact adaptive interval function and persisted next-poll / retry
deadlines. Fixed timers expose their next tick; manual mode reports that no check is scheduled.
A due check reads as waiting for the scheduler rather than a negative countdown, and active
refreshes read as checking. Forecasts describe an estimate while the app is running, not an
OS background wake-up guarantee. Regression coverage: dashboard word-count, scope/cache and
migration tests in daynews-db, layout tests in src/dashboard.rs, plus dayscript/dashboards.yaml
and the dashboard captures in the main walkthrough.

Refresh and Show Only Unread Feeds share the leading sidebar toolbar group. The unread
filter uses Google Material Symbols’ Apache-2.0 mark_email_unread vector, separately from
the article-grouping filter. Bulk Mark All as Read remains in the Article menu with its
shortcut, rather than occupying the toolbar. Walkthrough bulk-marking steps use its
shared command id through the menu model.
