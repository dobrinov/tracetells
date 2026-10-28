//! Server-side scrubbing of everything the collector sends. The collector
//! already redacts in the browser; this second pass means data from old or
//! hand-written clients is held to the same rules before it is stored, shown on
//! the dashboard or sent to the analyst.

use std::sync::LazyLock;

use regex::Regex;
use serde_json::Value;

static EMAIL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)[a-z0-9._%+-]+(@|%40)[a-z0-9.-]+\.[a-z]{2,}").unwrap());
static JWT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\beyJ[\w-]+\.[\w-]+\.[\w-]+").unwrap());
static URL_QUERY: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(https?://[^\s?#]+)[?#][^\s]*").unwrap());
static IBAN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\b[A-Z]{2}\d{2}(?:\s?[A-Z0-9]{4}){2,7}(?:\s?[A-Z0-9]{1,4})?\b").unwrap());
static CARD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\b(?:\d[ -]?){12,18}\d\b").unwrap());
/// Long strings mixing letters and digits: API keys, session ids, reset tokens.
static TOKEN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[A-Za-z0-9_\-+/=]{20,}").unwrap());
/// Phone numbers, account numbers, dates of birth: 7+ digits, possibly split by separators.
static LONG_NUMBER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\+?\d(?:[\s().-]{0,2}\d){6,}").unwrap());
static DIGIT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\d").unwrap());

fn is_token(s: &str) -> bool {
    s.chars().any(|c| c.is_ascii_digit()) && s.chars().any(|c| c.is_ascii_alphabetic())
}

/// Redacts emails, tokens, card numbers, IBANs and URL query strings.
/// `strict` also masks every digit, for text read from the page (labels,
/// button text), where numbers are usually amounts, balances or account data.
pub fn redact_text(s: &str, strict: bool) -> String {
    let s = URL_QUERY.replace_all(s, "$1?[redacted]");
    let s = EMAIL.replace_all(&s, "[email]");
    let s = JWT.replace_all(&s, "[token]");
    let s = IBAN.replace_all(&s, "[iban]");
    let s = CARD.replace_all(&s, "[card]");
    let s = TOKEN.replace_all(&s, |c: &regex::Captures| {
        if is_token(&c[0]) { "[token]".to_string() } else { c[0].to_string() }
    });
    if strict {
        DIGIT.replace_all(&s, "#").into_owned()
    } else {
        LONG_NUMBER.replace_all(&s, "[number]").into_owned()
    }
}

/// Replaces path segments that can identify a person or carry a secret.
/// Route names produced by the collector (`:id`, `:slug`, `*`) pass through.
pub fn redact_path(path: &str) -> String {
    let path = path.split(['?', '#']).next().unwrap_or("");
    path.split('/')
        .map(|seg| {
            if seg.is_empty() || seg.starts_with(':') || seg == "*" {
                seg.to_string()
            } else if seg.contains('@') || seg.to_ascii_lowercase().contains("%40") {
                ":email".into()
            } else if seg.contains('=') || (seg.matches('.').count() >= 2 && seg.len() >= 24) {
                ":token".into()
            } else if seg.chars().filter(|c| c.is_ascii_digit()).count() >= 3
                || (seg.len() >= 16 && is_token(seg))
                || (seg.len() >= 16 && seg.chars().all(|c| c.is_ascii_hexdigit()))
            {
                ":id".into()
            } else {
                seg.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("/")
}

/// Key names whose values are never stored, matched per word so that
/// `shipping` doesn't trip `pin` but `user_pin` and `cardNumber` do.
const SENSITIVE_KEY_WORDS: &[&str] = &[
    "password", "passwd", "pwd", "pass", "passcode", "passphrase", "secret", "token", "auth", "authorization",
    "bearer", "cookie", "session", "sid", "jwt", "apikey", "key", "credential", "credentials", "card", "cc",
    "ccv", "cvv", "cvc", "pan", "iban", "swift", "bic", "account", "acct", "routing", "sortcode", "ssn", "sin", "nin",
    "tax", "vat", "pin", "otp", "mfa", "totp", "email", "mail", "phone", "mobile", "tel", "address", "street",
    "zip", "postcode", "postal", "birth", "birthday", "dob", "salary", "income", "balance", "passport", "license",
];

pub fn is_sensitive_key(key: &str) -> bool {
    let mut words = String::with_capacity(key.len() + 4);
    let mut prev_lower = false;
    for c in key.chars() {
        if c.is_ascii_uppercase() && prev_lower {
            words.push(' ');
        }
        prev_lower = c.is_ascii_lowercase() || c.is_ascii_digit();
        words.push(if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { ' ' });
    }
    words.split_whitespace().any(|w| SENSITIVE_KEY_WORDS.contains(&w))
}

/// Drops sensitive keys and redacts string values in custom-event props.
pub fn clean_props(v: Value, depth: usize) -> Option<Value> {
    match v {
        Value::Null => None,
        Value::String(s) => Some(Value::String(redact_text(&crate::util::truncate(&s, 200), false))),
        Value::Bool(_) | Value::Number(_) => Some(v),
        Value::Array(items) if depth < 3 => Some(Value::Array(
            items.into_iter().take(20).filter_map(|i| clean_props(i, depth + 1)).collect(),
        )),
        Value::Object(map) if depth < 3 => Some(Value::Object(
            map.into_iter()
                .filter(|(k, _)| !is_sensitive_key(k))
                .take(20)
                .filter_map(|(k, v)| clean_props(v, depth + 1).map(|v| (k, v)))
                .collect(),
        )),
        _ => None,
    }
}

/// Keeps only the origin of a referrer: the rest of a URL often carries tokens.
pub fn referrer_origin(r: &str) -> Option<String> {
    let (scheme, rest) = r.split_once("://")?;
    if !matches!(scheme, "http" | "https") {
        return None;
    }
    let host = rest.split(['/', '?', '#']).next().filter(|h| !h.is_empty())?;
    let host = host.rsplit('@').next().unwrap_or(host); // drop user:pass@
    Some(format!("{scheme}://{host}"))
}

/// User ids must be opaque. The collector hashes email-like ids; anything
/// that still looks personal is dropped.
pub fn clean_user_id(uid: &str) -> Option<String> {
    let uid = uid.trim();
    if uid.is_empty() || uid.len() > 128 || EMAIL.is_match(uid) || uid.contains(' ') {
        return None;
    }
    Some(uid.to_string())
}

// ---- rules added by the workspace ------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuleKind {
    /// CSS selector: no text from inside matching elements (browser only).
    MaskSelector,
    /// Regular expression replaced in all text, browser and server.
    RedactPattern,
    /// Route pattern (`/admin/*`): nothing is collected on these pages.
    IgnorePath,
    /// Custom-event prop name that's always dropped.
    DropProp,
}

impl RuleKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::MaskSelector => "mask_selector",
            Self::RedactPattern => "redact_pattern",
            Self::IgnorePath => "ignore_path",
            Self::DropProp => "drop_prop",
        }
    }
}

impl std::str::FromStr for RuleKind {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> anyhow::Result<Self> {
        Ok(match s {
            "mask_selector" => Self::MaskSelector,
            "redact_pattern" => Self::RedactPattern,
            "ignore_path" => Self::IgnorePath,
            "drop_prop" => Self::DropProp,
            other => anyhow::bail!("unknown rule kind {other:?}"),
        })
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct Rule {
    pub id: i64,
    pub kind: RuleKind,
    pub pattern: String,
    pub replacement: Option<String>,
    pub label: String,
    pub enabled: bool,
    pub created_by: String,
    pub created_at: i64,
}

pub const DEFAULT_REPLACEMENT: &str = "[redacted]";

/// Checks a new rule and returns it normalised, with a message saying what's wrong otherwise.
pub fn validate_rule(kind: RuleKind, pattern: &str, replacement: Option<&str>) -> anyhow::Result<(String, Option<String>)> {
    let pattern = pattern.trim();
    if pattern.is_empty() || pattern.len() > 300 {
        anyhow::bail!("enter a pattern of up to 300 characters");
    }
    match kind {
        RuleKind::MaskSelector => {
            if pattern.contains(['{', '}']) || pattern.contains("</") {
                anyhow::bail!("that doesn't look like a CSS selector");
            }
            Ok((pattern.to_string(), None))
        }
        RuleKind::RedactPattern => {
            let re = regex::RegexBuilder::new(pattern)
                .size_limit(1 << 20)
                .build()
                .map_err(|e| anyhow::anyhow!("not a valid regular expression: {}", e.to_string().lines().last().unwrap_or("syntax error")))?;
            if re.is_match("") {
                anyhow::bail!("this pattern matches empty text, so it would match everywhere");
            }
            let replacement = replacement.map(str::trim).filter(|r| !r.is_empty()).unwrap_or(DEFAULT_REPLACEMENT);
            if replacement.len() > 40 {
                anyhow::bail!("keep the replacement under 40 characters");
            }
            Ok((pattern.to_string(), Some(replacement.to_string())))
        }
        RuleKind::IgnorePath => {
            if !pattern.starts_with('/') {
                anyhow::bail!("page patterns start with /, e.g. /admin/* or /account/billing");
            }
            Ok((pattern.to_string(), None))
        }
        RuleKind::DropProp => {
            if !pattern.chars().all(|c| c.is_ascii_alphanumeric() || "_-.".contains(c)) || pattern.len() > 64 {
                anyhow::bail!("prop names may only use letters, digits, _ - and .");
            }
            Ok((pattern.to_string(), None))
        }
    }
}

type RuleRow = (i64, String, String, Option<String>, String, crate::db::Flag, String, i64);

pub async fn list_rules(pool: &crate::Pool, project_id: i64) -> anyhow::Result<Vec<Rule>> {
    let rows: Vec<RuleRow> = sqlx::query_as(
        "SELECT id, kind, pattern, replacement, label, enabled, created_by, created_at FROM privacy_rules
         WHERE project_id = $1 ORDER BY kind, id",
    )
    .bind(project_id)
    .fetch_all(pool)
    .await?;
    rows.into_iter()
        .map(|(id, kind, pattern, replacement, label, enabled, created_by, created_at)| {
            Ok(Rule { id, kind: kind.parse()?, pattern, replacement, label, enabled: enabled.0, created_by, created_at })
        })
        .collect()
}

pub async fn add_rule(
    pool: &crate::Pool,
    project_id: i64,
    kind: RuleKind,
    pattern: &str,
    replacement: Option<&str>,
    label: &str,
    created_by: &str,
) -> anyhow::Result<i64> {
    let (pattern, replacement) = validate_rule(kind, pattern, replacement)?;
    Ok(sqlx::query_scalar(
        "INSERT INTO privacy_rules (project_id, kind, pattern, replacement, label, created_by, created_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7) RETURNING id",
    )
    .bind(project_id)
    .bind(kind.as_str())
    .bind(pattern)
    .bind(replacement)
    .bind(crate::util::truncate(label.trim(), 80))
    .bind(created_by)
    .bind(crate::util::now_ms())
    .fetch_one(pool)
    .await?)
}

pub async fn set_rule_enabled(pool: &crate::Pool, project_id: i64, id: i64, enabled: bool) -> anyhow::Result<bool> {
    let r = sqlx::query("UPDATE privacy_rules SET enabled = $1 WHERE project_id = $2 AND id = $3")
        .bind(crate::db::Flag(enabled))
        .bind(project_id)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(r.rows_affected() > 0)
}

pub async fn delete_rule(pool: &crate::Pool, project_id: i64, id: i64) -> anyhow::Result<Option<Rule>> {
    let rule = list_rules(pool, project_id).await?.into_iter().find(|r| r.id == id);
    sqlx::query("DELETE FROM privacy_rules WHERE project_id = $1 AND id = $2").bind(project_id).bind(id).execute(pool).await?;
    Ok(rule)
}

/// The enabled rules, compiled for ingest. Kept in memory by the server and
/// rebuilt when an admin changes a rule.
#[derive(Debug, Default, Clone)]
pub struct HostRules {
    masks: Vec<String>,
    redactions: Vec<(Regex, String)>,
    ignore_paths: Vec<String>,
    drop_props: Vec<String>,
}

impl HostRules {
    pub fn from_rules(rules: &[Rule]) -> Self {
        let mut out = Self::default();
        for r in rules.iter().filter(|r| r.enabled) {
            match r.kind {
                RuleKind::MaskSelector => out.masks.push(r.pattern.clone()),
                RuleKind::RedactPattern => {
                    if let Ok(re) = Regex::new(&r.pattern) {
                        out.redactions.push((re, r.replacement.clone().unwrap_or_else(|| DEFAULT_REPLACEMENT.into())));
                    }
                }
                RuleKind::IgnorePath => out.ignore_paths.push(r.pattern.clone()),
                RuleKind::DropProp => out.drop_props.push(r.pattern.to_ascii_lowercase()),
            }
        }
        out
    }

    pub async fn load(pool: &crate::Pool, project_id: i64) -> anyhow::Result<Self> {
        Ok(Self::from_rules(&list_rules(pool, project_id).await?))
    }

    /// Custom patterns first: once built-in masking has turned digits into
    /// `#`, a pattern like `ORD-\d+` could no longer match.
    pub fn redact(&self, s: &str) -> String {
        let mut out = s.to_string();
        for (re, with) in &self.redactions {
            out = re.replace_all(&out, with.as_str()).into_owned();
        }
        out
    }

    pub fn ignores(&self, path: &str) -> bool {
        let p = Some(path);
        self.ignore_paths.iter().any(|pat| crate::model::pattern_matches(&Some(pat.clone()), p))
    }

    pub fn drops(&self, key: &str) -> bool {
        self.drop_props.iter().any(|k| k.eq_ignore_ascii_case(key))
    }

    /// The rules as the collector receives them, written into `d.js`.
    pub fn client_json(&self) -> Value {
        // `(?i)` is the only Rust-only syntax worth supporting; the collector turns it into the `i` flag.
        serde_json::json!({
            "mask": self.masks,
            "redact": self.redactions.iter().map(|(re, with)| serde_json::json!([re.as_str(), with])).collect::<Vec<_>>(),
            "ignore": self.ignore_paths,
            "drop": self.drop_props,
        })
    }
}

/// [`redact_text`] with the workspace's own patterns applied first.
pub fn redact_with(rules: &HostRules, s: &str, strict: bool) -> String {
    redact_text(&rules.redact(s), strict)
}

/// [`clean_props`], also dropping the workspace's own prop names and applying its patterns.
pub fn clean_props_with(rules: &HostRules, v: Value, depth: usize) -> Option<Value> {
    match v {
        Value::String(s) => Some(Value::String(redact_with(rules, &crate::util::truncate(&s, 200), false))),
        Value::Array(items) if depth < 3 => Some(Value::Array(
            items.into_iter().take(20).filter_map(|i| clean_props_with(rules, i, depth + 1)).collect(),
        )),
        Value::Object(map) if depth < 3 => Some(Value::Object(
            map.into_iter()
                .filter(|(k, _)| !is_sensitive_key(k) && !rules.drops(k))
                .take(20)
                .filter_map(|(k, v)| clean_props_with(rules, v, depth + 1).map(|v| (k, v)))
                .collect(),
        )),
        other => clean_props(other, depth),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn redacts_text() {
        assert_eq!(redact_text("Contact jane.doe@example.com", false), "Contact [email]");
        assert_eq!(redact_text("Pay €1,234.56", true), "Pay €#,###.##");
        assert_eq!(redact_text("Card 4242 4242 4242 4242 saved", false), "Card [card] saved");
        assert_eq!(redact_text("IBAN GB82 WEST 1234 5698 7654 32", false), "IBAN [iban]");
        assert_eq!(redact_text("key sk_live_51Hh2kLmN0pQrStUvWxYz", false), "key [token]");
        assert_eq!(redact_text("token eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxIn0.abc-DEF_123", false), "token [token]");
        assert_eq!(redact_text("GET https://api.x.com/v1/me?access_token=abc failed", false), "GET https://api.x.com/v1/me?[redacted] failed");
        assert_eq!(redact_text("Request failed with status code 500", false), "Request failed with status code 500");
        assert_eq!(redact_text("Cannot read properties of undefined (reading 'annual')", false), "Cannot read properties of undefined (reading 'annual')");
        assert_eq!(redact_text("Add to cart", true), "Add to cart");
        assert_eq!(redact_text("call +44 7700 900123 today", false), "call [number] today");
        assert_eq!(redact_text("born 1990-04-12", false), "born [number]");
    }

    #[test]
    fn redacts_paths() {
        assert_eq!(redact_path("/users/jane@example.com/settings"), "/users/:email/settings");
        assert_eq!(redact_path("/reset/8f14e45fceea167a5a36dedd4bea2543"), "/reset/:id");
        assert_eq!(redact_path("/auth/eyJhbGciOiJIUzI1.eyJzdWIiOi.sig"), "/auth/:token");
        assert_eq!(redact_path("/invoices/INV-2024-0042?x=1"), "/invoices/:id");
        assert_eq!(redact_path("/products/:id"), "/products/:id");
        assert_eq!(redact_path("/blog/how-to-choose-running-shoes"), "/blog/how-to-choose-running-shoes");
        assert_eq!(redact_path("/products/shoe-1"), "/products/shoe-1");
        assert_eq!(redact_path("/img/missing-hero-banner.png"), "/img/missing-hero-banner.png");
    }

    #[test]
    fn cleans_props() {
        let p = json!({
            "plan": "pro", "amount": 129, "shipping": "express", "password": "hunter2",
            "cardNumber": "4242", "user_email": "a@b.co", "note": "mail me at a@b.co",
            "nested": { "apiKey": "x", "ok": true }
        });
        assert_eq!(
            clean_props(p, 0).unwrap(),
            json!({ "plan": "pro", "amount": 129, "shipping": "express", "note": "mail me at [email]", "nested": { "ok": true } })
        );
    }

    #[test]
    fn host_rules() {
        assert!(validate_rule(RuleKind::RedactPattern, "(", None).is_err());
        assert!(validate_rule(RuleKind::RedactPattern, "a*", None).is_err(), "matches empty text");
        assert!(validate_rule(RuleKind::IgnorePath, "admin", None).is_err());
        let rule = |kind, pattern: &str, replacement: Option<&str>| Rule {
            id: 0, kind, pattern: pattern.into(), replacement: replacement.map(Into::into), label: String::new(),
            enabled: true, created_by: String::new(), created_at: 0,
        };
        let rules = HostRules::from_rules(&[
            rule(RuleKind::RedactPattern, r"ORD-\d+", Some("[order]")),
            rule(RuleKind::IgnorePath, "/admin/*", None),
            rule(RuleKind::DropProp, "loyalty_id", None),
        ]);
        assert_eq!(redact_with(&rules, "Order ORD-20931 for jane@x.io", true), "Order [order] for [email]");
        assert!(rules.ignores("/admin/users") && !rules.ignores("/products"));
        assert_eq!(
            clean_props_with(&rules, json!({ "Loyalty_ID": "x", "plan": "pro" }), 0).unwrap(),
            json!({ "plan": "pro" })
        );
    }

    #[test]
    fn referrers_and_user_ids() {
        assert_eq!(referrer_origin("https://mail.example.com/reset?token=abc").as_deref(), Some("https://mail.example.com"));
        assert_eq!(referrer_origin("https://user:pw@evil.example/x").as_deref(), Some("https://evil.example"));
        assert_eq!(referrer_origin("android-app://com.x"), None);
        assert_eq!(clean_user_id("user_42").as_deref(), Some("user_42"));
        assert_eq!(clean_user_id("jane@example.com"), None);
    }
}
