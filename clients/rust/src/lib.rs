//! Report a server's errors to [Trace](https://tracetells.com).
//!
//! ```no_run
//! // Reads TRACE_SERVER_KEY, TRACE_ENDPOINT, TRACE_ENVIRONMENT and TRACE_RELEASE.
//! // Without a key it does nothing. Keep the guard: dropping it flushes.
//! let _trace = tracetells::init(tracetells::Config::from_env());
//!
//! if let Err(e) = std::fs::read_to_string("missing.toml") {
//!     tracetells::report(&e);
//! }
//! ```
//!
//! Panics are reported automatically. With the `tracing` feature, events logged
//! at error level are too ([`TracingLayer`]); with `tower` (or `axum`), errors
//! and panics in HTTP handlers carry their route, method and status
//! ([`TraceLayer`]).
//!
//! **What is sent:** the error's type, its message, the backtrace (files
//! relative to your project, lines, functions), the route template, the HTTP
//! method and status, the environment, the release, the Rust version, when it
//! happened and how often. Messages and paths are scrubbed of emails, tokens,
//! card numbers and long numbers before they leave. **Never sent:** request
//! or response bodies, parameters, headers, cookies, environment variables,
//! local variables, source code, host names or who was signed in. There is no
//! option to send them.
//!
//! Errors leave from a background thread, in batches, at most once a second.
//! Repeats of the same error within a minute are folded into a count. The
//! client never panics, never blocks a request, and drops what it can't send.

mod scrub;
#[cfg(feature = "tower")]
mod tower_layer;
#[cfg(feature = "tracing")]
mod tracing_layer;

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::mpsc::{Receiver, RecvTimeoutError, SyncSender, TrySendError, sync_channel};
use std::sync::OnceLock;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[cfg(feature = "tower")]
pub use tower_layer::{TraceLayer, TraceService};
#[cfg(feature = "tracing")]
pub use tracing_layer::TracingLayer;

const QUEUE: usize = 1_000;
const BATCH: usize = 100;
const FOLD: Duration = Duration::from_secs(60);
const SEND_EVERY: Duration = Duration::from_secs(1);
const MAX_FRAMES: usize = 40;

/// Where errors go and how they are labelled.
#[derive(Debug, Clone, Default)]
pub struct Config {
    key: Option<String>,
    endpoint: String,
    environment: String,
    release: String,
}

impl Config {
    /// From `TRACE_SERVER_KEY`, `TRACE_ENDPOINT` (default `https://app.tracetells.com`),
    /// `TRACE_ENVIRONMENT` (default `production` in release builds, `development` otherwise)
    /// and `TRACE_RELEASE` (default: a commit id the platform sets, if any).
    pub fn from_env() -> Self {
        let var = |k: &str| std::env::var(k).ok().map(|v| v.trim().to_string()).filter(|v| !v.is_empty());
        let release = ["TRACE_RELEASE", "SOURCE_VERSION", "HEROKU_SLUG_COMMIT", "RENDER_GIT_COMMIT", "VERCEL_GIT_COMMIT_SHA", "GIT_COMMIT", "GITHUB_SHA"]
            .iter()
            .find_map(|k| var(k))
            .unwrap_or_default();
        Config {
            key: var("TRACE_SERVER_KEY"),
            endpoint: var("TRACE_ENDPOINT").unwrap_or_else(|| "https://app.tracetells.com".into()),
            environment: var("TRACE_ENVIRONMENT").unwrap_or_else(|| if cfg!(debug_assertions) { "development" } else { "production" }.into()),
            release,
        }
    }

    pub fn key(mut self, key: impl Into<String>) -> Self {
        self.key = Some(key.into()).filter(|k: &String| !k.is_empty());
        self
    }
    pub fn endpoint(mut self, url: impl Into<String>) -> Self {
        self.endpoint = url.into();
        self
    }
    pub fn environment(mut self, env: impl Into<String>) -> Self {
        self.environment = env.into();
        self
    }
    pub fn release(mut self, release: impl Into<String>) -> Self {
        self.release = release.into();
        self
    }
}

/// Where an error happened, when it happened during an HTTP request.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Request {
    /// The route template, e.g. `/orders/:id`. A raw path is scrubbed of ids.
    pub route: String,
    pub method: String,
    pub status: u16,
}

thread_local! {
    /// The request a thread is working on, set by [`TraceLayer`] while it polls a
    /// handler, so a panic or a logged error there knows its route.
    static CURRENT: RefCell<Option<Request>> = const { RefCell::new(None) };
}

#[cfg_attr(not(feature = "tower"), allow(dead_code))]
pub(crate) fn with_request<T>(req: &Request, f: impl FnOnce() -> T) -> T {
    let before = CURRENT.with(|c| c.replace(Some(req.clone())));
    let out = f();
    CURRENT.with(|c| *c.borrow_mut() = before);
    out
}

fn current_request() -> Option<Request> {
    CURRENT.try_with(|c| c.borrow().clone()).ok().flatten()
}

#[derive(Debug, Clone, PartialEq)]
struct Frame {
    file: String,
    line: Option<u32>,
    function: String,
    in_app: bool,
}

#[derive(Debug, Clone, PartialEq)]
struct Event {
    class: String,
    message: String,
    frames: Vec<Frame>,
    request: Request,
    at: u64,
    count: u64,
}

impl Event {
    fn key(&self) -> String {
        let top = self.frames.iter().find(|f| f.in_app).map(|f| format!("{}|{}", f.file, f.function)).unwrap_or_default();
        format!("{}|{}|{}|{}|{}", self.class, top, self.request.route, self.request.status, if top.is_empty() { &self.message } else { "" })
    }

    fn json(&self, cfg: &Config) -> serde_json::Value {
        serde_json::json!({
            "class": self.class,
            "message": self.message,
            "backtrace": self.frames.iter().map(|f| serde_json::json!({ "file": f.file, "line": f.line, "function": f.function, "in_app": f.in_app })).collect::<Vec<_>>(),
            "route": self.request.route,
            "method": self.request.method,
            "status": if self.request.status > 0 { Some(self.request.status) } else { None },
            "environment": cfg.environment,
            "release": cfg.release,
            "runtime": { "name": "rust", "version": RUST_VERSION },
            "occurred_at": self.at,
            "count": self.count,
        })
    }
}

/// The compiler that built this crate, as `rustc --version` would say; set by build.rs.
const RUST_VERSION: &str = env!("TRACETELLS_RUSTC");

enum Msg {
    Event(Box<Event>),
    Flush(SyncSender<()>),
}

struct Client {
    tx: SyncSender<Msg>,
}

static CLIENT: OnceLock<Client> = OnceLock::new();

/// Keeps reporting on; dropping it flushes what's queued (for up to two seconds).
#[must_use = "dropping the guard right away flushes and stops nothing, but keep it for the flush at shutdown"]
pub struct Guard(());

impl Drop for Guard {
    fn drop(&mut self) {
        flush(Duration::from_secs(2));
    }
}

/// Starts the client and reports panics. Without a server key it does
/// nothing at all. Calling it again has no effect.
pub fn init(config: Config) -> Guard {
    let Some(key) = config.key.clone() else { return Guard(()) };
    if CLIENT.get().is_some() {
        return Guard(());
    }
    let (tx, rx) = sync_channel(QUEUE);
    let url = format!("{}/v1/errors", config.endpoint.trim_end_matches('/'));
    let spawned = std::thread::Builder::new().name("tracetells".into()).spawn(move || run(rx, config, url, key));
    if spawned.is_ok() && CLIENT.set(Client { tx }).is_ok() {
        install_panic_hook();
    }
    Guard(())
}

fn enabled() -> bool {
    CLIENT.get().is_some()
}

fn enqueue(e: Event) {
    if let Some(c) = CLIENT.get() {
        // A full queue drops the error rather than wait.
        if let Err(TrySendError::Full(_)) = c.tx.try_send(Msg::Event(Box::new(e))) {}
    }
}

/// Sends what's queued now and waits up to `timeout` for it.
pub fn flush(timeout: Duration) {
    if let Some(c) = CLIENT.get() {
        let (done_tx, done_rx) = sync_channel(1);
        if c.tx.send(Msg::Flush(done_tx)).is_ok() {
            let _ = done_rx.recv_timeout(timeout);
        }
    }
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// Reports an error you handled, with a backtrace from here.
pub fn report<E: std::error::Error + ?Sized>(err: &E) {
    if !enabled() {
        return;
    }
    let class = std::any::type_name::<E>();
    let class = if class.starts_with("dyn ") { "Error".to_string() } else { short_type(class) };
    capture(class, chain(err), current_request().unwrap_or_default());
}

/// Reports an error you handled, during the given request.
pub fn report_request<E: std::error::Error + ?Sized>(err: &E, request: Request) {
    if enabled() {
        capture(short_type(std::any::type_name::<E>()), chain(err), request);
    }
}

/// Reports something that went wrong without an error value.
pub fn report_message(class: &str, message: &str) {
    if enabled() {
        capture(class.to_string(), message.to_string(), current_request().unwrap_or_default());
    }
}

/// `std::io::Error` from `std::io::error::Error`; generics kept short too.
fn short_type(name: &str) -> String {
    let base = name.split('<').next().unwrap_or(name);
    let mut parts: Vec<&str> = base.split("::").collect();
    // Keep the crate and the type: sqlx_core::error::Error reads sqlx_core::Error,
    // and the standard library's module instead: std::io::error::Error reads io::Error.
    if parts.len() > 2 {
        let first = if matches!(parts[0], "std" | "core" | "alloc") { parts[1] } else { parts[0] };
        parts = vec![first, parts[parts.len() - 1]];
    }
    parts.join("::")
}

fn chain<E: std::error::Error + ?Sized>(err: &E) -> String {
    let mut out = err.to_string();
    let mut source = err.source();
    while let Some(s) = source {
        out.push_str(": ");
        out.push_str(&s.to_string());
        source = s.source();
    }
    out
}

pub(crate) fn capture(class: String, message: String, request: Request) {
    let frames = frames(&std::backtrace::Backtrace::force_capture().to_string());
    enqueue(event(class, message, frames, request));
}

fn event(class: String, message: String, frames: Vec<Frame>, request: Request) -> Event {
    Event {
        class: scrub::truncate(&scrub::one_line(&class), 200),
        message: scrub::truncate(&scrub::message(&message), 300),
        frames,
        request: Request {
            route: if request.route.is_empty() { String::new() } else { scrub::route(&request.route) },
            method: request.method.to_ascii_uppercase(),
            status: request.status,
        },
        at: now_ms(),
        count: 1,
    }
}

/// Frames from a backtrace's text, without the panic machinery, this crate or
/// the standard library's own frames at the top.
fn frames(text: &str) -> Vec<Frame> {
    let mut out: Vec<Frame> = Vec::new();
    let mut lines = text.lines().peekable();
    while let Some(line) = lines.next() {
        let t = line.trim();
        let Some((n, func)) = t.split_once(": ") else { continue };
        if n.parse::<u32>().is_err() {
            continue;
        }
        let function = strip_hash(func);
        let mut file = String::new();
        let mut line_no = None;
        if let Some(next) = lines.peek()
            && let Some(at) = next.trim().strip_prefix("at ")
        {
            let mut parts = at.rsplitn(3, ':');
            let _col = parts.next();
            line_no = parts.next().and_then(|l| l.parse().ok());
            file = parts.next().unwrap_or(at).to_string();
            lines.next();
        }
        let internal = ["std::", "core::", "alloc::", "<std::", "<core::", "<alloc::", "rust_begin_unwind", "rust_panic", "__rust", "tracetells::", "<tracetells::"];
        let ours = internal.iter().any(|p| function.starts_with(p));
        if out.is_empty() && ours {
            continue;
        }
        let dependency = file.is_empty() || file.contains("/rustc/") || file.contains("registry/src/") || file.contains("/.rustup/") || ours;
        out.push(Frame { file: scrub::file(&file), line: line_no, function: scrub::truncate(&scrub::one_line(&function), 200), in_app: !dependency });
        if out.len() >= MAX_FRAMES {
            break;
        }
    }
    out
}

/// `myapp::handler::h1a2b3c4d5e6f7a8b` → `myapp::handler`.
fn strip_hash(f: &str) -> String {
    match f.rsplit_once("::h") {
        Some((head, hash)) if hash.len() == 16 && hash.chars().all(|c| c.is_ascii_hexdigit()) => head.to_string(),
        _ => f.to_string(),
    }
}

fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let message = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "panic".into());
        let request = current_request().map(|mut r| {
            r.status = 500;
            r
        });
        let mut e = event("panic".into(), message, frames(&std::backtrace::Backtrace::force_capture().to_string()), request.unwrap_or_default());
        // Without a backtrace frame of the app's own, the panic's location stands in.
        if !e.frames.iter().any(|f| f.in_app)
            && let Some(loc) = info.location()
        {
            e.frames.insert(0, Frame { file: scrub::file(loc.file()), line: Some(loc.line()), function: String::new(), in_app: true });
        }
        enqueue(e);
        // The process may be about to exit: send it now.
        flush(Duration::from_secs(2));
        previous(info);
    }));
}

/// The background thread: folds repeats, batches, sends, backs off.
fn run(rx: Receiver<Msg>, cfg: Config, url: String, key: String) {
    let agent = ureq::AgentBuilder::new().timeout(Duration::from_secs(5)).build();
    let auth = format!("Bearer {key}");
    let mut outgoing: Vec<Event> = Vec::new();
    // Errors seen recently: when their minute started, and repeats since (with the latest).
    let mut recent: HashMap<String, (Instant, u64, Option<Event>)> = HashMap::new();
    let mut paused_until: Option<Instant> = None;
    let mut backoff = Duration::from_secs(5);
    let mut last_send = Instant::now();
    loop {
        let msg = rx.recv_timeout(SEND_EVERY);
        let mut flush_now: Option<SyncSender<()>> = None;
        match msg {
            Ok(Msg::Event(e)) => {
                let e = *e;
                let k = e.key();
                match recent.get_mut(&k) {
                    Some((start, n, latest)) if start.elapsed() < FOLD => {
                        *n += 1;
                        *latest = Some(e);
                    }
                    _ => {
                        recent.insert(k, (Instant::now(), 0, None));
                        outgoing.push(e);
                    }
                }
            }
            Ok(Msg::Flush(done)) => flush_now = Some(done),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }
        // Minutes that ended: their repeats go out as one error with a count.
        let ending_all = flush_now.is_some();
        recent.retain(|_, (start, n, latest)| {
            if start.elapsed() < FOLD && !ending_all {
                return true;
            }
            if let Some(mut e) = latest.take() {
                e.count = *n;
                outgoing.push(e);
            }
            *n = 0;
            start.elapsed() < FOLD
        });
        let due = !outgoing.is_empty() && (flush_now.is_some() || outgoing.len() >= BATCH || last_send.elapsed() >= SEND_EVERY);
        if due {
            if paused_until.is_some_and(|t| Instant::now() < t) {
                // Trace asked us to slow down, or is unreachable: drop rather than pile up.
                outgoing.clear();
            } else {
                while !outgoing.is_empty() {
                    let batch: Vec<Event> = outgoing.drain(..outgoing.len().min(BATCH)).collect();
                    let body = serde_json::json!({ "errors": batch.iter().map(|e| e.json(&cfg)).collect::<Vec<_>>() }).to_string();
                    match agent.post(&url).set("authorization", &auth).set("content-type", "application/json").send_string(&body) {
                        Ok(_) => backoff = Duration::from_secs(5),
                        Err(ureq::Error::Status(code, _)) if code != 429 && code < 500 => {
                            // A bad key or request won't get better by retrying; stay quiet.
                        }
                        Err(_) => {
                            paused_until = Some(Instant::now() + backoff);
                            backoff = (backoff * 2).min(Duration::from_secs(300));
                            outgoing.clear();
                        }
                    }
                }
            }
            last_send = Instant::now();
        }
        if let Some(done) = flush_now {
            let _ = done.send(());
        }
    }
}

/// For tests: what the client would send for an error, without sending it.
#[doc(hidden)]
pub fn __payload<E: std::error::Error>(err: &E, request: Request, cfg: &Config) -> serde_json::Value {
    let e = event(short_type(std::any::type_name::<E>()), chain(err), frames(&std::backtrace::Backtrace::force_capture().to_string()), request);
    e.json(cfg)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backtraces_lose_hashes_and_machinery() {
        let text = "   0: std::backtrace::Backtrace::force_capture
             at /rustc/abc/library/std/src/backtrace.rs:310:9
   1: tracetells::capture::h0123456789abcdef
             at /home/me/code/tracetells/clients/rust/src/lib.rs:200:5
   2: shop::orders::show::h0123456789abcdef
             at /home/me/code/shop/src/orders.rs:42:13
   3: tokio::runtime::task::harness::poll
             at /root/.cargo/registry/src/index.crates.io-6f17d22bba15001f/tokio-1.40.0/src/runtime/task/harness.rs:10:1";
        let f = frames(text);
        assert_eq!(f[0], Frame { file: "src/orders.rs".into(), line: Some(42), function: "shop::orders::show".into(), in_app: true });
        assert_eq!((f[1].file.as_str(), f[1].in_app), ("tokio-1.40.0/src/runtime/task/harness.rs", false));
        assert_eq!(f.len(), 2);
    }

    #[test]
    fn short_type_names() {
        assert_eq!(short_type("std::io::error::Error"), "io::Error");
        assert_eq!(short_type("core::io::error::Error"), "io::Error");
        assert_eq!(short_type("serde_json::error::Error"), "serde_json::Error");
        assert_eq!(short_type("MyError"), "MyError");
        assert_eq!(short_type("alloc::boxed::Box<dyn core::error::Error>"), "boxed::Box");
    }
}
