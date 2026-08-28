//! Parsing checks against feeds actually captured from the wire — RSS 2.0, Atom, Mastodon and
//! Discourse variants. Fixtures are bytes, so these run offline and deterministically.
use daynews_feed::parse;

struct Fixture {
    bytes: &'static [u8],
    url: &'static str,
    name: &'static str,
}

const FIXTURES: &[Fixture] = &[
    Fixture {
        bytes: include_bytes!("fixtures/feed1.xml"),
        url: "https://andrewkelley.me/rss.xml",
        name: "plain RSS 2.0",
    },
    Fixture {
        bytes: include_bytes!("fixtures/feed2.xml"),
        url: "https://blog.cryptographyengineering.com/feed/",
        name: "WordPress RSS + content:encoded",
    },
    Fixture {
        bytes: include_bytes!("fixtures/feed3.xml"),
        url: "https://forums.swift.org/c/general-announce/24.rss",
        name: "Discourse RSS",
    },
    Fixture {
        bytes: include_bytes!("fixtures/feed4.xml"),
        url: "https://www.youtube.com/feeds/videos.xml?channel_id=UCVHFbqXqoYvEWM1Ddxl0QDg",
        name: "YouTube Atom",
    },
    Fixture {
        bytes: include_bytes!("fixtures/feed5.xml"),
        url: "https://mas.to/@aabewhite.rss",
        name: "Mastodon RSS",
    },
];

#[test]
fn every_real_feed_parses_with_usable_items() {
    for f in FIXTURES {
        let feed = parse(f.bytes, f.url).unwrap_or_else(|e| panic!("{}: {e}", f.name));
        assert!(feed.title.is_some(), "{}: feed title", f.name);
        assert!(!feed.items.is_empty(), "{}: items", f.name);
        for it in &feed.items {
            assert!(
                !it.guid.trim().is_empty(),
                "{}: every item needs a stable id",
                f.name
            );
        }
        // Microblog feeds (Mastodon) ship items with no <title> at all, so the requirement is
        // that every item can still be NAMED and opened.
        for it in &feed.items {
            assert!(
                !it.display_title().is_empty(),
                "{}: item must be nameable",
                f.name
            );
        }
        let linked = feed.items.iter().filter(|i| i.url.is_some()).count();
        assert!(
            linked * 2 >= feed.items.len(),
            "{}: most items should link somewhere",
            f.name
        );
        println!(
            "{:34} {:>4} items, title={:?}, site={:?}",
            f.name,
            feed.items.len(),
            feed.title.as_deref().unwrap_or(""),
            feed.site_url.as_deref().unwrap_or("")
        );
    }
}

/// Item identity must be stable across re-parses, or every refresh resurrects read articles.
#[test]
fn item_ids_are_stable_and_unique() {
    for f in FIXTURES {
        let a = parse(f.bytes, f.url).expect("parse");
        let b = parse(f.bytes, f.url).expect("re-parse");
        let ids_a: Vec<_> = a.items.iter().map(|i| i.guid.clone()).collect();
        let ids_b: Vec<_> = b.items.iter().map(|i| i.guid.clone()).collect();
        assert_eq!(ids_a, ids_b, "{}: ids must be deterministic", f.name);
        let uniq: std::collections::HashSet<_> = ids_a.iter().collect();
        assert_eq!(
            uniq.len(),
            ids_a.len(),
            "{}: ids must be unique within a feed",
            f.name
        );
    }
}

/// Titles are plain text: no tags, no raw entities.
#[test]
fn titles_are_clean_text() {
    for f in FIXTURES {
        let feed = parse(f.bytes, f.url).expect("parse");
        for it in feed.items.iter().filter_map(|i| i.title.as_ref()) {
            assert!(
                !it.contains('<') && !it.contains('>'),
                "{}: tag in title: {it:?}",
                f.name
            );
            assert!(
                !it.contains("&amp;") && !it.contains("&#"),
                "{}: undecoded entity: {it:?}",
                f.name
            );
        }
    }
}

/// Garbage in must not panic — a publisher serving an HTML error page is routine.
#[test]
fn non_feed_input_errors_cleanly() {
    assert!(
        parse(
            b"<html><body>404 not found</body></html>",
            "https://x.example/f"
        )
        .is_err()
    );
    assert!(parse(b"", "https://x.example/f").is_err());
    assert!(parse(b"\x00\x01\x02 not xml", "https://x.example/f").is_err());
}

/// The Mastodon case explicitly: title-less items get a readable name from their body, with the
/// escaped markup resolved rather than shown.
#[test]
fn microblog_items_derive_a_title_from_content() {
    let f = &FIXTURES[4];
    let feed = parse(f.bytes, f.url).expect("parse");
    let first = feed.items.first().expect("an item");
    assert!(first.title.is_none(), "Mastodon items carry no <title>");
    let t = first.display_title();
    assert!(!t.is_empty() && t.len() <= 130, "derived title: {t:?}");
    assert!(
        !t.contains('<') && !t.contains("&lt;"),
        "markup must be resolved, got {t:?}"
    );
    println!("derived title: {t}");
}

/// Prose containing a bare `<` is not mistaken for markup.
#[test]
fn angle_brackets_in_prose_survive() {
    let xml = br#"<?xml version="1.0"?><rss version="2.0"><channel><title>T</title>
      <link>https://e.example/</link>
      <item><title>Why 2 &lt; 3 &amp; 4 &gt; 1</title><link>https://e.example/a</link>
      <guid>a</guid></item></channel></rss>"#;
    let feed = parse(xml, "https://e.example/f").expect("parse");
    assert_eq!(feed.items[0].title.as_deref(), Some("Why 2 < 3 & 4 > 1"));
}

/// Summaries are shown as PLAIN TEXT in the timeline, so typographic entities must be resolved —
/// publishers escape them constantly and nothing downstream would decode them.
#[test]
fn typographic_entities_are_decoded_in_text() {
    let xml = br#"<?xml version="1.0"?><rss version="2.0"><channel><title>T</title>
      <link>https://e.example/</link>
      <item><title>We&rsquo;ve shipped&hellip;</title><link>https://e.example/a</link><guid>a</guid>
      <description>&ldquo;privatized&rdquo; photos &mdash; and more &amp; more</description>
      </item></channel></rss>"#;
    let feed = parse(xml, "https://e.example/f").expect("parse");
    let it = &feed.items[0];
    assert_eq!(it.title.as_deref(), Some("We’ve shipped…"));
    assert_eq!(
        it.summary.as_deref(),
        Some("“privatized” photos — and more & more")
    );
    assert!(
        !it.summary.as_deref().unwrap().contains('&')
            || it.summary.as_deref().unwrap().contains("& more")
    );
}

/// Every summary from the real fixtures must be free of leftover named entities.
#[test]
fn real_feed_summaries_have_no_raw_entities() {
    for f in FIXTURES {
        let feed = parse(f.bytes, f.url).expect("parse");
        for s in feed.items.iter().filter_map(|i| i.summary.as_ref()) {
            for bad in [
                "&rsquo;", "&ldquo;", "&rdquo;", "&mdash;", "&amp;", "&#8217;",
            ] {
                assert!(
                    !s.contains(bad),
                    "{}: undecoded {bad} in summary: {s:.90}",
                    f.name
                );
            }
        }
    }
}

/// Documents the decoding layers, which are easy to get wrong: the XML parser resolves one
/// level of escaping before we ever see the text, and `clean` resolves what remains. A publisher
/// writing `&amp;amp;` has escaped twice and means a literal `&`, which is what a reader shows.
#[test]
fn double_escaped_text_resolves_to_one_ampersand() {
    let xml = br#"<?xml version="1.0"?><rss version="2.0"><channel><title>T</title>
      <link>https://e.example/</link>
      <item><title>Tips &amp;amp; tricks</title><link>https://e.example/a</link>
      <guid>a</guid></item></channel></rss>"#;
    let feed = parse(xml, "https://e.example/f").expect("parse");
    assert_eq!(feed.items[0].title.as_deref(), Some("Tips & tricks"));
}
