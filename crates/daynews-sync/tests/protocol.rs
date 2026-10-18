use day_part_http::{
    Client,
    simulation::{Reply, Simulation},
};
use daynews_sync::{feedbin::Feedbin, fixture::Server, *};
use std::{
    future::Future,
    sync::Arc,
    task::{Context, Poll, Wake, Waker},
};
struct Thread(std::thread::Thread);
impl Wake for Thread {
    fn wake(self: Arc<Self>) {
        self.0.unpark();
    }
}
fn wait<T>(f: impl Future<Output = T>) -> T {
    let w = Waker::from(Arc::new(Thread(std::thread::current())));
    let mut f = std::pin::pin!(f);
    loop {
        match f.as_mut().poll(&mut Context::from_waker(&w)) {
            Poll::Ready(v) => return v,
            Poll::Pending => std::thread::park(),
        }
    }
}
fn provider(sim: Simulation) -> Feedbin {
    Feedbin::with_client(
        Credentials {
            username: "synthetic".into(),
            password: "synthetic".into(),
        },
        Client::builder()
            .transport(sim)
            .redirects(day_part_http::Redirects::Never)
            .build(),
    )
}
#[test]
fn unauthorized_credentials_and_rate_limit_are_classified_without_response_details() {
    let s = Server::new();
    assert_eq!(
        wait(provider(s.simulation).authenticate()),
        Err(Error::Authentication)
    );
    let sim = Simulation::new();
    sim.handle(
        |_, _| true,
        |_| Ok(Reply::new(429, b"private response content".to_vec()).header("Retry-After", "123")),
    );
    let error = wait(provider(sim).authenticate()).unwrap_err();
    assert_eq!(error, Error::Retry { after_seconds: 123 });
    assert!(!format!("{error}").contains("private"));
}
#[test]
fn redirect_does_not_forward_credentials_to_another_origin() {
    let sim = Simulation::new();
    sim.handle(
        |_, u| u.starts_with("https://api.feedbin.com/"),
        |_| Ok(Reply::new(302, vec![]).header("Location", "https://other.invalid/")),
    );
    assert_eq!(
        wait(provider(sim.clone()).authenticate()),
        Err(Error::Authentication)
    );
    assert_eq!(sim.requests().len(), 1);
}
#[test]
fn malformed_unread_snapshot_is_never_interpreted_as_everything_read() {
    let sim = Simulation::new();
    sim.handle(|_, _| true, |_| Ok(Reply::new(200, b"[1,".to_vec())));
    assert!(matches!(
        wait(provider(sim).read_state()),
        Err(Error::InvalidResponse)
    ));
}
#[test]
fn bulk_read_upload_is_batched_and_acknowledged_per_id() {
    let s = Server::new();
    let p = s.provider();
    let operations = (1..=2001)
        .map(|id| Operation {
            revision: id,
            mutation: Mutation::SetRead(ReadState {
                id: id.to_string(),
                read: true,
            }),
        })
        .collect();
    let outcomes = wait(p.apply(operations)).unwrap();
    assert_eq!(outcomes.len(), 2001);
    assert!(
        outcomes
            .iter()
            .all(|o| matches!(o, Outcome::Applied { .. }))
    );
    assert_eq!(
        s.simulation.request_count(
            "POST",
            "https://api.feedbin.com/v2/unread_entries/delete.json"
        ),
        3
    );
}
#[test]
fn cursor_is_not_a_url_or_an_injection_surface() {
    let s = Server::new();
    let p = s.provider();
    assert!(matches!(
        wait(p.entries(Some("https://other.invalid".into()))),
        Err(Error::InvalidResponse)
    ));
    assert!(s.simulation.requests().is_empty());
}

#[test]
fn state_requests_bypass_platform_caches_after_a_mutation() {
    let sim = Simulation::new();
    sim.handle(
        |_, _| true,
        |request| {
            assert!(matches!(request.cache, day_part_http::CachePolicy::Reload));
            assert!(
                request
                    .headers
                    .iter()
                    .any(|(k, v)| k.eq_ignore_ascii_case("cache-control") && v == "no-cache")
            );
            Ok(Reply::new(200, b"[]".to_vec()))
        },
    );
    assert!(matches!(wait(provider(sim).subscriptions()),Ok(Changes::Snapshot(v)) if v.is_empty()));
}

#[test]
fn article_progress_uses_last_page_metadata_without_following_foreign_urls() {
    let sim = Simulation::new();
    sim.handle(|_, _| true, |_| Ok(Reply::new(200, b"[]".to_vec())
        .header("Link", "<https://other.invalid/?page=102>; rel=\"last\", <https://other.invalid/?page=2>; rel=\"next\"")));
    let page = wait(provider(sim.clone()).entries(None)).unwrap();
    assert_eq!(page.total_pages, Some(102));
    assert_eq!(page.next.as_deref(), Some("2"));
    assert_eq!(sim.requests().len(), 1);
    assert!(
        sim.requests()[0]
            .url
            .starts_with("https://api.feedbin.com/")
    );
}

#[test]
fn article_progress_stays_indeterminate_without_a_valid_last_page() {
    let sim = Simulation::new();
    sim.handle(|_, _| true, |_| Ok(Reply::new(200, b"[]".to_vec())
        .header("Link", "<https://api.feedbin.com/v2/entries.json?page=oops>; rel=\"last\", <https://api.feedbin.com/v2/entries.json?page=2>; rel=\"next\"")));
    assert_eq!(wait(provider(sim).entries(None)).unwrap().total_pages, None);
}

#[test]
fn aborting_during_a_delayed_http_response_does_not_wait_for_the_network_timeout() {
    use day_part_http::simulation::{Conditions, ManualClock};
    use futures_util::future::{AbortHandle, Abortable};
    let clock = ManualClock::default();
    let sim = Simulation::with_clock(Arc::new(clock.clone()));
    sim.set_conditions(Conditions {
        latency: std::time::Duration::from_secs(30),
        ..Default::default()
    });
    sim.handle(|_, _| true, |_| Ok(Reply::new(200, vec![])));
    let p = provider(sim.clone());
    let (abort, registration) = AbortHandle::new_pair();
    let mut future = Box::pin(Abortable::new(p.authenticate(), registration));
    let waker = Waker::from(Arc::new(Thread(std::thread::current())));
    assert!(
        future
            .as_mut()
            .poll(&mut Context::from_waker(&waker))
            .is_pending()
    );
    abort.abort();
    assert!(matches!(
        future.as_mut().poll(&mut Context::from_waker(&waker)),
        Poll::Ready(Err(_))
    ));
    drop(future);
    clock.advance(std::time::Duration::from_secs(60));
    assert_eq!(sim.requests().len(), 1);
}
