//! Small display helpers: relative dates and text snippets, the way a reader shows them.

/// "3m", "5h", "Tue", "12 Mar" — NetNewsWire's compact timeline stamp: recent items get a
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
