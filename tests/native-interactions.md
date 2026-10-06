# Native interaction regression tests

Use throwaway stores; these scripts mark articles read/unread and change synthetic feeds' reader
preferences. Never run them against a personal subscription database.

Start `python3 tests/reader-view-server.py 28772`. For Android, additionally run
`adb reverse tcp:28772 tcp:28772`. The fixture serves synthetic RSS previews and publication
HTML, including a long preview and a delayed full article. CORS is enabled for browser tests.

For a desktop target:

```sh
day launch -p macos-appkit --env DAY_NEWS_DATA_DIR=/tmp/day-news-regression \
  --script dayscript/seed-demo.yaml \
  --script dayscript/reading-navigation.yaml \
  --script dayscript/article-deselection.yaml \
  --script dayscript/reader-filter-selection.yaml \
  --script dayscript/reader-regressions.yaml \
  --script dayscript/native-reader-interactions.yaml
```

Use `macos-gtk` or `macos-qt` for the other desktop toolkits. Use `ios-uikit` with
`--ios-simulator <UDID>` and add `dayscript/feed-navigation.yaml` to check single-tap
navigation and feed overview. The iPad's split layout can legitimately keep the dashboard
visible alongside the article list. Clipboard permission must be granted for `seed-demo`,
or seed the isolated simulator store from a consistent backup of the synthetic desktop store.
On Android use an app-private store path such as
`/data/user/0/dev.daybrite.news/files/regression`. Browser tests use `web-dom` with the configured
Day web driver and a fresh browser profile; their database lives in browser storage.

The long-reader script checks visibility-triggered loading, no top loading banner, preserved
viewport/scroll, retained content after read/star changes, spinner visibility, collapse/expand,
and reversed animation. Screenshots capture appended and re-expanded content. Browser loading
may finish before a later script command can observe its transient busy state, so that
transient-state check is restricted to the five native targets.

Run `dayscript/walkthrough.yaml` after seeding for the full application flow and dashboard
screenshots. `dayscript/grouped-feeds.yaml` additionally checks desktop feed ordering and the
floating header after scrolling; the initial article must remain unobscured.

Dayscript injects framework events, not physical pointer or keyboard input. Complement it
with the real GTK selection-model test and AppKit NSTableView keyboard test in day/. For a long
reader page in an isolated macOS test app, set `window.scrollTo(0,0)` through `day drive`, run
`swift tests/macos-reader-scroll.swift <app-pid>`, and assert `scrollY > 100` through `web_eval`.
The helper requires macOS Accessibility permission and sends a real wheel event to the app's
visible content pane. This check passed on AppKit, GTK, and Qt. It does change desktop focus.
Android's article scrolling was checked with an actual
`adb shell input swipe`. Such tests do not establish IME, VoiceOver/TalkBack, physical-device,
or every theme/locale behavior. HarmonyOS and Windows runtime tests require CI or their hosts.

## Local run, October 6, 2026

| Target | Expanded interaction suite | Full walkthrough / combined suite |
| --- | --- | --- |
| macos-appkit | 171 passed | 222 passed |
| macos-gtk | 169 passed | 219 passed |
| macos-qt | Covered in combined suite | 279 passed, including grouping and reader cases |
| ios-uikit (iPad simulator) | 121 passed | 220 passed, including remaining reader cases |
| android-mdc (emulator) | 174 passed | 173 passed, including reader cases |
| web-dom (Chromium) | Covered in combined suite | 384 passed |

Counts describe separate runs and are not additive. Platform-inapplicable assertions are
skipped explicitly. All runs use synthetic subscriptions. Final targeted runs repeat affected
reader cases after the framework cleanup fixes.

Fresh UIKit Rust compilation succeeds, but Xcode simulator packaging stalls in its input-option
resolution; iPad runtime coverage therefore uses the existing simulator executable and does
not validate the final framework changes on iPad. HarmonyOS Rust compilation succeeds; local
HAP packaging is blocked by the host's missing `@ohos/hvigor-ohos-plugin`. No HarmonyOS emulator
tests were attempted. Android's full walkthrough passed on retry after reducing concurrent
browser load and using the already-seeded fixture store.
