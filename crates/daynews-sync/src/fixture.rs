//! Synthetic fixtures implementing Feedbin's observed wire format, installed only by tests.
//! Based on the trial round trip on 2026-10-18: 200 auth, subscription 201, paged entries,
//! unread ID array and per-ID 200 acknowledgements. All content and credentials are synthetic.
use day_part_http::{
    Client, Request,
    simulation::{Reply, Simulation},
    transport::PreparedBody,
};
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    sync::{Arc, Mutex},
};

#[derive(Clone)]
pub struct Server {
    pub simulation: Simulation,
    state: Arc<Mutex<State>>,
}
struct State {
    subscriptions: Vec<Value>,
    unread: BTreeSet<u64>,
    reject: BTreeSet<u64>,
    next_id: u64,
}
impl Default for Server {
    fn default() -> Self {
        Self::new()
    }
}
impl Server {
    pub fn new() -> Self {
        let simulation = Simulation::new();
        let state = Arc::new(Mutex::new(State {
            subscriptions: vec![subscription(
                101,
                11,
                "https://fixture.invalid/feed.xml",
                "Sync fixture",
            )],
            unread: [1001, 1002].into_iter().collect(),
            reject: BTreeSet::new(),
            next_id: 102,
        }));
        let captured = state.clone();
        let auth = Request::get("https://api.feedbin.com/v2/authentication.json")
            .basic_auth("fixture@example.invalid", "fixture-password")
            .headers()[0]
            .1
            .clone();
        simulation.handle(
            |_, u| u.starts_with("https://api.feedbin.com/v2/"),
            move |request| {
                if !request
                    .headers
                    .iter()
                    .any(|(k, v)| k.eq_ignore_ascii_case("authorization") && *v == auth)
                {
                    return Ok(Reply::new(401, vec![]));
                }
                let url = url::Url::parse(&request.url).expect("fixture URL");
                let path = url.path();
                let method = request.method.as_str();
                let body = match &request.body {
                    PreparedBody::Bytes(b) => {
                        serde_json::from_slice::<Value>(b).unwrap_or(Value::Null)
                    }
                    _ => Value::Null,
                };
                let mut s = captured.lock().unwrap();
                let (status, value) = match (method, path) {
                    ("GET", "/v2/authentication.json") => return Ok(Reply::new(200, vec![])),
                    ("GET", "/v2/subscriptions.json") => {
                        (200, Value::Array(s.subscriptions.clone()))
                    }
                    ("POST", "/v2/subscriptions.json") => {
                        let Some(url) = body["feed_url"].as_str() else {
                            return Ok(Reply::new(422, vec![]));
                        };
                        if let Some(old) = s.subscriptions.iter().find(|r| r["feed_url"] == url) {
                            (302, old.clone())
                        } else {
                            let id = s.next_id;
                            s.next_id += 1;
                            let sub = subscription(id, id + 100, url, "Added fixture");
                            s.subscriptions.push(sub.clone());
                            (201, sub)
                        }
                    }
                    ("GET", "/v2/unread_entries.json") => (200, json!(s.unread)),
                    ("POST", "/v2/unread_entries.json" | "/v2/unread_entries/delete.json") => {
                        let ids: Vec<u64> = body["unread_entries"]
                            .as_array()
                            .map(|a| a.iter().filter_map(Value::as_u64).collect())
                            .unwrap_or_default();
                        if ids.len() > 1000 {
                            return Ok(Reply::new(422, vec![]));
                        }
                        let mut accepted = vec![];
                        for id in ids {
                            if s.reject.contains(&id) {
                                continue;
                            }
                            if path.ends_with("delete.json") {
                                s.unread.remove(&id);
                            } else {
                                s.unread.insert(id);
                            }
                            accepted.push(id);
                        }
                        (200, json!(accepted))
                    }
                    ("GET", "/v2/entries.json") => {
                        let query: std::collections::HashMap<_, _> = url.query_pairs().collect();
                        let ids = query.get("ids").map(|v| {
                            v.split(',')
                                .filter_map(|i| i.parse::<u64>().ok())
                                .collect::<Vec<_>>()
                        });
                        let page = query.get("page").map(|v| v.as_ref()).unwrap_or("1");
                        if !query.contains_key("ids")
                            && page == "1"
                            && request.headers.iter().any(|(k, v)| {
                                k.eq_ignore_ascii_case("if-none-match")
                                    && v == "\"fixture-entries-v1\""
                            })
                        {
                            return Ok(Reply::new(304, vec![]));
                        }

                        let rows = if let Some(ids) = ids {
                            ids.into_iter()
                                .filter(|i| [1001, 1002].contains(i))
                                .map(entry)
                                .collect()
                        } else if page == "1" {
                            vec![entry(1001)]
                        } else {
                            vec![entry(1002)]
                        };
                        let mut reply = Reply::new(200, serde_json::to_vec(&rows).unwrap())
                            .header("Content-Type", "application/json")
                            .header("ETag", "\"fixture-entries-v1\"");
                        if !query.contains_key("ids") && page == "1" {
                            reply = reply.header(
                                "Link",
                                "<https://api.feedbin.com/v2/entries.json?page=2>; rel=\"next\", <https://api.feedbin.com/v2/entries.json?page=2>; rel=\"last\"",
                            );
                        }
                        return Ok(reply);
                    }
                    _ if path.starts_with("/v2/subscriptions/") => {
                        let id = path
                            .trim_start_matches("/v2/subscriptions/")
                            .split(['.', '/'])
                            .next()
                            .and_then(|s| s.parse::<u64>().ok());
                        if let Some(index) =
                            s.subscriptions.iter().position(|r| r["id"].as_u64() == id)
                        {
                            if method == "DELETE" {
                                s.subscriptions.remove(index);
                                return Ok(Reply::new(204, vec![]));
                            }
                            if method == "POST" {
                                s.subscriptions[index]["title"] = body["title"].clone();
                                (200, s.subscriptions[index].clone())
                            } else {
                                (404, Value::Null)
                            }
                        } else {
                            (403, Value::Null)
                        }
                    }
                    _ => (404, Value::Null),
                };
                Ok(Reply::new(status, serde_json::to_vec(&value).unwrap())
                    .header("Content-Type", "application/json"))
            },
        );
        Self { simulation, state }
    }
    pub fn client(&self) -> Client {
        Client::builder()
            .transport(self.simulation.clone())
            .redirects(day_part_http::Redirects::Never)
            .build()
    }
    pub fn provider(&self) -> crate::feedbin::Feedbin {
        crate::feedbin::Feedbin::with_client(
            crate::Credentials {
                username: "fixture@example.invalid".into(),
                password: "fixture-password".into(),
            },
            self.client(),
        )
    }
    pub fn unread(&self) -> BTreeSet<u64> {
        self.state.lock().unwrap().unread.clone()
    }
    pub fn set_read(&self, id: u64, read: bool) {
        let mut s = self.state.lock().unwrap();
        if read {
            s.unread.remove(&id);
        } else {
            s.unread.insert(id);
        }
    }
    pub fn reject(&self, id: u64) {
        self.state.lock().unwrap().reject.insert(id);
    }
}
fn subscription(id: u64, feed: u64, url: &str, title: &str) -> Value {
    json!({"id":id,"feed_id":feed,"feed_url":url,"title":title,"site_url":"https://fixture.invalid/","created_at":"2026-10-18T12:00:00.000000Z"})
}
fn entry(id: u64) -> Value {
    json!({"id":id,"feed_id":11,"title":format!("Synchronized article {id}"),"url":format!("https://fixture.invalid/articles/{id}"),"author":"Fixture Author","content":"<p>Synthetic synchronized content.</p>","summary":"Synthetic sync fixture","published":"2026-10-18T12:00:00.000000Z","created_at":"2026-10-18T12:00:00.000000Z"})
}
