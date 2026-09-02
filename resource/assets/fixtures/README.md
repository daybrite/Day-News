# Captured feed fixtures

Real feeds, saved byte for byte as their publishers served them on 2026-09-01. One copy does two
jobs: `crates/daynews-feed/tests/real_feeds.rs` parses them offline, and
`dayscript/seed-fixtures.yaml` subscribes to them through the `asset:` URL scheme, which is how
the CI walkthrough gets articles without a network. That second job is why they live under
`resource/assets/` and ship inside the app bundle, and why this note sits beside them.

| File | Source | Shape |
|---|---|---|
| `nasa.xml` | https://www.nasa.gov/feed/ | WordPress RSS 2.0: `content:encoded` bodies, `dc:creator` |
| `quanta.xml` | https://www.quantamagazine.org/feed/ | WordPress RSS 2.0: `content:encoded`, `media:` thumbnails |
| `sciencedaily.xml` | https://www.sciencedaily.com/rss/all.xml | Plain RSS 2.0, summary-only items |
| `merriam-webster.xml` | https://www.merriam-webster.com/wotd/feed/rss2 | Plain RSS 2.0, HTML in `description` |
| `rust-forum.xml` | https://users.rust-lang.org/c/announcements/6.rss | Discourse RSS 2.0, `dc:creator` usernames |
| `rust-blog.xml` | https://blog.rust-lang.org/feed.xml | Atom with full `content` |
| `rust-mastodon.xml` | https://social.rust-lang.org/@rust.rss | Mastodon RSS 2.0; items carry no `<title>` |

Each publisher keeps the copyright in its own content. The files are parser fixtures and demo
seed data, captured verbatim and unedited. To refresh one, fetch its URL again, replace the file,
and run `cargo test -p daynews-feed`: the tests assert on shape (titles, ids, links, entity
decoding), not on particular articles, so a fresh capture passes without edits. The walkthrough
searches for "space", which the NASA, Quanta and ScienceDaily feeds all carry.
