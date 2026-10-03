//! Persisted, app-wide reader presentation. Controls write through the same bindings as menus.
use day::prelude::*;
use std::{cell::OnceCell, rc::Rc};

const SCALE_KEY: &str = "news.reader.scale";
const FONT_KEY: &str = "news.reader.font";
const BACKGROUND_KEY: &str = "news.reader.background";
const TEXT_KEY: &str = "news.reader.text";
pub const DEFAULT_SCALE: f64 = 140.0;
pub const MIN_SCALE: f64 = 70.0;
pub const MAX_SCALE: f64 = 250.0;

#[derive(Clone, Debug, PartialEq)]
pub struct ReaderStyle {
    pub scale: f64,
    pub family: String,
    pub background: Option<Color>,
    pub text: Option<Color>,
}

impl Default for ReaderStyle {
    fn default() -> Self {
        Self {
            scale: DEFAULT_SCALE,
            family: String::new(),
            background: None,
            text: None,
        }
    }
}

fn scale(value: f64) -> f64 {
    if value.is_finite() {
        ((value / 10.0).round() * 10.0).clamp(MIN_SCALE, MAX_SCALE)
    } else {
        DEFAULT_SCALE
    }
}

fn stored_color(key: &str) -> Option<Color> {
    day::prefs::get(key)
        .and_then(|v| Color::parse(&v))
        .map(|mut c| {
            c.a = 1.0;
            c
        })
}

thread_local! {
    static STYLE: OnceCell<Signal<ReaderStyle>> = const { OnceCell::new() };
    static APPEARANCE: OnceCell<Signal<usize>> = const { OnceCell::new() };
}

pub fn state() -> Signal<ReaderStyle> {
    STYLE.with(|slot| {
        *slot.get_or_init(|| {
            Signal::new_in(
                day::reactive::Scope::root(),
                ReaderStyle {
                    scale: scale(
                        day::prefs::get(SCALE_KEY)
                            .and_then(|s| s.parse().ok())
                            .unwrap_or(DEFAULT_SCALE),
                    ),
                    family: day::prefs::get(FONT_KEY)
                        .filter(|s| s.len() <= 512)
                        .unwrap_or_default(),
                    background: stored_color(BACKGROUND_KEY),
                    text: stored_color(TEXT_KEY),
                },
            )
        })
    })
}

pub fn appearance() -> Signal<usize> {
    APPEARANCE.with(|slot| {
        *slot.get_or_init(|| {
            Signal::new_in(
                day::reactive::Scope::root(),
                match day::prefs::get(super::settings::THEME_KEY).as_deref() {
                    Some("light") => 0,
                    Some("dark") => 1,
                    _ => 2,
                },
            )
        })
    })
}

fn change(edit: impl FnOnce(&mut ReaderStyle)) {
    let signal = state();
    let old = signal.get_untracked();
    let mut next = old.clone();
    edit(&mut next);
    if next == old {
        return;
    }
    if next.scale != old.scale {
        if next.scale == DEFAULT_SCALE {
            day::prefs::remove(SCALE_KEY);
        } else {
            day::prefs::set(SCALE_KEY, &next.scale.to_string());
        }
    }
    if next.family != old.family {
        if next.family.is_empty() {
            day::prefs::remove(FONT_KEY);
        } else {
            day::prefs::set(FONT_KEY, &next.family);
        }
    }
    for (key, value, previous) in [
        (BACKGROUND_KEY, next.background, old.background),
        (TEXT_KEY, next.text, old.text),
    ] {
        if value != previous {
            if let Some(color) = value {
                day::prefs::set(key, &css_color(color));
            } else {
                day::prefs::remove(key);
            }
        }
    }
    signal.set(next);
}

pub fn adjust_size(delta: f64) {
    Scale.write(state().get_untracked().scale + delta);
}

pub fn reset() {
    for key in [
        SCALE_KEY,
        FONT_KEY,
        BACKGROUND_KEY,
        TEXT_KEY,
        super::settings::THEME_KEY,
    ] {
        day::prefs::remove(key);
    }
    batch(|| {
        state().set(ReaderStyle::default());
        crate::settings::reset_list_styles();
        appearance().set(2);
        day::set_appearance(None);
    });
}

#[derive(Clone, Copy)]
pub struct Scale;
impl Binding<f64> for Scale {
    fn read(&self) -> f64 {
        state().get().scale
    }
    fn peek(&self) -> f64 {
        state().get_untracked().scale
    }
    fn write(&self, value: f64) {
        change(|s| s.scale = scale(value));
    }
}

#[derive(Clone, Copy)]
pub struct Appearance;
impl Binding<usize> for Appearance {
    fn read(&self) -> usize {
        appearance().get()
    }
    fn peek(&self) -> usize {
        appearance().get_untracked()
    }
    fn write(&self, index: usize) {
        let index = index.min(2);
        match index {
            0 => day::prefs::set(super::settings::THEME_KEY, "light"),
            1 => day::prefs::set(super::settings::THEME_KEY, "dark"),
            _ => day::prefs::remove(super::settings::THEME_KEY),
        };
        appearance().set(index);
        day::set_appearance(match index {
            0 => Some(false),
            1 => Some(true),
            _ => None,
        });
    }
}

#[derive(Clone, Copy)]
pub enum ReaderColor {
    Background,
    Text,
}
impl Binding<Color> for ReaderColor {
    fn read(&self) -> Color {
        let style = state().get();
        let palette = crate::theme::palette();
        match self {
            Self::Background => style.background.unwrap_or(palette.bg),
            Self::Text => style.text.unwrap_or(palette.text),
        }
    }
    fn peek(&self) -> Color {
        day::reactive::untrack(|| self.read())
    }
    fn write(&self, mut color: Color) {
        color.a = 1.0;
        change(|s| match self {
            Self::Background => s.background = Some(color),
            Self::Text => s.text = Some(color),
        });
    }
}

#[derive(Clone)]
pub struct FontChoice(pub Rc<Vec<String>>);
impl Binding<usize> for FontChoice {
    fn read(&self) -> usize {
        let family = state().get().family;
        self.0.iter().position(|f| *f == family).unwrap_or(0)
    }
    fn peek(&self) -> usize {
        day::reactive::untrack(|| self.read())
    }
    fn write(&self, index: usize) {
        if let Some(family) = self.0.get(index) {
            change(|s| s.family = family.clone());
        }
    }
}

/// Repeated recommended entries are intentional: the lower section is the complete catalog.
pub fn font_choices(mut available: Vec<String>) -> (Vec<String>, usize) {
    available.sort_by_key(|s| s.to_lowercase());
    available.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
    let mut choices = vec![String::new()]; // platform default
    for recommended in [
        "Georgia",
        "Palatino",
        "Charter",
        "Iowan Old Style",
        "Helvetica Neue",
    ] {
        if let Some(family) = available
            .iter()
            .find(|f| f.eq_ignore_ascii_case(recommended))
        {
            choices.push(family.clone());
        }
    }
    let boundary = choices.len();
    choices.extend(available);
    (choices, boundary)
}

pub fn css_color(c: Color) -> String {
    format!(
        "#{:02x}{:02x}{:02x}",
        (c.r * 255.0).round() as u8,
        (c.g * 255.0).round() as u8,
        (c.b * 255.0).round() as u8
    )
}

fn css_family(family: &str) -> String {
    // CSS string escaping also prevents a stored/font-catalog name from ending a style tag.
    let mut escaped = String::new();
    for c in family.chars() {
        if c.is_ascii_alphanumeric() || c == ' ' || c == '-' || c == '_' {
            escaped.push(c);
        } else {
            escaped.push_str(&format!("\\{:x} ", c as u32));
        }
    }
    format!("\"{escaped}\", -apple-system, system-ui, sans-serif")
}

pub fn stylesheet(style: &ReaderStyle, dark: bool) -> String {
    let p = crate::theme::palette_for(dark);
    let background = style.background.unwrap_or(p.bg);
    let text = style.text.unwrap_or(p.text);
    let family = if style.family.is_empty() {
        "-apple-system, BlinkMacSystemFont, system-ui, sans-serif".into()
    } else {
        css_family(&style.family)
    };
    // Blend supporting colors into the chosen background so custom paper/ink stays coherent.
    let blend = |amount: f64| {
        Color::rgb(
            text.r * amount + background.r * (1.0 - amount),
            text.g * amount + background.g * (1.0 - amount),
            text.b * amount + background.b * (1.0 - amount),
        )
    };
    format!(
        ":root {{ color-scheme: {scheme}; }}\na, .src, #reader-load-full {{ color: {accent}; }}\nhtml, body {{ background: {bg}; color: {fg}; }}\nbody {{ font-family: {family}; font-size: calc(1rem * {factor}); }}\n.by, .when, blockquote {{ color: {muted}; }}\n.rule, hr, blockquote {{ border-color: {rule}; }}\n#reader-load-full {{ border-color: {rule}; background: linear-gradient(color-mix(in srgb, {alt} 80%, {bg}), {alt}); }} .reader-load-label {{ text-shadow: 0 1px 0 color-mix(in srgb, {bg} 75%, transparent); }}\npre, #reader-link-tooltip {{ background: {alt}; }} #reader-link-tooltip {{ color: {fg}; border-color: {rule}; }}",
        scheme = if dark { "dark" } else { "light" },
        accent = css_color(p.accent),
        bg = css_color(background),
        fg = css_color(text),
        factor = scale(style.scale) / 100.0,
        muted = css_color(if style.text.is_some() || style.background.is_some() {
            blend(0.65)
        } else {
            p.text_muted
        }),
        rule = css_color(if style.text.is_some() || style.background.is_some() {
            blend(0.18)
        } else {
            p.rule
        }),
        alt = css_color(if style.text.is_some() || style.background.is_some() {
            blend(0.06)
        } else {
            p.bg_alt
        })
    )
}

pub fn apply_to(js: day_piece_webview::JsHandle) {
    let css = stylesheet(&state().get_untracked(), day::dark_mode());
    let encoded = serde_json::to_string(&css).expect("CSS string serializes");
    day::task(async move {
        // Replace a dedicated style node, leaving document state, selection, and scroll intact.
        let _ = js.eval(format!("(() => {{ const style = document.getElementById('reader-display'); if (style) style.textContent = {encoded}; }})()" )).await;
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn scale_is_bounded_and_invalid_persistence_is_safe() {
        assert_eq!(scale(f64::NAN), DEFAULT_SCALE);
        assert_eq!(scale(f64::INFINITY), DEFAULT_SCALE);
        assert_eq!(scale(0.0), MIN_SCALE);
        assert_eq!(scale(999.0), MAX_SCALE);
        assert_eq!(scale(117.0), 120.0);
    }
    #[test]
    fn recommendations_precede_the_complete_sorted_catalog() {
        let (fonts, separator) = font_choices(vec![
            "Zulu Fixture".into(),
            "Georgia".into(),
            "Alpha Fixture".into(),
            "Georgia".into(),
        ]);
        assert_eq!(&fonts[..separator], &["", "Georgia"]);
        assert_eq!(
            &fonts[separator..],
            &["Alpha Fixture", "Georgia", "Zulu Fixture"]
        );
    }
    #[test]
    fn styles_escape_font_names_and_restore_automatic_colors() {
        let s = ReaderStyle {
            family: "Fixture\";</style><script>".into(),
            scale: 150.0,
            background: Some(Color::hex(0xffeedd)),
            text: Some(Color::hex(0x112233)),
        };
        let css = stylesheet(&s, false);
        assert!(!css.contains("</style>"));
        assert!(css.contains("#ffeedd"));
        assert!(css.contains("#112233"));
        assert!(css.contains("1rem * 1.5"));
        assert!(
            stylesheet(&ReaderStyle::default(), true)
                .contains(&css_color(crate::theme::palette_for(true).bg))
        );
    }
}
