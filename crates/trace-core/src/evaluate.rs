//! Measures whether released insights moved their KPI: compares an equal-length
//! window before and after the release with a two-sided Welch test.

use anyhow::Result;
use crate::Pool;

use crate::model::{Evaluation, InsightStatus, Verdict};
use crate::util::{DAY_MS, now_ms};
use crate::{insights, kpi};

#[derive(Debug, Clone, Copy)]
pub struct Options {
    /// Longest before/after window, in days.
    pub window_days: f64,
    /// Fewest sessions needed in each window before a verdict is given.
    pub min_sessions: usize,
    /// Two-sided p-value below which a change counts as real.
    pub alpha: f64,
}

impl Default for Options {
    fn default() -> Self {
        Self { window_days: 14.0, min_sessions: 30, alpha: 0.05 }
    }
}

/// Evaluates every released insight that is linked to a KPI and stores the result.
pub async fn run(pool: &Pool, project_id: i64, opts: Options) -> Result<Vec<Evaluation>> {
    let mut out = Vec::new();
    for insight in insights::list(pool, project_id, Some(InsightStatus::Released)).await? {
        let (Some(kpi_id), Some(released_at)) = (insight.kpi_id, insight.released_at) else { continue };
        let Some(kpi) = kpi::get(pool, project_id, kpi_id).await? else { continue };

        let now = now_ms();
        let window = (now - released_at).min((opts.window_days * DAY_MS as f64) as i64);
        let window_days = window as f64 / DAY_MS as f64;
        let before = kpi::measure(pool, project_id, &kpi.definition, released_at - window, released_at).await?;
        let after = kpi::measure(pool, project_id, &kpi.definition, released_at, released_at + window).await?;

        let fmt = |v: Option<f64>| v.map(|v| kpi.definition.format_value(v)).unwrap_or_else(|| "n/a".into());
        let (verdict, p_value, summary) = if window < DAY_MS {
            (Verdict::InsufficientData, None, "Released less than a day ago. Waiting for data.".to_string())
        } else if before.n() < opts.min_sessions || after.n() < opts.min_sessions {
            (
                Verdict::InsufficientData,
                None,
                format!(
                    "Not enough sessions yet ({} before, {} after; need {} each).",
                    before.n(),
                    after.n(),
                    opts.min_sessions
                ),
            )
        } else {
            let (delta, p) = welch(&before.samples, &after.samples);
            let good = delta * kpi.direction.sign() > 0.0;
            let verdict = match (p < opts.alpha, good) {
                (false, _) => Verdict::NoChange,
                (true, true) => Verdict::Improved,
                (true, false) => Verdict::Regressed,
            };
            let change = if kpi.definition.is_rate() {
                format!("{:+.1} pts", delta * 100.0)
            } else {
                format!("{delta:+.2}")
            };
            let headline = match verdict {
                Verdict::Improved => "Improved",
                Verdict::Regressed => "Regressed",
                _ => "No significant change",
            };
            (
                verdict,
                Some(p),
                format!(
                    "{headline}: {} went from {} to {} ({change}, p={p:.3}) over {window_days:.1} days on each side of the release (n={} vs {}).",
                    kpi.name,
                    fmt(before.value),
                    fmt(after.value),
                    before.n(),
                    after.n()
                ),
            )
        };

        let eval = Evaluation {
            insight_id: insight.id,
            kpi_id,
            baseline: before.value,
            current: after.value,
            baseline_n: before.n() as i64,
            current_n: after.n() as i64,
            p_value,
            verdict,
            summary,
            window_days,
            created_at: now,
        };
        sqlx::query(
            "INSERT INTO evaluations (insight_id, kpi_id, baseline, current, baseline_n, current_n, p_value, verdict, summary, window_days, created_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
             ON CONFLICT(insight_id) DO UPDATE SET kpi_id = excluded.kpi_id, baseline = excluded.baseline, current = excluded.current,
               baseline_n = excluded.baseline_n, current_n = excluded.current_n, p_value = excluded.p_value, verdict = excluded.verdict,
               summary = excluded.summary, window_days = excluded.window_days, created_at = excluded.created_at",
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
        .execute(pool)
        .await?;
        out.push(eval);
    }
    Ok(out)
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
