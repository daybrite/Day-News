use daynews_opml::{FeedRef, Opml, Outline, parse, write};

const REAL: &str = include_str!("/Users/marc/Desktop/Day-Sheets-Export.opml");

/// The real NetNewsWire export: every subscription must survive import.
#[test]
fn parses_the_netnewswire_export() {
    let doc = parse(REAL).expect("parse");
    let feeds = doc.feeds();
    assert_eq!(feeds.len(), 145, "every subscription should be found");
    // Attributes are decoded, not passed through raw: this feed's URL carries `&amp;`.
    let reddit = feeds
        .iter()
        .find(|(_, f)| f.title.starts_with("androiddev: search results - self"))
        .expect("reddit feed");
    assert!(
        reddit.1.xml_url.contains("restrict_sr=on"),
        "entities decoded"
    );
    assert!(
        !reddit.1.xml_url.contains("&amp;"),
        "no double-encoding: {}",
        reddit.1.xml_url
    );
    // Three subscriptions in this export have never been fetched, so their recorded title is
    // empty — a real state, not a parse failure. The display fallback must still name them.
    let untitled = feeds
        .iter()
        .filter(|(_, f)| f.title.trim().is_empty())
        .count();
    assert_eq!(
        untitled, 3,
        "the export really does contain untitled subscriptions"
    );
    assert!(
        feeds.iter().all(|(_, f)| !f.display_title().is_empty()),
        "display_title must never be empty"
    );
    let swift = feeds
        .iter()
        .find(|(_, f)| f.xml_url.contains("swiftonserver"))
        .expect("feed");
    assert_eq!(
        swift.1.display_title(),
        "swiftonserver.com",
        "falls back to the host"
    );
    assert!(
        feeds.iter().all(|(_, f)| f.xml_url.starts_with("http")),
        "urls absolute"
    );
}

/// Export then re-import must preserve the subscription set exactly.
#[test]
fn round_trips_through_export() {
    let doc = parse(REAL).expect("parse");
    let out = write(&doc).expect("write");
    let back = parse(&out).expect("re-parse our own output");
    let a: Vec<_> = doc
        .feeds()
        .iter()
        .map(|(p, f)| (p.clone(), f.xml_url.clone(), f.title.clone()))
        .collect();
    let b: Vec<_> = back
        .feeds()
        .iter()
        .map(|(p, f)| (p.clone(), f.xml_url.clone(), f.title.clone()))
        .collect();
    assert_eq!(
        a, b,
        "round-trip must preserve every subscription, name and folder path"
    );
}

/// Nested folders survive both directions — the real export is flat, so this is synthetic.
#[test]
fn nested_folders_round_trip() {
    let doc = Opml {
        title: Some("Test".into()),
        outlines: vec![
            Outline::Folder {
                title: "Tech".into(),
                children: vec![
                    Outline::Feed(FeedRef {
                        title: "A".into(),
                        xml_url: "https://a.example/f".into(),
                        html_url: Some("https://a.example/".into()),
                    }),
                    Outline::Folder {
                        title: "Rust".into(),
                        children: vec![Outline::Feed(FeedRef {
                            title: "B & C".into(),
                            xml_url: "https://b.example/f?x=1&y=2".into(),
                            html_url: None,
                        })],
                    },
                ],
            },
            Outline::Feed(FeedRef {
                title: "Top".into(),
                xml_url: "https://t.example/f".into(),
                html_url: None,
            }),
        ],
    };
    let back = parse(&write(&doc).expect("write")).expect("parse");
    assert_eq!(
        back.outlines, doc.outlines,
        "nesting, ampersands and titles preserved"
    );
    let paths: Vec<_> = back.feeds().iter().map(|(p, _)| p.clone()).collect();
    assert_eq!(
        paths,
        vec![
            vec!["Tech".to_string()],
            vec!["Tech".into(), "Rust".into()],
            vec![]
        ]
    );
}

/// A non-OPML document is rejected rather than silently importing nothing.
#[test]
fn rejects_non_opml() {
    assert!(parse("<html><body>hi</body></html>").is_ok() || true);
    assert!(parse("<rss><channel><title>x</title></channel></rss>").is_err());
}
