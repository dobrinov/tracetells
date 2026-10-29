//! Plan templates and the limits they set. An account's effective limits are
//! its template's, with any per-account overrides on top. `None` is no limit.

use std::collections::BTreeMap;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use crate::Pool;

use crate::util::now_ms;

/// The limits a plan can set, in display order.
pub const LIMITS: [(&str, &str); 4] = [
    ("retention_days", "Data kept (days)"),
    ("max_sites", "Websites"),
    ("monthly_events", "Events per month"),
    ("max_members", "Members"),
];

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Limits {
    pub retention_days: Option<i64>,
    pub max_sites: Option<i64>,
    pub monthly_events: Option<i64>,
    pub max_members: Option<i64>,
}

impl Limits {
    pub fn get(&self, key: &str) -> Option<i64> {
        match key {
            "retention_days" => self.retention_days,
            "max_sites" => self.max_sites,
            "monthly_events" => self.monthly_events,
            "max_members" => self.max_members,
            _ => None,
        }
    }

    fn set(&mut self, key: &str, v: Option<i64>) {
        match key {
            "retention_days" => self.retention_days = v,
            "max_sites" => self.max_sites = v,
            "monthly_events" => self.monthly_events = v,
            "max_members" => self.max_members = v,
            _ => {}
        }
    }
}

/// Per-account overrides: a key present overrides that limit (`None` = no limit).
pub type Overrides = BTreeMap<String, Option<i64>>;

pub fn effective(template: &Limits, overrides: &Overrides) -> Limits {
    let mut out = *template;
    for (key, value) in overrides {
        out.set(key, *value);
    }
    out
}

pub fn validate_overrides(overrides: &Overrides) -> Result<()> {
    for (key, value) in overrides {
        if !LIMITS.iter().any(|(k, _)| k == key) {
            bail!("unknown limit {key:?}");
        }
        if value.is_some_and(|v| v < 0) {
            bail!("{key} can't be negative");
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize)]
pub struct Plan {
    pub id: i64,
    pub key: String,
    pub name: String,
    pub limits: Limits,
    pub is_default: bool,
}

/// Templates every server starts with. The operator can edit them later.
const BUILT_IN: &[(&str, &str, Limits)] = &[
    ("unlimited", "Unlimited", Limits { retention_days: None, max_sites: None, monthly_events: None, max_members: None }),
    ("free", "Free", Limits { retention_days: Some(30), max_sites: Some(1), monthly_events: Some(100_000), max_members: Some(1) }),
    ("friend", "Friend", Limits { retention_days: Some(365), max_sites: Some(5), monthly_events: Some(1_000_000), max_members: Some(10) }),
    ("owner", "Owner", Limits { retention_days: None, max_sites: None, monthly_events: None, max_members: None }),
];

/// Adds missing built-in templates. On a server with no templates yet, makes
/// `default_key` the default (falling back to `unlimited`).
pub async fn seed(pool: &Pool, default_key: Option<&str>) -> Result<()> {
    let first_start: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM plans").fetch_one(pool).await?;
    for (key, name, l) in BUILT_IN {
        sqlx::query(
            "INSERT INTO plans (key, name, retention_days, max_sites, monthly_events, max_members, created_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7) ON CONFLICT DO NOTHING",
        )
        .bind(key)
        .bind(name)
        .bind(l.retention_days)
        .bind(l.max_sites)
        .bind(l.monthly_events)
        .bind(l.max_members)
        .bind(now_ms())
        .execute(pool)
        .await?;
    }
    if first_start == 0 {
        let key = default_key.filter(|k| BUILT_IN.iter().any(|(b, ..)| b == k)).unwrap_or("unlimited");
        set_default(pool, key).await?;
    }
    Ok(())
}

type PlanRow = (i64, String, String, Option<i64>, Option<i64>, Option<i64>, Option<i64>, crate::db::Flag);
const COLS: &str = "id, key, name, retention_days, max_sites, monthly_events, max_members, is_default";

fn from_row((id, key, name, retention_days, max_sites, monthly_events, max_members, is_default): PlanRow) -> Plan {
    Plan { id, key, name, is_default: is_default.0, limits: Limits { retention_days, max_sites, monthly_events, max_members } }
}

pub async fn list(pool: &Pool) -> Result<Vec<Plan>> {
    let rows: Vec<PlanRow> = sqlx::query_as(&format!("SELECT {COLS} FROM plans ORDER BY id")).fetch_all(pool).await?;
    Ok(rows.into_iter().map(from_row).collect())
}

pub async fn by_key(pool: &Pool, key: &str) -> Result<Option<Plan>> {
    let row: Option<PlanRow> = sqlx::query_as(&format!("SELECT {COLS} FROM plans WHERE key = $1")).bind(key).fetch_optional(pool).await?;
    Ok(row.map(from_row))
}

pub async fn default_plan(pool: &Pool) -> Result<Plan> {
    let row: Option<PlanRow> = sqlx::query_as(&format!("SELECT {COLS} FROM plans WHERE is_default = 1 LIMIT 1")).fetch_optional(pool).await?;
    match row {
        Some(r) => Ok(from_row(r)),
        None => by_key(pool, "unlimited").await?.context("no default plan"),
    }
}

pub async fn set_default(pool: &Pool, key: &str) -> Result<()> {
    by_key(pool, key).await?.with_context(|| format!("no plan {key:?}"))?;
    sqlx::query("UPDATE plans SET is_default = CASE WHEN key = $1 THEN 1 ELSE 0 END").bind(key).execute(pool).await?;
    Ok(())
}

fn check_key(key: &str) -> Result<()> {
    if key.is_empty() || key.len() > 32 || !key.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-') {
        bail!("plan keys use lowercase letters, digits and -");
    }
    Ok(())
}

fn check_limits(l: &Limits) -> Result<()> {
    if LIMITS.iter().any(|(k, _)| l.get(k).is_some_and(|v| v < 0)) {
        bail!("limits can't be negative");
    }
    Ok(())
}

pub async fn create(pool: &Pool, key: &str, name: &str, l: &Limits) -> Result<Plan> {
    check_key(key)?;
    check_limits(l)?;
    if by_key(pool, key).await?.is_some() {
        bail!("a plan called {key:?} already exists");
    }
    sqlx::query(
        "INSERT INTO plans (key, name, retention_days, max_sites, monthly_events, max_members, created_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7)",
    )
    .bind(key)
    .bind(name.trim())
    .bind(l.retention_days)
    .bind(l.max_sites)
    .bind(l.monthly_events)
    .bind(l.max_members)
    .bind(now_ms())
    .execute(pool)
    .await?;
    by_key(pool, key).await?.context("plan was just created")
}

pub async fn update(pool: &Pool, key: &str, name: &str, l: &Limits) -> Result<Plan> {
    check_limits(l)?;
    let r = sqlx::query(
        "UPDATE plans SET name = $1, retention_days = $2, max_sites = $3, monthly_events = $4, max_members = $5 WHERE key = $6",
    )
    .bind(name.trim())
    .bind(l.retention_days)
    .bind(l.max_sites)
    .bind(l.monthly_events)
    .bind(l.max_members)
    .bind(key)
    .execute(pool)
    .await?;
    if r.rows_affected() == 0 {
        bail!("no plan {key:?}");
    }
    by_key(pool, key).await?.context("plan exists")
}

/// An account's plan, its overrides and the limits that result.
#[derive(Debug, Clone, Serialize)]
pub struct AccountPlan {
    pub plan: Plan,
    pub overrides: Overrides,
    pub effective: Limits,
}

pub async fn for_account(pool: &Pool, account_id: i64) -> Result<AccountPlan> {
    let (plan_id, overrides): (i64, String) = sqlx::query_as("SELECT plan_id, overrides FROM accounts WHERE id = $1")
        .bind(account_id)
        .fetch_one(pool)
        .await
        .context("no such account")?;
    let row: PlanRow = sqlx::query_as(&format!("SELECT {COLS} FROM plans WHERE id = $1")).bind(plan_id).fetch_one(pool).await?;
    let plan = from_row(row);
    let overrides: Overrides = serde_json::from_str(&overrides).unwrap_or_default();
    Ok(AccountPlan { effective: effective(&plan.limits, &overrides), plan, overrides })
}

pub async fn set_account_plan(pool: &Pool, account_id: i64, key: &str) -> Result<()> {
    let plan = by_key(pool, key).await?.with_context(|| format!("no plan {key:?}"))?;
    sqlx::query("UPDATE accounts SET plan_id = $1 WHERE id = $2").bind(plan.id).bind(account_id).execute(pool).await?;
    Ok(())
}

pub async fn set_overrides(pool: &Pool, account_id: i64, overrides: &Overrides) -> Result<()> {
    validate_overrides(overrides)?;
    sqlx::query("UPDATE accounts SET overrides = $1 WHERE id = $2")
        .bind(serde_json::to_string(overrides)?)
        .bind(account_id)
        .execute(pool)
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overrides_win_and_can_be_unlimited() {
        let free = BUILT_IN[1].2;
        let mut o = Overrides::new();
        assert_eq!(effective(&free, &o), free);
        o.insert("retention_days".into(), Some(30));
        o.insert("max_sites".into(), None);
        let e = effective(&free, &o);
        assert_eq!((e.retention_days, e.max_sites, e.monthly_events), (Some(30), None, Some(100_000)));
        o.remove("retention_days");
        assert_eq!(effective(&free, &o).retention_days, Some(30), "removing an override restores the template");
        assert!(validate_overrides(&BTreeMap::from([("bogus".to_string(), Some(1))])).is_err());
    }
}
