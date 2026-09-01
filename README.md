# Day News

A feed reader built on [Day](https://daybrite.dev), modeled on
[NetNewsWire](https://github.com/Ranchero-Software/NetNewsWire): subscriptions on the left, a
timeline in the middle, the article on the right — collapsing to push navigation on a phone.
One Rust codebase, native widgets on every platform.

## Run it

`day launch --git` clones this repo, builds it for your desktop, and runs it — no checkout needed:

```sh
cargo install day-cli
day doctor                                                # what's installed, what's missing
day launch --git https://github.com/daybrite/Day-News.git
```

`day doctor` prints the fix for anything it can't find. `day launch --git` prints where it put the
checkout, so you can `cd` there and edit the code.

From inside a clone, name a target instead. Day compiles **exactly one backend per binary**, and
the Day CLI supplies the right feature for each:

```sh
day launch -p macos-appkit   # build + run
day build  -p macos-appkit   # build only
```

Targets live in `Day.toml`. A bare `cargo build` uses this crate's default `mock` backend, which is
what lets rust-analyzer and `cargo check` work with no flags. To pick a real one from plain cargo,
turn the default off as well — otherwise `mock` and your choice are both on, which is two backends
and a compile error:

```sh
cargo build --no-default-features --features appkit    # or gtk / qt / uikit / mdc / xaml / dom
```

A fresh install has no subscriptions. Seed one:

```sh
day launch -p macos-appkit --script dayscript/import.yaml      # a sample OPML, through the file picker
day launch -p android-mdc  --script dayscript/seed-mobile.yaml # a few real feeds, by URL
```

Then drive the whole reader loop:

```sh
day launch -p macos-appkit --script dayscript/walkthrough.yaml
```

## What's inside

- `src/lib.rs` — the shell: a typed-route sidebar
  ([navigation](https://daybrite.dev/docs/navigation)) whose article list is a real
  content-list pane, so desktops get three columns and a phone pushes through them.
- `src/timeline.rs` — the article list, a native recycling
  [`list`](https://daybrite.dev/docs/list) with platform selection and edge swipe actions
  (read/unread trailing, star leading).
- `src/reader.rs` — the article pane: a native web view over a document generated per article.
- `src/subscriptions.rs`, `src/settings.rs`, `src/menus.rs`, `src/toolbar.rs` — feed management
  and OPML import/export, retention, the app menus, and the window toolbar.
- `crates/` — `daynews-opml`, `daynews-feed`, `daynews-db` and `daynews-core`: OPML, feed
  parsing, the store, and the view-model. Everything except the UI is testable without a
  screen or a network (`cargo test --workspace`).
- `resource/locales/en/app.ftl` — every user-facing string
  ([localization](https://daybrite.dev/docs/localization)).
- `dayscript/` — [dayscript](https://daybrite.dev/docs/dayscript) UI tests: `walkthrough.yaml`
  is the full reader loop, `import.yaml` and `seed-mobile.yaml` seed a store, and the rest
  cover menus, the reader, and polish.
- `platform/` — the thin native host projects (Xcode / Gradle) the mobile targets build
  through; `day build` keeps their identity in sync with `Day.toml`.
- `Day.toml` — app metadata + the target list.

Test resources live in the repository — never a path into a home directory — so every script
and test runs on any machine and on CI. `crates/daynews-opml/tests/data` records where the
vendored OPML samples came from.

`day lint` checks routes, element ids, and locale coverage. `DESIGN.md` is the architecture.
