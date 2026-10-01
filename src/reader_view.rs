//! Per-window Reader View state. RSS content stays intact; extracted content is a temporary
//! overlay for the selected article. Selection changes and window disposal cancel extraction.

use crate::extraction::{ArticleExtractor, ExtractedArticle, ExtractionError, LocalReadability};
use day::prelude::*;
use day_piece_webview::JsHandle;
use std::{cell::Cell, rc::Rc};

#[derive(Clone, Copy)]
pub struct ReaderView {
    pub engine: JsHandle,
    pub ready: Signal<bool>,
    pub active: Signal<bool>,
    pub loading: Signal<bool>,
    pub error: Signal<Option<ExtractionError>>,
    pub extracted: Signal<Option<(u64, ExtractedArticle)>>,
    task: Signal<Rc<Cell<Option<day::TaskHandle>>>>,
}

impl Ambient for ReaderView {
    fn create() -> Self {
        let pending = Rc::new(Cell::new(None::<day::TaskHandle>));
        let state = Self {
            engine: JsHandle::new(),
            ready: Signal::new(false),
            active: Signal::new(false),
            loading: Signal::new(false),
            error: Signal::new(None),
            extracted: Signal::new(None),
            task: Signal::new(pending.clone()),
        };
        let scene = daynews_core::scene();
        watch(
            move || {
                scene
                    .article
                    .with(|a| a.as_ref().map(|a| (a.id, a.url.clone())))
            },
            move |_, _| {
                state.cancel();
                state.active.set(false);
                state.error.set(None);
                state.extracted.set(None);
                state.ready.set(false);
            },
        );
        // Signals are already disposed when scope cleanup runs. Keep only the cancellation
        // cell alive here; never read or write window signals during disposal.
        day::reactive::Scope::current().on_cleanup(move || {
            if let Some(task) = pending.take() {
                task.abort();
            }
        });
        state
    }
}

impl ReaderView {
    pub fn current() -> Option<Self> {
        Self::try_ambient().or_else(Self::focused)
    }

    fn cancel(self) {
        if let Some(task) = self.task.get_untracked().take() {
            task.abort();
        }
        self.loading.set(false);
    }

    pub fn toggle(self, scene: daynews_core::NewsScene) {
        if self.loading.get_untracked() {
            self.cancel();
            return;
        }
        if self.active.get_untracked() {
            self.active.set(false);
            return;
        }
        let Some(article) = scene.article.get_untracked() else {
            return;
        };
        if self
            .extracted
            .with_untracked(|a| a.as_ref().is_some_and(|(id, _)| *id == article.id))
        {
            self.active.set(true);
            return;
        }
        let Some(url) = article
            .url
            .filter(|url| crate::extraction::web_url(url).is_some())
        else {
            return;
        };
        self.error.set(None);
        self.loading.set(true);
        let task = day::task(async move {
            let provider = LocalReadability {
                engine: self.engine,
            };
            let result = match futures_util::future::select(
                provider.extract(&url),
                Box::pin(day::sleep(30_000)),
            )
            .await
            {
                futures_util::future::Either::Left((result, _)) => result,
                futures_util::future::Either::Right(_) => Err(ExtractionError::Timeout),
            };
            // A delayed response must never replace another article or another window's body.
            if !scene.article.with_untracked(|a| {
                a.as_ref()
                    .is_some_and(|a| a.id == article.id && a.url.as_deref() == Some(&url))
            }) {
                return;
            }
            day::reactive::batch(|| {
                self.loading.set(false);
                match result {
                    Ok(content) => {
                        self.extracted.set(Some((article.id, content)));
                        self.active.set(true);
                    }
                    Err(error) => self.error.set(Some(error)),
                }
            });
        });
        self.task.get_untracked().set(Some(task));
    }

    pub fn article(
        self,
        original: Option<daynews_core::StoredArticle>,
    ) -> Option<daynews_core::StoredArticle> {
        let mut article = original?;
        if self.active.get()
            && let Some((id, extracted)) = self.extracted.get()
            && id == article.id
        {
            article.content_html = Some(extracted.content);
            article.url = Some(extracted.url);
            if extracted
                .title
                .as_ref()
                .is_some_and(|s| !s.trim().is_empty())
            {
                article.title = extracted.title;
            }
            if extracted
                .byline
                .as_ref()
                .is_some_and(|s| !s.trim().is_empty())
            {
                article.author = extracted.byline;
            }
        }
        Some(article)
    }

    pub fn status(self) -> String {
        use crate::res;
        if self.loading.get() {
            res::str::reader_view_loading().format()
        } else if let Some(error) = self.error.get() {
            match error {
                ExtractionError::Network => res::str::reader_view_network_error().format(),
                ExtractionError::TooLarge => res::str::reader_view_too_large().format(),
                ExtractionError::NoContent => res::str::reader_view_no_content().format(),
                ExtractionError::Timeout => res::str::reader_view_timeout().format(),
                _ => res::str::reader_view_unavailable().format(),
            }
        } else {
            String::new()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(id: u64) -> daynews_core::StoredArticle {
        daynews_core::StoredArticle {
            id,
            feed_id: 1,
            feed_title: "Fixture publication".into(),
            title: Some("RSS title".into()),
            url: Some(format!("https://fixture.example/{id}")),
            author: None,
            published_at: 0,
            summary: None,
            content_html: Some("<p>RSS excerpt</p>".into()),
            is_read: true,
            is_starred: false,
        }
    }

    fn scene() -> (daynews_core::NewsScene, ReaderView) {
        let scene = daynews_core::NewsScene::create();
        day::reactive::Scope::current().provide(scene);
        let view = ReaderView::create();
        scene.article.set(Some(fixture(1)));
        day::reactive::flush_sync();
        (scene, view)
    }

    #[test]
    fn overlay_toggles_without_mutating_the_feed_and_clears_on_selection() {
        let scope = day::reactive::Scope::child();
        scope.enter(|| {
            let (scene, view) = scene();
            view.extracted.set(Some((
                1,
                ExtractedArticle {
                    content: "<p>Full fixture article</p>".into(),
                    title: Some("Full title".into()),
                    byline: Some("Fixture author".into()),
                    url: "https://fixture.example/final".into(),
                },
            )));
            view.toggle(scene);
            assert_eq!(
                view.article(scene.article.get()).unwrap().title.as_deref(),
                Some("Full title")
            );
            assert_eq!(
                scene.article.get().unwrap().title.as_deref(),
                Some("RSS title")
            );
            view.toggle(scene);
            assert_eq!(
                view.article(scene.article.get())
                    .unwrap()
                    .content_html
                    .as_deref(),
                Some("<p>RSS excerpt</p>")
            );
            view.toggle(scene);
            scene.article.set(Some(fixture(2)));
            day::reactive::flush_sync();
            assert!(!view.active.get());
            assert!(view.extracted.get().is_none());
        });
        scope.dispose();
    }

    #[test]
    fn window_disposal_and_selection_cancel_pending_work() {
        struct Pending(Rc<Cell<usize>>);
        impl Drop for Pending {
            fn drop(&mut self) {
                self.0.set(self.0.get() + 1);
            }
        }
        let dropped = Rc::new(Cell::new(0));
        let scope = day::reactive::Scope::child();
        scope.enter(|| {
            let (scene, view) = scene();
            for iteration in 0..2 {
                let pending = Pending(dropped.clone());
                let task = day::task(async move {
                    let _pending = pending;
                    std::future::pending::<()>().await;
                });
                view.task.get_untracked().set(Some(task));
                view.loading.set(true);
                if iteration == 0 {
                    scene.article.set(Some(fixture(2)));
                    day::reactive::flush_sync();
                    assert!(task.is_finished());
                    assert!(!view.loading.get());
                    assert_eq!(dropped.get(), 1);
                }
            }
        });
        scope.dispose();
        assert_eq!(
            dropped.get(),
            2,
            "cleanup cancels without accessing disposed signals"
        );
    }
}
