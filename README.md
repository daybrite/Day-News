# Day News

A feed reader in three panes on a desktop and three taps on a phone, built with
[Day](https://daybrite.dev) in one Rust codebase and rendered with the platform's own widgets on
iPhone, Android, Mac, Windows, Linux, HarmonyOS, and the web.

<p align="center">
  <kbd><img src="https://daybrite.github.io/Day-News/gallery/macos-appkit/en/timeline.png" width="760" alt="Subscriptions, timeline, and article side by side on macOS"></kbd>
</p>

## Run it in one command

Install the `day` CLI, then let it clone, build, and launch the app for your desktop:

```sh
cargo install day-cli
day launch --git https://github.com/daybrite/Day-News.git
```

`day doctor` lists what your platform's toolkit needs and prints the install command for anything
missing. The launch prints where it put the checkout, so you can open the code and change it.

## What you get

Subscriptions on the left, the timeline in the middle, the article on the right, in the layout
[NetNewsWire](https://github.com/Ranchero-Software/NetNewsWire) made familiar. On a phone the same
three panes become three taps, each one a native push.

<p align="center">
  <kbd><img src="https://daybrite.github.io/Day-News/gallery/ios-uikit/iphone/en/subscriptions.png" width="200" alt="Subscriptions on iPhone"></kbd>
  <kbd><img src="https://daybrite.github.io/Day-News/gallery/ios-uikit/iphone/en/timeline.png" width="200" alt="The timeline on iPhone"></kbd>
  <kbd><img src="https://daybrite.github.io/Day-News/gallery/ios-uikit/iphone/en/article.png" width="200" alt="An article on iPhone"></kbd>
  <kbd><img src="https://daybrite.github.io/Day-News/gallery/android-mdc/phone/en/search-results.png" width="200" alt="Search results on Android"></kbd>
</p>

- RSS, Atom, RDF, and JSON Feed.
- Smart feeds for Today, All Unread, Starred, and All Articles, plus folders with unread counts
  and a marker on any feed that stopped responding.
- Full-text search across every article, matching as you type.
- Star what you want to keep and mark what you have read. A refresh never undoes either.
- Settings include live Light, Dark, or System appearance and a language selector (English
  currently ships). On Mac, Command-comma opens a dedicated Settings window, including a
  browser choice for Day News. On iOS, links follow the system browser setting.
- Article List preferences offer an independent text-size slider and zero to five preview
  lines (two by default), with matching row heights and a compact, single-row empty state.
  List text defaults to 120% and reader text to 140%; size and preview choices persist
  across launches.
- Reader preferences include a persistent text-size slider, installed-font picker with reading
  recommendations, and native background/text color wells. Article → Increase/Decrease Text
  Size (Command-plus/minus) updates the same setting. Styles apply without reloading the article.
  Reset Styles restores System appearance and automatic reader styling while keeping other settings.
- Article links open externally while the reader keeps its place. The browser controls whether
  the destination appears in a new tab or window. Article → Open in Browser opens the selected
  article with Return/Enter, without a modifier key.
- Import and export OPML with folders intact, so moving in or out is a single file.
- The timeline is a native recycling list with the platform's own swipe actions and keyboard
  navigation, and the article pane is the system web view over a document generated per article —
  or, where the platform ships no web engine, the same article composed from pieces.

The app talks directly to the sites you subscribe to. There is no account and no sync service in
the middle.

## The same code on every platform

These captures come from the app's own CI, which runs the walkthrough on every target and
publishes the results to the [gallery](https://daybrite.dev/gallery/Day-News/).

| Windows · XAML | Linux · GTK | Linux · Qt |
|:---:|:---:|:---:|
| <kbd><img src="https://daybrite.github.io/Day-News/gallery/windows-xaml/en/article.png" width="300" alt="All Articles on Windows"></kbd> | <kbd><img src="https://daybrite.github.io/Day-News/gallery/linux-gtk/en/timeline.png" width="300" alt="Timeline on GTK"></kbd> | <kbd><img src="https://daybrite.github.io/Day-News/gallery/linux-qt/en/timeline.png" width="300" alt="Timeline on Qt"></kbd> |

| Web · DOM | Android · Material |
|:---:|:---:|
| <kbd><img src="https://daybrite.github.io/Day-News/gallery/web-dom/en/article.png" width="300" alt="An article in the browser"></kbd> | <kbd><img src="https://daybrite.github.io/Day-News/gallery/android-mdc/phone/en/article.png" width="150" alt="An article on Android"></kbd> |

Managing subscriptions, the timeline scoped to a tag, and the sidebar tucked away:

<p align="center">
  <kbd><img src="https://daybrite.github.io/Day-News/gallery/macos-appkit/en/subscriptions.png" width="360" alt="Subscriptions management on macOS"></kbd>
  <kbd><img src="https://daybrite.github.io/Day-News/gallery/macos-appkit/en/tag-scope.png" width="360" alt="The timeline scoped to a tag on macOS"></kbd>
</p>
<p align="center">
  <kbd><img src="https://daybrite.github.io/Day-News/gallery/macos-appkit/en/sidebar-hidden.png" width="360" alt="The sidebar hidden on macOS"></kbd>
  <kbd><img src="https://daybrite.github.io/Day-News/gallery/macos-appkit/en/settings.png" width="360" alt="Settings on macOS"></kbd>
</p>

## Build from a clone

Day compiles one toolkit backend per binary, so name a target when you build or launch. Every
target the app ships is listed in `Day.toml`.

```sh
day doctor                       # toolchains present and missing, with fixes
day launch -p macos-appkit       # build + run
day launch -p ios-uikit          # needs a booted Simulator
day launch -p android-mdc        # needs a JDK and a running emulator or device
day launch -p web-dom            # serves the WebAssembly build locally
```

A bare `cargo build` uses the crate's default `mock` backend, which is what lets rust-analyzer and
`cargo check` work with no flags. To pick a toolkit from plain cargo, turn the default off first:

```sh
cargo build --no-default-features --features appkit    # or gtk / qt / uikit / mdc / xaml / dom
```

A fresh install has no subscriptions. Seed some, then drive the whole reader loop:

```sh
day launch -p macos-appkit --script dayscript/seed-demo.yaml    # the bundled demo feeds, offline
day launch -p macos-appkit --script dayscript/import.yaml       # a sample OPML, through the file picker
day launch -p android-mdc  --script dayscript/seed-mobile.yaml  # a few live feeds, by URL
day launch -p macos-appkit --script dayscript/walkthrough.yaml  # the full loop, with screenshots
```

For an isolated validation run, pass `--env DAY_NEWS_DATA_DIR=/absolute/writable/path`
to `day launch`. This selects a separate store for the demo scripts; omit it for normal use.
On iOS the path must be inside the app's sandbox.

Those [dayscripts](https://daybrite.dev/docs/dayscript) are the UI tests, and the walkthrough is
what CI runs on every target to produce the gallery. Everything below the UI is testable without a
screen or a network: `cargo test --workspace`.

To build against a local `day` checkout instead of the pinned git revision, let the CLI write and
verify the patch table:

```sh
day patch --local /path/to/day
```

## Inside the code

**Reader View** (Article menu, **⌘⇧R** on macOS / **Ctrl+Shift+R** elsewhere) loads the
publisher's full article into the current reader. The article toolbar exposes the same command
on phones. Invoke it again to return to the RSS version, or while loading to cancel. Your
reader typography and colors apply to both versions.

Extraction runs locally using bundled Mozilla Readability and DOMPurify. It does not execute
publisher scripts, so pages requiring JavaScript, sign-in, or a subscription may not yield an
article. Web builds also remain subject to publisher CORS rules. NetNewsWire's Feedbin service
requires authorized application credentials; Day News does not reuse NetNewsWire's credentials
or impersonate it. `ArticleExtractor` in `src/extraction.rs` is the replacement point for an
authorized service or another local engine.

- `src/lib.rs` is the shell: a typed-route sidebar whose article list is a content-list pane, so
  desktops get three columns and a phone pushes through them.
- `src/timeline.rs` is the article list, a native recycling [`list`](https://daybrite.dev/docs/internal/list)
  with platform selection and edge swipe actions.
- `src/reader.rs` is the article pane: a native web view over a generated document, and the same
  article composed from pieces when a backend has no web engine. GTK on macOS uses WebKit.
- `src/extraction.rs` defines the extraction provider; `src/reader_view.rs` owns per-window
  loading, cancellation, errors, and the temporary full-text overlay.
- `src/subscriptions.rs`, `src/settings.rs`, `src/menus.rs`, and `src/toolbar.rs` cover feed
  management with OPML import and export, retention, the app menus, and the window toolbar.
- `crates/` holds `daynews-opml`, `daynews-feed`, `daynews-db`, and `daynews-core`: OPML, feed
  parsing, the store, and the view model, all headless.
- `resource/locales/en/app.ftl` carries every user-facing string.
- `resource/assets/demo/` holds the articles the screenshots show: seven feeds written for the
  app, one set per language, read offline from inside the app.
- `platform/` holds the thin native host projects the mobile targets build through.

Test fixtures live in the repository; `crates/daynews-feed/tests/data` and
`crates/daynews-opml/tests/data` record where the captured feeds and the vendored OPML samples came
from. `day lint` checks routes, element ids, and locale coverage, and `DESIGN.md` is the
architecture.

Day News is open source under the Apache-2.0 license.

Feed refreshes run up to four requests concurrently, including newly added subscriptions.
ETag and Last-Modified validators survive relaunch; unchanged feeds return 304 without
reparsing or rewriting articles. On AppKit/UIKit, active feeds show a subtle spinner over
their sidebar icon while connecting or when the response length is unknown. A known-length
download fills a circular progress ring. Feed names and unread counts stay in place. Native
feed parsing runs off the UI thread, and downloads have a 30-second request deadline and
16 MiB body limit.

The sidebar discovers color site icons from feed metadata, home-page icon links, web
manifests, and conventional favicon/touch-icon locations. Raster icons are normalized to
64-pixel PNG thumbnails and cached in the app data directory (seven days for successful
lookups, one day for misses). Cached thumbnails appear before network discovery, which runs
four sites at a time with a 25-second per-site deadline. Sites without a usable icon keep the
RSS symbol; no third-party favicon service receives the subscription list. SVG-only icons
currently fall back to another raster candidate or the RSS symbol. Web builds retain the RSS
fallback because the local filesystem cache is a native capability.

Feed → Show Only Unread Feeds and the toolbar's filter button (above the article list on macOS, in the feed bar on iOS) control the same
persistent preference. Smart feeds remain available, and hiding an empty feed does not close
an already open article.

Network regressions: run `python3 tests/feed-refresh-server.py`, then launch against a fresh
`DAY_NEWS_DATA_DIR` with `--script dayscript/feed-refresh.yaml`. Relaunch the same library on
macOS with `--script dayscript/feed-refresh-relaunch.yaml`; `/stats` on port 28762 should show
additional 304s, no new icon downloads, and at most four active feed requests.
