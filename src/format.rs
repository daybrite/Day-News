//! Small display helpers: relative dates and text snippets, the way a reader shows them.

/// "3m", "5h", "Tue", "12 Mar": NetNewsWire's compact timeline stamp. Recent items get a
/// relative age, older ones a date, so a glance tells you how fresh the list is.
pub fn relative_time(unix_secs: i64) -> String {
    // `daynews_db::now_unix` rather than SystemTime, which panics on wasm32.
    let now = daynews_db::now_unix();
    let age = now - unix_secs;
    match age {
        a if a < 0 => "now".into(),
        a if a < 60 => "now".into(),
        a if a < 3_600 => format!("{}m", a / 60),
        a if a < 86_400 => format!("{}h", a / 3_600),
        a if a < 7 * 86_400 => format!("{}d", a / 86_400),
        _ => civil_date(unix_secs),
    }
}

/// `DD Mon` for dates in this year, `DD Mon YYYY` otherwise.
pub fn civil_date(unix_secs: i64) -> String {
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let (y, m, d) = civil_from_days(unix_secs.div_euclid(86_400));
    let now_y = civil_from_days(daynews_db::now_unix().div_euclid(86_400)).0;
    let mon = MONTHS[(m as usize).clamp(1, 12) - 1];
    if y == now_y {
        format!("{d} {mon}")
    } else {
        format!("{d} {mon} {y}")
    }
}

/// A full timestamp for the article header.
pub fn full_date(unix_secs: i64) -> String {
    let secs_of_day = unix_secs.rem_euclid(86_400);
    format!(
        "{} at {:02}:{:02}",
        civil_date(unix_secs),
        secs_of_day / 3600,
        (secs_of_day % 3600) / 60
    )
}

/// Howard Hinnant's days→civil algorithm (proleptic Gregorian), days since 1970-01-01.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// The article's text as paragraphs, for the reader on a backend with no web engine to render
/// its HTML (macos-gtk; see `reader::article_text`). Block-level tags break paragraphs, inline
/// markup drops out, `script`/`style` bodies are skipped, and the entities feed HTML uses are
/// decoded. Text inside `h1`–`h6` comes back marked as a heading, so the reader can
/// set it apart instead of showing it as one more paragraph.
pub fn blocks(html: &str) -> Vec<Block> {
    const BLOCKS: [&str; 14] = [
        "p",
        "div",
        "br",
        "li",
        "tr",
        "h1",
        "h2",
        "h3",
        "h4",
        "h5",
        "h6",
        "blockquote",
        "pre",
        "figcaption",
    ];
    fn flush(cur: &mut String, heading: bool, out: &mut Vec<Block>) {
        let text = decode_entities(cur.trim());
        if !text.is_empty() {
            out.push(Block { text, heading });
        }
        cur.clear();
    }

    let mut out = Vec::new();
    let mut cur = String::new();
    let mut tag = String::new();
    let mut in_tag = false;
    // Whether the text being gathered sits inside an `h1`–`h6`.
    let mut heading = false;
    // `script`/`style` text is markup's, not the reader's.
    let mut skipping = 0usize;
    for c in html.chars() {
        match c {
            '<' => {
                in_tag = true;
                tag.clear();
            }
            '>' if in_tag => {
                in_tag = false;
                let closing = tag.starts_with('/');
                let name: String = tag
                    .trim_start_matches('/')
                    .chars()
                    .take_while(char::is_ascii_alphanumeric)
                    .flat_map(char::to_lowercase)
                    .collect();
                match name.as_str() {
                    "script" | "style" => {
                        skipping = if closing {
                            skipping.saturating_sub(1)
                        } else {
                            skipping + 1
                        }
                    }
                    n if BLOCKS.contains(&n) => {
                        flush(&mut cur, heading, &mut out);
                        // `h1`–`h6` are the only two-letter `h` names in BLOCKS.
                        heading = !closing && n.len() == 2 && n.starts_with('h');
                    }
                    _ => {}
                }
            }
            _ if in_tag => tag.push(c),
            _ if skipping > 0 => {}
            c if c.is_whitespace() => {
                if !cur.is_empty() && !cur.ends_with(' ') {
                    cur.push(' ');
                }
            }
            c => cur.push(c),
        }
    }
    flush(&mut cur, heading, &mut out);
    out
}

/// One block of an article's text, as [`blocks`] splits it.
#[derive(Debug, Clone, PartialEq)]
pub struct Block {
    pub text: String,
    /// Set for text inside `h1`–`h6`.
    pub heading: bool,
}

/// The entities feed HTML uses, named and numeric. Anything else stays as written:
/// showing `&frac34;` beats swallowing the text around it.
fn decode_entities(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        let tail = &rest[i..];
        // A real entity is short; an unescaped `&` in prose is not the start of one.
        match tail.find(';').filter(|end| *end <= 10) {
            Some(end) => {
                let name = &tail[1..end];
                let decoded = match name {
                    "amp" => Some('&'),
                    "lt" => Some('<'),
                    "gt" => Some('>'),
                    "quot" => Some('"'),
                    "apos" => Some('\''),
                    "nbsp" => Some(' '),
                    "hellip" => Some('…'),
                    "mdash" => Some('—'),
                    "ndash" => Some('–'),
                    "lsquo" => Some('\u{2018}'),
                    "rsquo" => Some('\u{2019}'),
                    "ldquo" => Some('\u{201c}'),
                    "rdquo" => Some('\u{201d}'),
                    n => n
                        .strip_prefix("#x")
                        .or_else(|| n.strip_prefix("#X"))
                        .and_then(|h| u32::from_str_radix(h, 16).ok())
                        .or_else(|| n.strip_prefix('#').and_then(|d| d.parse::<u32>().ok()))
                        .and_then(char::from_u32),
                };
                match decoded {
                    Some(c) => out.push(c),
                    None => out.push_str(&tail[..=end]),
                }
                rest = &tail[end + 1..];
            }
            None => {
                out.push('&');
                rest = &tail[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// One line of plain text for the timeline's preview, from a summary that may be HTML.
pub fn snippet(s: &str, max: usize) -> String {
    let mut out = String::with_capacity(max);
    let mut in_tag = false;
    let mut last_space = true;
    for c in s.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if in_tag => {}
            c if c.is_whitespace() => {
                if !last_space {
                    out.push(' ');
                    last_space = true;
                }
            }
            c => {
                out.push(c);
                last_space = false;
            }
        }
        if out.chars().count() >= max {
            out.push('…');
            break;
        }
    }
    out.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The blocks' text alone, for the cases that are not about headings.
    fn texts(html: &str) -> Vec<String> {
        blocks(html).into_iter().map(|b| b.text).collect()
    }

    #[test]
    fn blocks_break_on_block_tags_and_drop_inline_markup() {
        let html = "<p>First <b>bold</b> line.</p><p>Second line.</p>";
        assert_eq!(texts(html), ["First bold line.", "Second line."]);
    }

    #[test]
    fn blocks_skip_script_and_style_bodies() {
        let html = "<style>p { color: red }</style><p>Visible.</p><script>var x = 1;</script>";
        assert_eq!(texts(html), ["Visible."]);
    }

    #[test]
    fn blocks_decode_the_entities_feeds_use() {
        // `&frac34;` is not in the table, and text around an unknown entity must survive it.
        let html = "<p>Tom &amp; Jerry &mdash; &#8220;quoted&#8221; &#x2019;s &frac34;</p>";
        assert_eq!(
            texts(html),
            ["Tom & Jerry \u{2014} \u{201c}quoted\u{201d} \u{2019}s &frac34;"]
        );
    }

    #[test]
    fn blocks_collapse_whitespace_and_drop_empty_blocks() {
        let html = "<div>\n  spaced   out\n</div><p></p><p>  </p><li>Item</li>";
        assert_eq!(texts(html), ["spaced out", "Item"]);
    }

    #[test]
    fn bare_text_is_one_block() {
        assert_eq!(texts("Just text"), ["Just text"]);
        assert!(blocks("   ").is_empty());
        assert!(blocks("").is_empty());
    }

    #[test]
    fn heading_text_is_marked_and_what_follows_is_not() {
        let html = "<p>Intro.</p><h3>A <em>compass</em> that keeps time</h3><p>Body.</p><hr>After";
        let heading = |text: &str, heading| Block {
            text: text.into(),
            heading,
        };
        assert_eq!(
            blocks(html),
            [
                heading("Intro.", false),
                heading("A compass that keeps time", true),
                heading("Body.", false),
                heading("After", false),
            ]
        );
    }

    #[test]
    fn an_unterminated_ampersand_is_left_alone() {
        assert_eq!(decode_entities("Fish & chips"), "Fish & chips");
    }
}
