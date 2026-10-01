//! Replaceable full-article extraction. Providers return publication data, never UI strings.
//! The default fetches HTML with native networking and runs bundled Mozilla Readability in
//! an inert DOM in the reader's JS engine. It never navigates to or runs the publisher page.

use std::{future::Future, pin::Pin, time::Duration};

use day::prelude::*;
use day_piece_webview::JsHandle;
use serde::Deserialize;

const MAX_HTML_BYTES: usize = 5 * 1024 * 1024;

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct ExtractedArticle {
    pub content: String,
    pub title: Option<String>,
    pub byline: Option<String>,
    pub url: String,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ExtractionError {
    InvalidUrl,
    Network,
    TooLarge,
    NoContent,
    Unavailable,
    Timeout,
}

pub type ExtractionFuture<'a> =
    Pin<Box<dyn Future<Output = Result<ExtractedArticle, ExtractionError>> + 'a>>;

/// An HTTP service or a local/offscreen engine can implement this without changing commands,
/// window state, rendering, or cancellation. Providers must return sanitized, inert HTML.
/// Dropping the future abandons results and cancels work supported by the underlying engine.
pub trait ArticleExtractor {
    fn extract<'a>(&'a self, url: &'a str) -> ExtractionFuture<'a>;
}

pub struct LocalReadability {
    pub engine: JsHandle,
}

pub fn web_url(value: &str) -> Option<url::Url> {
    url::Url::parse(value).ok().filter(|url| {
        matches!(url.scheme(), "http" | "https")
            && url.host_str().is_some()
            && url.username().is_empty()
            && url.password().is_none()
    })
}

impl ArticleExtractor for LocalReadability {
    fn extract<'a>(&'a self, url: &'a str) -> ExtractionFuture<'a> {
        Box::pin(async move {
            let url = web_url(url).ok_or(ExtractionError::InvalidUrl)?;
            let response = day_part_http::fetch_future(
                day_part_http::Request::get(url.as_str())
                    .header("Accept", "text/html, application/xhtml+xml;q=0.9")
                    .header("User-Agent", concat!("DayNews/", env!("CARGO_PKG_VERSION")))
                    .timeout(Duration::from_secs(25)),
            )
            .await
            .map_err(|_| ExtractionError::Network)?;
            let (html, base) = decode_page(response, url.as_str())?;
            let script = extraction_script(&html, &base)?;
            let reply = self
                .engine
                .eval(script)
                .await
                .map_err(|_| ExtractionError::Unavailable)?;
            let article: Option<ExtractedArticle> =
                serde_json::from_str(&reply).map_err(|_| ExtractionError::NoContent)?;
            article
                .filter(|a| !a.content.trim().is_empty())
                .ok_or(ExtractionError::NoContent)
        })
    }
}

fn decode_page(
    response: day_part_http::Response,
    original_url: &str,
) -> Result<(String, String), ExtractionError> {
    if !(200..300).contains(&response.status) {
        return Err(ExtractionError::Network);
    }
    if response.body.len() > MAX_HTML_BYTES {
        return Err(ExtractionError::TooLarge);
    }
    let content_type = response
        .headers
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case("content-type"))
        .map(|(_, value)| value.as_str())
        .unwrap_or("text/html");
    let media_type = content_type.split(';').next().unwrap_or_default().trim();
    if !media_type.eq_ignore_ascii_case("text/html")
        && !media_type.eq_ignore_ascii_case("application/xhtml+xml")
    {
        return Err(ExtractionError::NoContent);
    }
    let charset = content_type.split(';').skip(1).find_map(|part| {
        let (key, value) = part.trim().split_once('=')?;
        key.eq_ignore_ascii_case("charset")
            .then(|| value.trim().trim_matches(['\'', '"']))
    });
    let encoding = charset
        .and_then(|name| encoding_rs::Encoding::for_label(name.as_bytes()))
        .unwrap_or(encoding_rs::UTF_8);
    let (html, _, _) = encoding.decode(&response.body);
    let base = if response.url.is_empty() {
        original_url
    } else {
        &response.url
    };
    let base = web_url(base).ok_or(ExtractionError::InvalidUrl)?;
    Ok((html.into_owned(), base.into()))
}

fn script_asset(name: day::AssetName) -> Result<String, ExtractionError> {
    let asset = resource(name).ok_or(ExtractionError::Unavailable)?;
    String::from_utf8(asset.as_slice().to_vec()).map_err(|_| ExtractionError::Unavailable)
}

fn extraction_script(html: &str, url: &str) -> Result<String, ExtractionError> {
    let readability = script_asset(crate::res::assets::reader::readability_js)?;
    let purify = script_asset(crate::res::assets::reader::purify_js)?;
    let extract = script_asset(crate::res::assets::reader::extract_js)?;
    // JSON quoting keeps publication text inert, including quotes and closing script tags.
    let input = serde_json::json!({ "html": html, "url": url });
    Ok(format!(
        "(() => {{ {readability}\n{purify}\n{extract}\nreturn extractArticle({input}); }})()"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_only_web_article_urls_without_embedded_credentials() {
        for url in [
            "file:///etc/passwd",
            "javascript:alert(1)",
            "https://u:p@example.org/",
        ] {
            assert!(web_url(url).is_none());
        }
        assert!(web_url("https://example.org/article?q=1#part").is_some());
    }

    #[test]
    fn decodes_publication_encoding_and_preserves_redirect_base() {
        let mut response = day_part_http::Response::new(
            200,
            vec![(
                "Content-Type".into(),
                "text/html; charset=windows-1252".into(),
            )],
            b"<p>caf\xe9</p>".to_vec(),
        );
        response.url = "https://example.org/final/article".into();
        assert_eq!(
            decode_page(response, "https://example.org/").unwrap(),
            (
                "<p>café</p>".into(),
                "https://example.org/final/article".into()
            )
        );
    }

    #[test]
    fn rejects_error_pages_non_html_and_oversized_responses() {
        assert_eq!(
            decode_page(
                day_part_http::Response::new(403, vec![], vec![]),
                "https://example.org/"
            ),
            Err(ExtractionError::Network)
        );
        assert_eq!(
            decode_page(
                day_part_http::Response::new(
                    200,
                    vec![("content-type".into(), "application/pdf".into())],
                    vec![]
                ),
                "https://example.org/"
            ),
            Err(ExtractionError::NoContent)
        );
        assert_eq!(
            decode_page(
                day_part_http::Response::new(200, vec![], vec![0; MAX_HTML_BYTES + 1]),
                "https://example.org/"
            ),
            Err(ExtractionError::TooLarge)
        );
    }
}
