//! Measures whether released insights moved their KPI: compares a fixed
//! window before the release with the same window after it, using a two-sided
//! Welch test. The verdict waits until the window after the release has fully
//! passed and then doesn't change, so checking early and often can't turn
//! noise into a result. Each release is judged by its KPI as defined when it
//! shipped.

use anyhow::Result;
use crate::Pool;

use crate::model::{Direction, Evaluation, Insight, InsightStatus, KpiDefinition, Verdict};
use crate::util::{DAY_MS, date, now_ms};
use crate::{insights, kpi};

#[derive(Debug, Clone, Copy)]
pub struct Options {
    /// Days measured on each side of the release. Whole weeks keep the
    /// weekday mix the same before and after.
    pub window_days: u32,
    /// Fewest sessions needed in each window before a verdict is given.
    pub min_sessions: usize,
    /// Two-sided p-value below which a change counts as real.
    pub alpha: f64,
}

impl Default for Options {
    fn default() -> Self {
        Self { window_days: 14, min_sessions: 30, alpha: 0.05 }
    }
}

/// Evaluates every released insight that is linked to a KPI and stores the
/// result. Settled evaluations are left as they are.
pub async fn run(pool: &Pool, project_id: i64, opts: Options) -> Result<Vec<Evaluation>> {
    let mut out = Vec::new();
    let released = insights::list(pool, project_id, Some(InsightStatus::Released)).await?;
    for insight in &released {
        let (Some(kpi_id), Some(released_at)) = (insight.kpi_id, insight.released_at) else { continue };
        if let Some(e) = insight.evaluation.as_ref().filter(|e| e.settled_at.is_some()) {
            out.push(e.clone());
            continue;
        }
        let Some(kpi) = kpi::get(pool, project_id, kpi_id).await? else { continue };
        // The KPI as it was when the change shipped, if Trace kept it.
        let (definition, direction, redefined) = match insights::released_kpi(pool, insight.id).await? {
            Some((def, dir)) => {
                let def: KpiDefinition = serde_json::from_str(&def)?;
                let dir: Direction = dir.parse()?;
                let redefined = def != kpi.definition || dir != kpi.direction;
                (def, dir, redefined)
            }
            None => (kpi.definition.clone(), kpi.direction, false),
        };

        let now = now_ms();
        let window = i64::from(opts.window_days) * DAY_MS;
        let ends_at = released_at + window;
        let fmt = |v: Option<f64>| v.map(|v| definition.format_value(v)).unwrap_or_else(|| "n/a".into());

        let eval = if now < ends_at {
            let so_far = kpi::measure(pool, project_id, &definition, released_at, now).await?;
            let day = ((now - released_at) / DAY_MS + 1).min(i64::from(opts.window_days));
            Evaluation {
                insight_id: insight.id,
                kpi_id,
                baseline: None,
                current: None,
                baseline_n: 0,
                current_n: so_far.n() as i64,
                p_value: None,
                verdict: Verdict::Measuring,
                summary: format!(
                    "Measuring: day {day} of {}, {} sessions since the release. The verdict comes on {}, once the whole \
                     window has passed, so an early swing can't decide it.",
                    opts.window_days,
                    so_far.n(),
                    date(ends_at)
                ),
                window_days: f64::from(opts.window_days),
                created_at: now,
                settled_at: None,
            }
        } else {
            let before = kpi::measure(pool, project_id, &definition, released_at - window, released_at).await?;
            let after = kpi::measure(pool, project_id, &definition, released_at, ends_at).await?;
            let (verdict, p_value, mut summary) = if before.n() < opts.min_sessions || after.n() < opts.min_sessions {
                (
                    Verdict::InsufficientData,
                    None,
                    format!(
                        "Not enough sessions to tell: {} in the {} days before the release and {} after; a verdict needs {} on each side.",
                        before.n(),
                        opts.window_days,
                        after.n(),
                        opts.min_sessions
                    ),
                )
            } else {
                let (delta, p) = welch(&before.samples, &after.samples);
                let good = delta * direction.sign() > 0.0;
                let verdict = match (p < opts.alpha, good) {
                    (false, _) => Verdict::NoChange,
                    (true, true) => Verdict::Improved,
                    (true, false) => Verdict::Regressed,
                };
                let change = if definition.is_rate() { format!("{:+.1} pts", delta * 100.0) } else { format!("{delta:+.2}") };
                let headline = match verdict {
                    Verdict::Improved => "Improved",
                    Verdict::Regressed => "Regressed",
                    _ => "No significant change",
                };
                (
                    verdict,
                    Some(p),
                    format!(
                        "{headline}: {} went from {} to {} ({change}, p={p:.3}) over the {} days on each side of the release (n={} vs {}).",
                        kpi.name,
                        fmt(before.value),
                        fmt(after.value),
                        opts.window_days,
                        before.n(),
                        after.n()
                    ),
                )
            };
            if redefined {
                summary.push_str(" Measured with the KPI as it was defined when this shipped; it has changed since.");
            }
            let others = overlapping(&released, insight, released_at, window);
            if !others.is_empty() {
                summary.push_str(&format!(
                    " Other changes to this KPI shipped within {} days of this one ({}), so part of the difference may be theirs.",
                    opts.window_days,
                    others.iter().map(|id| format!("#{id}")).collect::<Vec<_>>().join(", ")
                ));
            }
            Evaluation {
                insight_id: insight.id,
                kpi_id,
                baseline: before.value,
                current: after.value,
                baseline_n: before.n() as i64,
                current_n: after.n() as i64,
                p_value,
                verdict,
                summary,
                window_days: f64::from(opts.window_days),
                created_at: now,
                settled_at: Some(now),
            }
        };
        store(pool, &eval).await?;
        out.push(eval);
    }
    Ok(out)
}

/// Other released insights on the same KPI whose release falls inside this
/// one's before or after window.
fn overlapping(released: &[Insight], insight: &Insight, at: i64, window: i64) -> Vec<i64> {
    released
        .iter()
        .filter(|o| o.id != insight.id && o.kpi_id == insight.kpi_id)
        .filter(|o| o.released_at.is_some_and(|t| (t - at).abs() < window))
        .map(|o| o.id)
        .collect()
}

async fn store(pool: &Pool, eval: &Evaluation) -> Result<()> {
    sqlx::query(
        "INSERT INTO evaluations (insight_id, kpi_id, baseline, current, baseline_n, current_n, p_value, verdict, summary, window_days, created_at, settled_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
         ON CONFLICT(insight_id) DO UPDATE SET kpi_id = excluded.kpi_id, baseline = excluded.baseline, current = excluded.current,
           baseline_n = excluded.baseline_n, current_n = excluded.current_n, p_value = excluded.p_value, verdict = excluded.verdict,
           summary = excluded.summary, window_days = excluded.window_days, created_at = excluded.created_at,
           settled_at = excluded.settled_at",
    )
    .bind(eval.insight_id)
    .bind(eval.kpi_id)
    .bind(crate::db::Real(eval.baseline))
    .bind(crate::db::Real(eval.current))
    .bind(eval.baseline_n)
    .bind(eval.current_n)
    .bind(crate::db::Real(eval.p_value))
    .bind(eval.verdict.as_str())
    .bind(&eval.summary)
    .bind(eval.window_days)
    .bind(eval.created_at)
    .bind(eval.settled_at)
    .execute(pool)
    .await?;
    Ok(())
}

/// Difference in means (b − a) and its two-sided p-value under a normal
/// approximation of Welch's t-test. Works for 0/1 samples (rates) and counts.
pub fn welch(a: &[f64], b: &[f64]) -> (f64, f64) {
    let (ma, va) = mean_var(a);
    let (mb, vb) = mean_var(b);
    let delta = mb - ma;
    let se = (va / a.len() as f64 + vb / b.len() as f64).sqrt();
    if se == 0.0 || !se.is_finite() {
        return (delta, if delta == 0.0 { 1.0 } else { 0.0 });
    }
    let z = delta / se;
    (delta, erfc(z.abs() / std::f64::consts::SQRT_2))
}

fn mean_var(xs: &[f64]) -> (f64, f64) {
    let n = xs.len() as f64;
    let mean = xs.iter().sum::<f64>() / n;
    let var = if xs.len() > 1 { xs.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / (n - 1.0) } else { 0.0 };
    (mean, var)
}

/// Complementary error function (Numerical Recipes `erfcc`, |error| < 1.2e-7).
fn erfc(x: f64) -> f64 {
    let z = x.abs();
    let t = 1.0 / (1.0 + 0.5 * z);
    let r = t * (-z * z - 1.26551223
        + t * (1.00002368
            + t * (0.37409196
                + t * (0.09678418
                    + t * (-0.18628806
                        + t * (0.27886807 + t * (-1.13520398 + t * (1.48851587 + t * (-0.82215223 + t * 0.17087277)))))))))
        .exp();
    if x >= 0.0 { r } else { 2.0 - r }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn erfc_matches_known_values() {
        assert!((erfc(0.0) - 1.0).abs() < 1e-6);
        assert!((erfc(1.0) - 0.157_299_2).abs() < 1e-6);
        // z = 1.96 → p ≈ 0.05
        assert!((erfc(1.96 / std::f64::consts::SQRT_2) - 0.05).abs() < 1e-3);
    }

    /// `n` sessions starting at `at`, one hour apart; the first `converted` reach /done.
    async fn traffic(pool: &Pool, site: i64, tag: &str, at: i64, n: usize, converted: usize) {
        for i in 0..n {
            let (sid, t) = (format!("{tag}-{i}"), at + i as i64 * 3_600_000);
            sqlx::query("INSERT INTO sessions (id, project_id, started_at, last_seen_at) VALUES ($1, $2, $3, $3)")
                .bind(&sid).bind(site).bind(t).execute(pool).await.unwrap();
            let mut paths = vec!["/"];
            if i < converted {
                paths.push("/done");
            }
            for (k, path) in paths.into_iter().enumerate() {
                sqlx::query("INSERT INTO events (project_id, session_id, ts, type, path) VALUES ($1, $2, $3, 'pageview', $4)")
                    .bind(site).bind(&sid).bind(t + k as i64 * 1000).bind(path).execute(pool).await.unwrap();
            }
        }
    }

    #[tokio::test]
    async fn waits_for_the_window_then_settles() {
        let target = crate::db::test_target("evaluate").await;
        let pool = crate::db::connect(&target).await.unwrap();
        let (_, site) = crate::db::create_account_with_site(&pool, "Shop", None).await.unwrap();
        let kpis = |to: &str| {
            crate::config::parse(&format!(r#"{{"kpis":{{"done":{{"conversion":{{"from":{{"pageview":"/"}},"to":{{"pageview":"{to}"}}}},"goal":"increase"}}}}}}"#)).unwrap()
        };
        crate::config::apply(&pool, site.id, &kpis("/done"), "test").await.unwrap();
        let kpi_id = kpi::list(&pool, site.id).await.unwrap()[0].id;
        let now = now_ms();
        let day = |d: i64| now - d * DAY_MS;
        let new = |title: &str| crate::model::NewInsight {
            title: title.into(), problem: "p".into(), proposal: "x".into(), kpi_id: Some(kpi_id), source: "test".into(), ..Default::default()
        };
        let mut ids = Vec::new();
        for title in ["Shipped three weeks ago", "Shipped two days after it", "Shipped yesterday"] {
            let id = insights::insert(&pool, site.id, &new(title)).await.unwrap().unwrap();
            sqlx::query("UPDATE insights SET created_at = $1 WHERE id = $2").bind(day(60)).bind(id).execute(&pool).await.unwrap();
            ids.push(id);
        }
        let release = |id: i64, at: i64| insights::set_status(&pool, site.id, id, InsightStatus::Released, None, Some(at));

        // Release times are checked.
        assert!(release(ids[0], now + DAY_MS).await.is_err(), "in the future");
        assert!(release(ids[0], day(61)).await.is_err(), "before the insight existed");
        assert!(insights::set_status(&pool, site.id, ids[0], InsightStatus::Selected, None, Some(day(1))).await.is_err());

        // 20% converted in the 14 days before day 21, 80% in the 14 days after.
        traffic(&pool, site.id, "before", day(34), 40, 8).await;
        traffic(&pool, site.id, "after", day(20), 40, 32).await;
        release(ids[0], day(21)).await.unwrap();
        release(ids[1], day(19)).await.unwrap();
        // The goal moves after ids[1] shipped; it is still judged by /done.
        crate::config::apply(&pool, site.id, &kpis("/thanks"), "test").await.unwrap();
        release(ids[2], day(1) - 1000).await.unwrap();

        let evals = run(&pool, site.id, Options::default()).await.unwrap();
        let by_id = |id: i64| evals.iter().find(|e| e.insight_id == id).unwrap().clone();
        let first = by_id(ids[0]);
        assert_eq!(first.verdict, Verdict::Improved, "{}", first.summary);
        assert!(first.settled_at.is_some() && first.summary.contains(&format!("#{}", ids[1])), "{}", first.summary);
        assert!(by_id(ids[1]).summary.contains("as it was defined when this shipped"), "{}", by_id(ids[1]).summary);
        let latest = by_id(ids[2]);
        assert_eq!((latest.verdict, latest.settled_at), (Verdict::Measuring, None));
        assert!(latest.summary.starts_with("Measuring: day 2 of 14") && latest.summary.contains(&date(day(1) - 1000 + 14 * DAY_MS)), "{}", latest.summary);

        // A settled verdict doesn't move, whatever arrives later.
        traffic(&pool, site.id, "late", day(20) + 1, 40, 0).await;
        let again = run(&pool, site.id, Options::default()).await.unwrap();
        let first_again = again.iter().find(|e| e.insight_id == ids[0]).unwrap();
        assert_eq!((first_again.verdict, first_again.settled_at, first_again.p_value), (first.verdict, first.settled_at, first.p_value));
        // Correcting the release time starts the measurement over.
        release(ids[0], day(18)).await.unwrap();
        assert!(insights::get(&pool, site.id, ids[0]).await.unwrap().unwrap().evaluation.is_none());
        crate::db::drop_test_target(&target).await;
    }

    #[test]
    fn welch_detects_a_real_lift() {
        let a: Vec<f64> = (0..1000).map(|i| if i % 10 < 4 { 1.0 } else { 0.0 }).collect(); // 40%
        let b: Vec<f64> = (0..1000).map(|i| if i % 2 == 0 { 1.0 } else { 0.0 }).collect(); // 50%
        let (delta, p) = welch(&a, &b);
        assert!((delta - 0.1).abs() < 1e-9);
        assert!(p < 0.001);
        let (_, p_same) = welch(&a, &a);
        assert!(p_same > 0.99);
    }
}
