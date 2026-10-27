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
use telltale_core::model::{Direction, EventMatch, IdeaStatus, KpiDefinition, KpiInput, NewIdea};
use telltale_core::util::{DAY_MS, now_ms};
use telltale_core::{SqlitePool, ideas, kpi};

const SHOP_HTML: &str = include_str!("../assets/shop/index.html");
const SHOP_JS: &str = include_str!("../assets/shop/shop.js");

/// The Stride shop as a single-page app: every path serves the same page,
/// which loads the collector from `telltale_url` with the project's write key.
pub fn site(telltale_url: &str, write_key: &str) -> Router {
    let html: &'static str = Box::leak(
        SHOP_HTML
            .replace("{{TELLTALE_URL}}", telltale_url.trim_end_matches('/'))
            .replace("{{WRITE_KEY}}", write_key)
            .into_boxed_str(),
    );
    Router::new()
        .route("/shop.js", get(|| async { ([(header::CONTENT_TYPE, "application/javascript")], SHOP_JS).into_response() }))
        .route("/favicon.ico", get(|| async { axum::http::StatusCode::NO_CONTENT }))
        .fallback(get(move || async move { Html(html) }))
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

pub async fn seed(pool: &SqlitePool, project_id: i64, sessions: usize) -> Result<()> {
    let existing: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM sessions WHERE project_id = ?")
        .bind(project_id)
        .fetch_one(pool)
        .await?;
    if existing > 0 {
        anyhow::bail!("this database already has {existing} sessions; run the demo against a fresh one, e.g. `telltale --db demo.db demo`");
    }

    let now = now_ms();
    let released_at = now - 7 * DAY_MS;
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
                        selector: Some("/shop.js:199".into()),
                        text: Some("Uncaught TypeError: Cannot read properties of undefined (reading 'annual')".into()),
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
                let product = format!("/products/shoe-{}", rng.range(1, 6));
                s.visit(&mut rng, &product);
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
            if rng.chance(if after_fix { 0.52 } else { 0.38 }) {
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
             VALUES (?, ?, ?, ?, ?, ?, ?)",
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
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(project_id)
            .bind(&sid)
            .bind(ts)
            .bind(e.kind)
            .bind(&e.path)
            .bind(&e.selector)
            .bind(&e.text)
            .bind(&e.name)
            .bind(e.value)
            .execute(&mut *tx)
            .await?;
            n_events += 1;
        }
    }
    tx.commit().await?;
    sqlx::query("UPDATE projects SET name = 'Stride' WHERE id = ?")
        .bind(project_id)
        .execute(pool)
        .await?;

    let pv = |path: &str| EventMatch { kind: "pageview".into(), path: Some(path.into()), ..Default::default() };
    let checkout = kpi::create(
        pool,
        project_id,
        &KpiInput {
            name: "Checkout conversion".into(),
            description: "Share of sessions reaching checkout that complete payment".into(),
            definition: KpiDefinition::Conversion { from: pv("/checkout"), to: pv("/checkout/success") },
            direction: Direction::Increase,
            target: Some(0.55),
        },
    )
    .await?;
    kpi::create(
        pool,
        project_id,
        &KpiInput {
            name: "Signup completion".into(),
            description: "Share of sessions that start the signup form and finish it".into(),
            definition: KpiDefinition::Conversion {
                from: EventMatch { kind: "form_start".into(), path: Some("/signup".into()), ..Default::default() },
                to: EventMatch { kind: "custom".into(), name: Some("signup_completed".into()), ..Default::default() },
            },
            direction: Direction::Increase,
            target: Some(0.6),
        },
    )
    .await?;
    kpi::create(
        pool,
        project_id,
        &KpiInput {
            name: "Sessions with JS errors".into(),
            description: String::new(),
            definition: KpiDefinition::SessionRate { matcher: EventMatch { kind: "error".into(), ..Default::default() } },
            direction: Direction::Decrease,
            target: Some(0.01),
        },
    )
    .await?;

    let released = ideas::insert(
        pool,
        project_id,
        &NewIdea {
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
    ideas::set_status(pool, project_id, released, IdeaStatus::Released, None).await?;
    sqlx::query("UPDATE ideas SET released_at = ? WHERE id = ?")
        .bind(released_at)
        .bind(released)
        .execute(pool)
        .await?;

    println!("Seeded {sessions} sessions and {n_events} events over 30 days, 3 KPIs and 1 released idea.");
    println!("Next: `telltale analyze`, then `telltale serve --demo-site 127.0.0.1:4319` and open both.");
    Ok(())
}
