//! The demo: `seed` fills a database with 30 days of simulated traffic for
//! "Stride", a small shoe shop with a few planted problems (a slow pay button,
//! a JS error on pricing, a leaky signup form, dead clicks on product images)
//! and one released fix to evaluate. `site` serves that same shop as a real,
//! instrumented website, so live clicks land next to the simulated history.

use anyhow::Result;
use axum::Router;
use axum::http::header;
use axum::response::{Html, IntoResponse};
use axum::routing::get;
use trace_core::model::{InsightStatus, NewInsight};
use trace_core::util::{DAY_MS, now_ms};
use trace_core::model::Project;
use trace_core::{Pool, config, db, insights, kpi};

const SHOP_HTML: &str = include_str!("../assets/shop/index.html");
const SHOP_JS: &str = include_str!("../assets/shop/shop.js");
/// The shop's own trace.json, as it would live in the shop's repository.
pub const SHOP_CONFIG: &str = include_str!("../assets/shop/trace.json");

/// The line of shop.js that throws when someone picks Annual on /pricing,
/// as a browser would report it.
fn annual_error_line() -> usize {
    SHOP_JS.lines().position(|l| l.contains("pricing.yearly.annual")).expect("shop.js still has the Annual bug") + 1
}

/// The Stride shop as a single-page app: every path serves the same page,
/// which loads the collector from `trace_url` with the project's write key.
pub fn site(trace_url: &str, write_key: &str) -> Router {
    let trace_url = trace_url.trim_end_matches('/');
    let html: &'static str = Box::leak(
        SHOP_HTML
            .replace("{{TRACE_URL}}", trace_url)
            .replace("{{WRITE_KEY}}", write_key)
            .replace("{{BASE}}", "")
            .replace("{{REVIEWS}}", "api")
            .replace("{{CONSENT}}", "")
            .replace("{{RECORDED_URL}}", &format!("{trace_url}/#/behaviour"))
            .into_boxed_str(),
    );
    Router::new()
        .route("/trace.json", get(|| async { ([(header::CONTENT_TYPE, "application/json; charset=utf-8")], SHOP_CONFIG).into_response() }))
        .route("/shop.js", get(|| async { ([(header::CONTENT_TYPE, "application/javascript; charset=utf-8")], SHOP_JS).into_response() }))
        .route("/favicon.ico", get(|| async { axum::http::StatusCode::NO_CONTENT }))
        // Planted problem: the reviews API fails about 30% of the time.
        .route(
            "/api/reviews/{id}",
            get(|| async {
                static CALLS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
                let n = CALLS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                if n % 10 < 3 {
                    (axum::http::StatusCode::INTERNAL_SERVER_ERROR, "upstream timeout").into_response()
                } else {
                    axum::Json(serde_json::json!({ "rating": 4.6, "count": 128 })).into_response()
                }
            }),
        )
        .fallback(get(move || async move { Html(html) }))
}

/// Every demo account uses this password. It's far below the 10-character
/// minimum, so the accounts are inserted directly: only `trace demo`,
/// which refuses to touch a database with data in it, can create them.
const DEMO_PASSWORD: &str = "1";

/// The demo's accounts: the operator's own on `owner` with two websites, a
/// friend's on `friend` and a tester's on `free`, with one person per role.
/// Returns the Stride shop, which gets the simulated traffic.
async fn seed_accounts(pool: &Pool) -> Result<Project> {
    use trace_core::auth::{Role, hash_password, set_operator};
    let hash = hash_password(DEMO_PASSWORD)?;
    let add_user = |account: i64, email: &'static str, name: &'static str, role: Role| {
        let hash = hash.clone();
        async move {
            sqlx::query("INSERT INTO users (account_id, email, name, password_hash, role, created_at) VALUES ($1, $2, $3, $4, $5, $6)")
                .bind(account)
                .bind(email)
                .bind(name)
                .bind(hash)
                .bind(role.as_str())
                .bind(now_ms())
                .execute(pool)
                .await
        }
    };
    let (labs, shop) = db::create_account_with_site(pool, "Stride", Some("owner")).await?;
    db::rename_account(pool, labs.id, "Stride Labs").await?;
    db::create_site(pool, labs.id, "Stride Docs").await?;
    add_user(labs.id, "owner@example.com", "Olivia Owner", Role::Owner).await?;
    add_user(labs.id, "admin@example.com", "Adam Admin", Role::Admin).await?;
    add_user(labs.id, "user@example.com", "Ursula User", Role::User).await?;
    set_operator(pool, "owner@example.com", true).await?;

    let (bolt, _) = db::create_account_with_site(pool, "Bolt Bikes", Some("friend")).await?;
    add_user(bolt.id, "friend@example.com", "Fred Friend", Role::Owner).await?;
    let (tryout, _) = db::create_account_with_site(pool, "Tryout", Some("free")).await?;
    add_user(tryout.id, "free@example.com", "Fay Free", Role::Owner).await?;
    Ok(shop)
}

/// xorshift64*: deterministic and dependency-free, which is all a demo needs.
struct Rng(u64);

impl Rng {
    fn next_f64(&mut self) -> f64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        (self.0.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 11) as f64 / (1u64 << 53) as f64
    }
    fn chance(&mut self, p: f64) -> bool {
        self.next_f64() < p
    }
    fn range(&mut self, lo: i64, hi: i64) -> i64 {
        lo + (self.next_f64() * (hi - lo) as f64) as i64
    }
}

#[derive(Default)]
struct Ev {
    kind: &'static str,
    path: String,
    selector: Option<String>,
    text: Option<String>,
    name: Option<String>,
    value: Option<f64>,
}

struct Session {
    t: i64,
    path: String,
    events: Vec<(i64, Ev)>,
}

impl Session {
    fn emit(&mut self, kind: &'static str, selector: Option<&str>, text: Option<&str>) {
        self.t += 400;
        self.events.push((
            self.t,
            Ev { kind, path: self.path.clone(), selector: selector.map(Into::into), text: text.map(Into::into), ..Default::default() },
        ));
    }
    fn custom(&mut self, name: &str) {
        self.events.push((self.t, Ev { kind: "custom", path: self.path.clone(), name: Some(name.into()), ..Default::default() }));
    }
    fn visit(&mut self, rng: &mut Rng, path: &str) {
        self.leave(rng);
        self.t += rng.range(500, 3000);
        self.path = path.into();
        self.events.push((self.t, Ev { kind: "pageview", path: path.into(), ..Default::default() }));
    }
    fn leave(&mut self, rng: &mut Rng) {
        if self.path.is_empty() {
            return;
        }
        let dwell = rng.range(4_000, 90_000);
        self.t += dwell;
        let scroll = rng.range(25, 101) as f64;
        self.events.push((self.t, Ev { kind: "scroll", path: self.path.clone(), value: Some(scroll), ..Default::default() }));
        self.events.push((self.t, Ev { kind: "leave", path: self.path.clone(), value: Some(dwell as f64), ..Default::default() }));
    }
}

pub async fn seed(pool: &Pool, sessions: usize) -> Result<()> {
    let existing: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM accounts")
        .fetch_one(pool)
        .await?;
    if existing > 0 {
        anyhow::bail!("this database already has {existing} account(s); run the demo against a fresh one, e.g. `trace --db demo.db demo`");
    }
    let shop = seed_accounts(pool).await?;
    let project_id = shop.id;

    let now = now_ms();
    // Two weeks ago: the fixed window after the release has passed, so the demo shows a verdict.
    let released_at = now - 14 * DAY_MS;
    let mut rng = Rng(0x7e11_7a1e_5eed);
    let mut tx = pool.begin().await?;
    let mut n_events = 0;

    for i in 0..sessions {
        let start = now - 30 * DAY_MS + (i as i64 * 30 * DAY_MS / sessions as i64) + rng.range(0, 3_600_000);
        let after_fix = start >= released_at;
        let mut s = Session { t: start, path: String::new(), events: Vec::new() };
        let sid = format!("demo-{i:05}");
        let vw = match rng.next_f64() {
            x if x < 0.55 => 390,
            x if x < 0.6 => 820,
            _ => 1440,
        };

        let entry = match rng.next_f64() {
            x if x < 0.6 => "/",
            x if x < 0.85 => "/products",
            _ => "/pricing",
        };
        s.visit(&mut rng, entry);
        if entry == "/" {
            match rng.next_f64() {
                x if x < 0.5 => s.visit(&mut rng, "/products"),
                x if x < 0.65 => s.visit(&mut rng, "/pricing"),
                x if x < 0.75 => s.visit(&mut rng, "/signup"),
                _ => {}
            }
        }
        if s.path == "/pricing" {
            if rng.chance(0.25) {
                s.events.push((
                    s.t + 800,
                    Ev {
                        kind: "error",
                        path: "/pricing".into(),
                        selector: Some(format!("/shop.js:{}", annual_error_line())),
                        text: Some("Uncaught TypeError: Cannot read properties of undefined (reading 'annual')".into()),
                        name: Some("exception".into()),
                        ..Default::default()
                    },
                ));
            }
            if rng.chance(0.2) {
                s.visit(&mut rng, "/signup");
            }
        }
        if s.path == "/signup" && rng.chance(0.8) {
            s.emit("form_start", Some("form#signup-form"), None);
            if rng.chance(0.35) {
                s.emit("submit", Some("form#signup-form"), None);
                s.custom("signup_completed");
            }
        }
        if s.path == "/products" {
            if rng.chance(0.2) {
                s.emit("click", Some("div.product-card > img"), None);
                s.emit("dead_click", Some("div.product-card > img"), None);
            }
            if rng.chance(0.7) {
                s.visit(&mut rng, "/products/:id");
                if rng.chance(0.3) {
                    // The flaky reviews API: a failed request, and the app's own console.error about it.
                    for (kind, text) in [("network", "GET /api/reviews/:id → 500"), ("console", "Reviews failed to load: HTTP 500")] {
                        s.events.push((
                            s.t + 600,
                            Ev { kind: "error", path: s.path.clone(), text: Some(text.into()), name: Some(kind.into()), ..Default::default() },
                        ));
                    }
                }
                if rng.chance(0.6) {
                    s.emit("click", Some("button#add-to-cart"), Some("Add to cart"));
                    s.custom("add_to_cart");
                    s.visit(&mut rng, "/cart");
                    if rng.chance(0.75) {
                        s.emit("click", Some("button#checkout"), Some("Checkout"));
                        s.visit(&mut rng, "/checkout");
                    }
                }
            }
        }
        if s.path == "/checkout" {
            if rng.chance(0.18) {
                for _ in 0..4 {
                    s.emit("click", Some("button#pay"), Some("Pay now"));
                }
                s.emit("rage_click", Some("button#pay"), Some("Pay now"));
            }
            // The released fix (shipping cost shown on the cart page) lifts checkout completion.
            if rng.chance(if after_fix { 0.56 } else { 0.36 }) {
                s.emit("click", Some("button#pay"), Some("Pay now"));
                s.visit(&mut rng, "/checkout/success");
                s.custom("purchase");
            }
        }
        s.leave(&mut rng);

        let last = s.events.last().map(|(t, _)| *t).unwrap_or(start);
        let device = match vw {
            0..768 => "mobile",
            768..1024 => "tablet",
            _ => "desktop",
        };
        sqlx::query(
            "INSERT INTO sessions (id, project_id, visitor_id, started_at, last_seen_at, device, entry_path)
             VALUES ($1, $2, $3, $4, $5, $6, $7)",
        )
        .bind(&sid)
        .bind(project_id)
        .bind(format!("visitor-{}", rng.range(0, sessions as i64 * 2 / 3)))
        .bind(start)
        .bind(last)
        .bind(device)
        .bind(entry)
        .execute(&mut *tx)
        .await?;
        for (ts, e) in s.events {
            sqlx::query(
                "INSERT INTO events (project_id, session_id, ts, type, path, selector, text, name, value)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
            )
            .bind(project_id)
            .bind(&sid)
            .bind(ts)
            .bind(e.kind)
            .bind(&e.path)
            .bind(&e.selector)
            .bind(&e.text)
            .bind(&e.name)
            .bind(trace_core::db::Real(e.value))
            .execute(&mut *tx)
            .await?;
            n_events += 1;
        }
    }
    tx.commit().await?;
    let parsed = config::parse(SHOP_CONFIG).map_err(|e| anyhow::anyhow!(e.join("; ")))?;
    config::apply(pool, project_id, &parsed, "demo: assets/shop/trace.json").await?;
    let checkout = kpi::list(pool, project_id)
        .await?
        .into_iter()
        .find(|k| k.key.as_deref() == Some("checkout_conversion"))
        .expect("the demo config defines checkout_conversion");

    let released = insights::insert(
        pool,
        project_id,
        &NewInsight {
            title: "Show the shipping cost on the cart page".into(),
            problem: "Shoppers only saw the shipping cost on the payment step, and many left there.".into(),
            evidence: vec!["62% of sessions that reached /checkout did not complete payment".into()],
            proposal: "Show the shipping estimate and the order total on /cart, before the checkout button.".into(),
            acceptance_criteria: vec!["/cart shows shipping and total before checkout".into()],
            pages: vec!["/cart".into(), "/checkout".into()],
            kpi_id: Some(checkout.id),
            expected_impact: "Checkout conversion up by 5-10 points".into(),
            confidence: "medium".into(),
            source: "demo".into(),
            fingerprint: Some("demo:shipping-on-cart".into()),
        },
    )
    .await?
    .expect("fresh database");
    insights::set_status(pool, project_id, released, InsightStatus::Released, None, None).await?;
    sqlx::query("UPDATE insights SET released_at = $1 WHERE id = $2")
        .bind(released_at)
        .bind(released)
        .execute(pool)
        .await?;

    println!("Accounts: Stride Labs (owner plan; websites stride and stride-docs), Bolt Bikes (friend), Tryout (free).");
    println!("Sign in as owner@ (also an operator), admin@, user@, friend@ or free@example.com, all with password \"{DEMO_PASSWORD}\" (demo only).");
    println!("Seeded {sessions} sessions and {n_events} events over 30 days, applied the shop's trace.json (3 KPIs, 3 events) and 1 released insight.");
    println!("Next: `trace analyze`, then `trace serve --demo-site 127.0.0.1:4319` and open both.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tower::ServiceExt;

    #[tokio::test]
    async fn the_shop_is_served_with_everything_filled_in() {
        let req = axum::http::Request::builder().uri("/products/shoe-2").body(axum::body::Body::empty()).unwrap();
        let res = site("http://127.0.0.1:4318", "k123").oneshot(req).await.unwrap();
        let html = String::from_utf8(axum::body::to_bytes(res.into_body(), 1 << 20).await.unwrap().to_vec()).unwrap();
        assert!(!html.contains("{{"), "a placeholder is left");
        assert!(html.contains(r#"data-key="k123" data-routes="/products/:id" defer"#) && html.contains(r#"src="/shop.js""#));
        // The simulated Annual error points at the line that really throws.
        assert!(SHOP_JS.lines().nth(annual_error_line() - 1).unwrap().contains("pricing.yearly"));
    }
}
