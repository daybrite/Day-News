//! Fetching and parsing syndication feeds, normalized to the shape the store keeps.
//!
//! [`parse`] is pure and offline-testable; [`fetch`] adds the network on top of it. Policy that
//! the app depends on — which field becomes the title, how an article's stable identity is
//! derived, which body wins — lives here rather than being spread through the UI.

pub use day_part_http::HttpError;

/// A feed as we store it: channel metadata plus its current items.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ParsedFeed {
    pub title: Option<String>,
    pub site_url: Option<String>,
    pub description: Option<String>,
    pub icon_url: Option<String>,
    pub items: Vec<ParsedItem>,
}

/// One article. `guid` is the stable identity used to recognize an item across refreshes.
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedItem {
    pub guid: String,
    pub title: Option<String>,
    pub url: Option<String>,
    pub author: Option<String>,
    /// Unix seconds. `None` when the feed omits a date (some do) — the store falls back to
    /// first-seen time so ordering stays stable.
    pub published: Option<i64>,
    pub summary: Option<String>,
    /// The richest body the feed offered, as HTML.
    pub content_html: Option<String>,
}

impl ParsedItem {
    /// A name that is never empty. Microblog feeds (Mastodon, and anything else posting short
    /// updates) ship items with NO `<title>` at all — only a body — so readers show the start of
    /// the content instead. Falls back further to the link, then the id.
    pub fn display_title(&self) -> String {
        if let Some(t) = self
            .title
            .as_ref()
            .map(|t| t.trim())
            .filter(|t| !t.is_empty())
        {
            return t.to_string();
        }
        let body = self
            .summary
            .as_deref()
            .or(self.content_html.as_deref())
            .unwrap_or("");
        let text = clean(body);
        if !text.is_empty() {
            return truncate_on_word(&text, 120);
        }
        self.url.clone().unwrap_or_else(|| self.guid.clone())
    }
}

/// Cut to at most `max` chars, preferring a word boundary, with an ellipsis when shortened.
fn truncate_on_word(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let cut: String = s.chars().take(max).collect();
    let cut = match cut.rsplit_once(' ') {
        // Only honour the word boundary if it does not throw away most of the text.
        Some((head, _)) if head.chars().count() >= max / 2 => head,
        _ => cut.trim_end(),
    };
    format!("{}…", cut.trim_end_matches([',', '.', ';', ':', ' ']))
}

#[derive(Debug)]
pub enum FeedError {
    Http(HttpError),
    /// A non-2xx response; the feed may have moved or be gone.
    Status(u16),
    /// The bytes did not parse as any syndication format we understand.
    Parse(String),
}

impl std::fmt::Display for FeedError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FeedError::Http(e) => write!(f, "network error: {e:?}"),
            FeedError::Status(s) => write!(f, "server returned HTTP {s}"),
            FeedError::Parse(e) => write!(f, "could not parse feed: {e}"),
        }
    }
}

impl std::error::Error for FeedError {}

/// Parse feed bytes. `base_url` is the feed's own URL, used to resolve relative links and as
/// the last resort for deriving an item id.
pub fn parse(bytes: &[u8], base_url: &str) -> Result<ParsedFeed, FeedError> {
    let parsed = feed_rs::parser::Builder::new()
        .base_uri(Some(base_url))
        // Feed HTML is untrusted and we hand it to a native web view, so strip scripts and
        // event handlers at the parse boundary rather than trusting the renderer.
        .sanitize_content(true)
        .build()
        .parse(bytes)
        .map_err(|e| FeedError::Parse(e.to_string()))?;
    Ok(normalize(parsed, base_url))
}

/// Fetch and parse. Sends a browser-ish `Accept` because some hosts serve HTML to unknown
/// clients, and follows the part's platform-native redirect handling.
pub async fn fetch(url: &str) -> Result<ParsedFeed, FeedError> {
    fetch_with_base(url, url).await
}

/// [`fetch`] with the parse base split from the fetch target — for a caller whose fetch URL
/// is not a usable base (the web build fetches bundled `asset:` feeds as RELATIVE same-origin
/// URLs, and URL resolution inside the parser needs an absolute base).
pub async fn fetch_with_base(fetch_url: &str, base_url: &str) -> Result<ParsedFeed, FeedError> {
    let req = day_part_http::Request::get(fetch_url)
        .header(
            "Accept",
            "application/atom+xml, application/rss+xml, application/xml;q=0.9, */*;q=0.8",
        )
        .header("User-Agent", USER_AGENT);
    let res = day_part_http::fetch_future(req)
        .await
        .map_err(FeedError::Http)?;
    if !(200..300).contains(&res.status) {
        return Err(FeedError::Status(res.status));
    }
    parse(&res.body, base_url)
}

/// Identifies us to publishers; several block requests with no agent string.
pub const USER_AGENT: &str = concat!(
    "DayNews/",
    env!("CARGO_PKG_VERSION"),
    " (+https://daybrite.dev)"
);

fn normalize(f: feed_rs::model::Feed, base_url: &str) -> ParsedFeed {
    // The human-facing site: `rel="alternate"` when present, otherwise any link that is not the
    // feed's own address (WordPress lists the feed itself first, which would send readers in a
    // circle when they click "open website").
    let site_url = f
        .links
        .iter()
        .find(|l| l.rel.as_deref() == Some("alternate") && l.href != base_url)
        .or_else(|| f.links.iter().find(|l| l.href != base_url))
        .map(|l| l.href.clone());
    ParsedFeed {
        title: f.title.map(|t| clean(&t.content)).filter(|s| !s.is_empty()),
        site_url,
        description: f
            .description
            .map(|t| clean(&t.content))
            .filter(|s| !s.is_empty()),
        icon_url: f.icon.or(f.logo).map(|i| i.uri),
        items: f
            .entries
            .into_iter()
            .map(|e| normalize_entry(e, base_url))
            .collect(),
    }
}

fn normalize_entry(e: feed_rs::model::Entry, base_url: &str) -> ParsedItem {
    let url = e
        .links
        .iter()
        .find(|l| l.rel.as_deref() == Some("alternate"))
        .or_else(|| e.links.first())
        .map(|l| l.href.clone());

    // Body preference mirrors what readers show: the full `content` when present, otherwise the
    // summary. Keeping both lets the list show a snippet while the reader shows the article.
    let content_html = e
        .content
        .as_ref()
        .and_then(|c| c.body.clone())
        .filter(|b| !b.trim().is_empty());
    let summary = e
        .summary
        .as_ref()
        .map(|t| clean(&t.content))
        .filter(|s| !s.is_empty());

    ParsedItem {
        // Identity, most stable first: the feed's own id, then the article URL, then a hash of
        // title+date. Without this an item reappears as "new" whenever a publisher rewrites a
        // field, which is the classic feed-reader bug.
        guid: first_non_empty([
            Some(e.id.clone()),
            url.clone(),
            Some(fallback_id(&e, base_url)),
        ]),
        title: e.title.map(|t| clean(&t.content)).filter(|s| !s.is_empty()),
        url,
        author: e
            .authors
            .first()
            .map(|a| a.name.clone())
            .filter(|s| !s.is_empty()),
        published: e
            .published
            .or(e.updated)
            .map(|d| d.timestamp())
            .filter(|t| *t > 0),
        summary,
        content_html,
    }
}

fn first_non_empty<const N: usize>(candidates: [Option<String>; N]) -> String {
    candidates
        .into_iter()
        .flatten()
        .find(|s| !s.trim().is_empty())
        .unwrap_or_default()
}

/// A deterministic id for an entry with neither id nor link: hash what identity it does have.
fn fallback_id(e: &feed_rs::model::Entry, base_url: &str) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut eat = |s: &str| {
        for b in s.as_bytes() {
            h ^= *b as u64;
            h = h.wrapping_mul(0x100_0000_01b3);
        }
    };
    eat(base_url);
    if let Some(t) = &e.title {
        eat(&t.content);
    }
    if let Some(d) = e.published.or(e.updated) {
        eat(&d.timestamp().to_string());
    } else {
        // Nothing identifying at all: fall back to now, so at least it is not confused with a
        // different item. It will look new once; there is no better answer.
        eat(&now_secs().to_string());
    }
    format!("news:{h:016x}")
}

fn now_secs() -> i64 {
    // day-part-timezone rather than `SystemTime::now()`, which aborts on wasm32 — on web this
    // is the page's `Date.now()`.
    (day_part_timezone::now_epoch_ms() / 1000) as i64
}

/// Collapse whitespace and strip tags from a text field. Feeds put HTML in `<title>` more often
/// than they should, and a title with markup in it looks broken in a list.
fn clean(s: &str) -> String {
    // Decode FIRST: feeds routinely escape a whole HTML body into a text field, so the tags
    // only become visible after decoding (`&lt;p&gt;` → `<p>`).
    let decoded = decode_entities(s);
    let chars: Vec<char> = decoded.chars().collect();
    let mut out = String::with_capacity(decoded.len());
    let mut i = 0;
    let mut last_space = true;
    // Whether the field turned out to hold escaped HTML — see the second decode below.
    let mut stripped_a_tag = false;
    while i < chars.len() {
        let c = chars[i];
        // A `<` only opens a tag when a name, a closing slash or a declaration follows; that
        // keeps prose like "a < b" intact.
        if c == '<'
            && chars
                .get(i + 1)
                .is_some_and(|n| n.is_ascii_alphabetic() || *n == '/' || *n == '!')
        {
            match chars[i..].iter().position(|c| *c == '>') {
                Some(end) => {
                    i += end + 1;
                    stripped_a_tag = true;
                    // A stripped tag is a word break, not a join.
                    if !last_space {
                        out.push(' ');
                        last_space = true;
                    }
                    continue;
                }
                None => break, // unterminated tag: drop the remainder
            }
        }
        if c.is_whitespace() {
            if !last_space {
                out.push(' ');
                last_space = true;
            }
        } else {
            out.push(c);
            last_space = false;
        }
        i += 1;
    }
    let out = out.trim();
    // Escaped HTML is escaped TWICE: once for the markup itself, and again for the entities
    // inside it, so a publisher's `&` arrives as `&amp;amp;`. Finding tags after the first
    // decode proves this field was escaped HTML, which is what makes a second pass safe — text
    // that merely mentions "&amp;" has no tags and is left exactly as written.
    if stripped_a_tag {
        decode_entities(out)
    } else {
        out.to_string()
    }
}

/// The named entities a reader actually meets in feed text, plus the numeric forms.
///
/// Publishers escape typographic punctuation constantly (`&rsquo;`, `&ldquo;`, `&mdash;`), and
/// summaries are shown as PLAIN TEXT in the timeline — nothing downstream will decode them, so
/// an undecoded entity is visible to the user as literal `&rsquo;`. Bodies are different: they
/// go to a web view, which decodes the full HTML set itself.
const NAMED_ENTITIES: &[(&str, char)] = &[
    ("amp", '&'),
    ("lt", '<'),
    ("gt", '>'),
    ("quot", '"'),
    ("apos", '\''),
    ("nbsp", ' '),
    ("ensp", ' '),
    ("emsp", ' '),
    ("thinsp", ' '),
    ("rsquo", '\u{2019}'),
    ("lsquo", '\u{2018}'),
    ("rdquo", '\u{201D}'),
    ("ldquo", '\u{201C}'),
    ("sbquo", '\u{201A}'),
    ("bdquo", '\u{201E}'),
    ("prime", '\u{2032}'),
    ("Prime", '\u{2033}'),
    ("mdash", '\u{2014}'),
    ("ndash", '\u{2013}'),
    ("minus", '\u{2212}'),
    ("hyphen", '\u{2010}'),
    ("hellip", '\u{2026}'),
    ("bull", '\u{2022}'),
    ("middot", '\u{00B7}'),
    ("copy", '\u{00A9}'),
    ("reg", '\u{00AE}'),
    ("trade", '\u{2122}'),
    ("deg", '\u{00B0}'),
    ("laquo", '\u{00AB}'),
    ("raquo", '\u{00BB}'),
    ("times", '\u{00D7}'),
    ("divide", '\u{00F7}'),
    ("frac12", '\u{00BD}'),
    ("frac14", '\u{00BC}'),
    ("euro", '\u{20AC}'),
    ("pound", '\u{00A3}'),
    ("yen", '\u{00A5}'),
    ("cent", '\u{00A2}'),
    ("sect", '\u{00A7}'),
    ("para", '\u{00B6}'),
    ("dagger", '\u{2020}'),
    ("Dagger", '\u{2021}'),
    ("permil", '\u{2030}'),
    ("larr", '\u{2190}'),
    ("rarr", '\u{2192}'),
    ("harr", '\u{2194}'),
    ("darr", '\u{2193}'),
    ("uarr", '\u{2191}'),
    ("ne", '\u{2260}'),
    ("le", '\u{2264}'),
    ("ge", '\u{2265}'),
];

fn decode_entities(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        let Some(end) = rest.find(';').filter(|e| *e <= 9) else {
            out.push('&');
            rest = &rest[1..];
            continue;
        };
        let ent = &rest[1..end];
        let decoded = match ent {
            e if e.starts_with("#x") || e.starts_with("#X") => u32::from_str_radix(&e[2..], 16)
                .ok()
                .and_then(char::from_u32),
            e if e.starts_with('#') => e[1..].parse().ok().and_then(char::from_u32),
            e => NAMED_ENTITIES
                .iter()
                .find(|(name, _)| *name == e)
                .map(|(_, c)| *c),
        };
        match decoded {
            Some(c) => {
                out.push(c);
                rest = &rest[end + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}
