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
day launch -p macos-appkit --script dayscript/import.yaml       # a sample OPML, through the file picker
day launch -p android-mdc  --script dayscript/seed-mobile.yaml  # a few feeds, by URL
day launch -p macos-appkit --script dayscript/walkthrough.yaml  # the full loop, with screenshots
```

Those [dayscripts](https://daybrite.dev/docs/dayscript) are the UI tests, and the walkthrough is
what CI runs on every target to produce the gallery. Everything below the UI is testable without a
screen or a network: `cargo test --workspace`.

To build against a local `day` checkout instead of the pinned git revision, let the CLI write and
verify the patch table:

```sh
day patch --local /path/to/day
```

## Inside the code

- `src/lib.rs` is the shell: a typed-route sidebar whose article list is a content-list pane, so
  desktops get three columns and a phone pushes through them.
- `src/timeline.rs` is the article list, a native recycling [`list`](https://daybrite.dev/docs/internal/list)
  with platform selection and edge swipe actions.
- `src/reader.rs` is the article pane: a native web view over a generated document, and the same
  article composed from pieces on a backend with no web engine (macos-gtk).
- `src/subscriptions.rs`, `src/settings.rs`, `src/menus.rs`, and `src/toolbar.rs` cover feed
  management with OPML import and export, retention, the app menus, and the window toolbar.
- `crates/` holds `daynews-opml`, `daynews-feed`, `daynews-db`, and `daynews-core`: OPML, feed
  parsing, the store, and the view model, all headless.
- `resource/locales/en/app.ftl` carries every user-facing string.
- `platform/` holds the thin native host projects the mobile targets build through.

Test fixtures live in the repository; `crates/daynews-opml/tests/data` records where the vendored
OPML samples came from. `day lint` checks routes, element ids, and locale coverage, and
`DESIGN.md` is the architecture.

Day News is open source under the Apache-2.0 license.
