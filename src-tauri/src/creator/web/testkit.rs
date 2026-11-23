//! Test-only: a bare engine on a throwaway data folder, and the fixture pages
//! served in-process.
//!
//! Engine-backed tests run only when `CREATOR_TEST_BROWSER` points at a ShardX
//! binary (on the Mac: the sandboxed copy under `.spike-home`, never the owner's
//! installed app); without it they skip with a note, and `CREATOR_REQUIRE_BROWSER=1`
//! makes that a failure. The data folder is always a fresh temp dir.

use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;

use axum::http::header;
use axum::response::IntoResponse;
use axum::Router;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use super::{Page, Session};

fn engine() -> Option<PathBuf> {
    std::env::var_os("CREATOR_TEST_BROWSER").map(PathBuf::from)
}

/// The engine process and its temp data folder; dropped after the session.
struct Proc {
    child: Child,
    dir: PathBuf,
}

impl Drop for Proc {
    fn drop(&mut self) {
        // Our own process on a temp folder: nothing to save.
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

pub struct TestBrowser {
    pub session: Session,
    _proc: Proc,
}

impl TestBrowser {
    pub async fn launch() -> Option<TestBrowser> {
        let proc = Proc::spawn("about:blank")?;
        let ws = devtools_ws(&proc.dir).await;
        let session = Session::connect(&ws).await.expect("connect");
        Some(TestBrowser {
            session,
            _proc: proc,
        })
    }

    pub async fn page(&self) -> Page {
        self.session.page().await.expect("open a page")
    }
}

/// The bare engine with no client attached (the stealth probe connects its own).
pub struct Bare {
    pub ws: String,
    _proc: Proc,
}

impl Bare {
    pub async fn launch() -> Option<Bare> {
        let proc = Proc::spawn("about:blank")?;
        let ws = devtools_ws(&proc.dir).await;
        Some(Bare { ws, _proc: proc })
    }
}

impl Proc {
    fn spawn(url: &str) -> Option<Proc> {
        let Some(bin) = engine().filter(|p| p.exists()) else {
            if std::env::var("CREATOR_REQUIRE_BROWSER").as_deref() == Ok("1") {
                panic!("no engine: set CREATOR_TEST_BROWSER");
            }
            eprintln!("skipped: no engine (set CREATOR_TEST_BROWSER)");
            return None;
        };
        let dir = std::env::temp_dir().join(format!("creator-web-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let child = Command::new(bin)
            .arg(format!("--user-data-dir={}", dir.display()))
            .args([
                "--remote-debugging-port=0",
                "--shardx-automation",
                "--no-first-run",
                "--no-default-browser-check",
                "--window-position=-2400,-2400",
                "--window-size=1280,800",
                url,
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("start the engine");
        Some(Proc { child, dir })
    }
}

async fn devtools_ws(dir: &std::path::Path) -> String {
    let file = dir.join("DevToolsActivePort");
    for _ in 0..300 {
        if let Ok(s) = std::fs::read_to_string(&file) {
            let mut lines = s.lines();
            if let (Some(port), Some(path)) = (lines.next(), lines.next()) {
                return format!("ws://127.0.0.1:{port}{path}");
            }
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("the engine wrote no DevToolsActivePort");
}

fn fixture(name: &str) -> Option<&'static str> {
    Some(match name {
        "basic.html" => include_str!("fixtures/basic.html"),
        "hang.html" => include_str!("fixtures/hang.html"),
        "roles.html" => include_str!("fixtures/roles.html"),
        "act.html" => include_str!("fixtures/act.html"),
        "frames.html" => include_str!("fixtures/frames.html"),
        "widget.html" => include_str!("fixtures/widget.html"),
        "detect.html" => include_str!("fixtures/detect.html"),
        "interact.html" => include_str!("fixtures/interact.html"),
        "bounce.html" => include_str!("fixtures/bounce.html"),
        _ => return None,
    })
}

/// The fixtures at `/<name>` on two origins of one port: `site`
/// (`http://127.0.0.1:p`) and `other` (`http://localhost:p`, cross-site to `site`).
pub struct FixtureServer {
    pub site: String,
    pub other: String,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for FixtureServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl FixtureServer {
    pub async fn start() -> FixtureServer {
        Self::start_with(Router::new()).await
    }

    /// The fixtures plus the test's own routes.
    pub async fn start_with(routes: Router) -> FixtureServer {
        let app = routes.fallback(|uri: axum::http::Uri| async move {
            match fixture(uri.path().trim_start_matches('/')) {
                Some(html) => {
                    ([(header::CONTENT_TYPE, "text/html; charset=utf-8")], html).into_response()
                }
                None => axum::http::StatusCode::NOT_FOUND.into_response(),
            }
        });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let task = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        FixtureServer {
            site: format!("http://127.0.0.1:{port}"),
            other: format!("http://localhost:{port}"),
            task,
        }
    }
}

/// A server whose `/flaky` answers garbage for `fail_for` after its first hit, then 200.
/// By time, not by count: the browser's error page reloads itself now and then,
/// so a count of requests is not a count of our tries. Returns the URL and the
/// loads: a request counts only when it comes over 2 s after the last counted
/// one (our tries are `GOTO_GAP` = 3 s apart; the browser's own re-sends and
/// error-page reloads of one try come within that).
pub async fn flaky_server(fail_for: Duration) -> (String, Arc<AtomicU32>) {
    let first = Arc::new(std::sync::OnceLock::<std::time::Instant>::new());
    let counted = Arc::new(std::sync::Mutex::new(None::<std::time::Instant>));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let hits = Arc::new(AtomicU32::new(0));
    let seen = hits.clone();
    tokio::spawn(async move {
        loop {
            let Ok((mut sock, _)) = listener.accept().await else {
                return;
            };
            let seen = seen.clone();
            let first = first.clone();
            let counted = counted.clone();
            tokio::spawn(async move {
                let mut buf = [0u8; 2048];
                let n = sock.read(&mut buf).await.unwrap_or(0);
                if !String::from_utf8_lossy(&buf[..n]).starts_with("GET /flaky ") {
                    let _ = sock
                        .write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                        .await;
                    return;
                }
                {
                    let mut last = counted.lock().unwrap();
                    if last.is_none_or(|t| t.elapsed() > Duration::from_secs(2)) {
                        *last = Some(std::time::Instant::now());
                        seen.fetch_add(1, Ordering::SeqCst);
                    }
                }
                let since = *first.get_or_init(std::time::Instant::now);
                if since.elapsed() < fail_for {
                    // Not HTTP: net::ERR_INVALID_HTTP_RESPONSE (a dropped socket is
                    // sometimes retried by the network stack itself).
                    let _ = sock.write_all(b"garbage\r\n\r\n").await;
                    return;
                }
                let body = "<title>Flaky</title><p>ok</p>";
                let head = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = sock.write_all(head.as_bytes()).await;
                let _ = sock.write_all(body.as_bytes()).await;
            });
        }
    });
    (format!("http://127.0.0.1:{port}/flaky"), hits)
}
