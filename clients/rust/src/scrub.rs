//! The same scrubbing Trace's server does (crates/trace-core/src/privacy.rs
//! and errors.rs), done here first so nothing personal leaves the server.
//! `clients/test-vectors.json` holds the cases every client must agree on.

use std::sync::OnceLock;

use regex::Regex;

struct Patterns {
    email: Regex,
    jwt: Regex,
    url_query: Regex,
    iban: Regex,
    card: Regex,
    token: Regex,
    long_number: Regex,
}

fn patterns() -> &'static Patterns {
    static P: OnceLock<Patterns> = OnceLock::new();
    P.get_or_init(|| Patterns {
        email: Regex::new(r"(?i)[a-z0-9._%+-]+(@|%40)[a-z0-9.-]+\.[a-z]{2,}").unwrap(),
        jwt: Regex::new(r"\beyJ[\w-]+\.[\w-]+\.[\w-]+").unwrap(),
        url_query: Regex::new(r"(https?://[^\s?#]+)[?#][^\s]*").unwrap(),
        iban: Regex::new(r"\b[A-Z]{2}\d{2}(?:\s?[A-Z0-9]{4}){2,7}(?:\s?[A-Z0-9]{1,4})?\b").unwrap(),
        card: Regex::new(r"\b(?:\d[ -]?){12,18}\d\b").unwrap(),
        token: Regex::new(r"[A-Za-z0-9_\-+/=]{20,}").unwrap(),
        long_number: Regex::new(r"\+?\d(?:[\s().-]{0,2}\d){6,}").unwrap(),
    })
}

fn is_token(s: &str) -> bool {
    s.chars().any(|c| c.is_ascii_digit()) && s.chars().any(|c| c.is_ascii_alphabetic())
}

/// On one line: control and invisible characters become spaces, backticks quotes.
pub fn one_line(s: &str) -> String {
    let hidden = |c: char| {
        c.is_control() || matches!(c, '\u{00AD}' | '\u{200B}'..='\u{200F}' | '\u{2028}'..='\u{202E}' | '\u{2060}'..='\u{2069}' | '\u{FEFF}')
    };
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        let c = if hidden(c) { ' ' } else if c == '`' { '\'' } else { c };
        if !(c == ' ' && (out.is_empty() || out.ends_with(' '))) {
            out.push(c);
        }
    }
    out.truncate(out.trim_end().len());
    out
}

pub fn truncate(s: &str, max: usize) -> String {
    match s.char_indices().nth(max) {
        Some((i, _)) => s[..i].to_string(),
        None => s.to_string(),
    }
}

/// Emails, tokens, card numbers, IBANs, phone-like numbers and URL query strings.
pub fn message(s: &str) -> String {
    let p = patterns();
    let s = one_line(s);
    let s = p.url_query.replace_all(&s, "$1?[redacted]");
    let s = p.email.replace_all(&s, "[email]");
    let s = p.jwt.replace_all(&s, "[token]");
    let s = p.iban.replace_all(&s, "[iban]");
    let s = p.card.replace_all(&s, "[card]");
    let s = p.token.replace_all(&s, |c: &regex::Captures| if is_token(&c[0]) { "[token]".to_string() } else { c[0].to_string() });
    p.long_number.replace_all(&s, "[number]").into_owned()
}

/// A route as Trace stores it: no query string, segments that look like ids,
/// emails or tokens replaced. Route parameters (`:id`, `{id}`) pass through.
pub fn route(path: &str) -> String {
    let path = one_line(path.trim()).replace(' ', "%20");
    let path = path.split(['?', '#']).next().unwrap_or("");
    if !path.starts_with('/') {
        return String::new();
    }
    let out = path
        .split('/')
        .map(|seg| {
            if seg.starts_with('{') && seg.ends_with('}') && seg.len() > 2 {
                // axum's {id} and {*rest} read as Trace's :id and :rest.
                format!(":{}", seg[1..seg.len() - 1].trim_start_matches('*'))
            } else if seg.is_empty() || seg.starts_with(':') || seg == "*" {
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
        .join("/");
    truncate(&out, 200)
}

const DEPENDENCY_ROOTS: &[&str] = &["node_modules/", "gems/", "registry/src/", "site-packages/", "vendor/"];
const PROJECT_ROOTS: &[&str] = &["src/", "app/", "lib/", "crates/", "packages/", "config/", "bin/", "spec/", "test/", "tests/"];

/// A frame's file, relative to the project or the dependency it is in.
pub fn file(file: &str) -> String {
    let f = one_line(file).replace('\\', "/");
    let f = f.strip_prefix("file://").unwrap_or(&f);
    let f = f.split(['?', '#']).next().unwrap_or("");
    let absolute = f.starts_with('/') || f.as_bytes().get(1) == Some(&b':') || f.starts_with("~/");
    let rel = if !absolute {
        f.to_string()
    } else if let Some((i, marker)) = DEPENDENCY_ROOTS.iter().filter_map(|m| f.rfind(m).map(|i| (i, *m))).max_by_key(|(i, _)| *i) {
        if marker == "registry/src/" {
            let rest = &f[i + marker.len()..];
            rest.split_once('/').map(|(_, r)| r).unwrap_or(rest).to_string()
        } else {
            f[i..].to_string()
        }
    } else if let Some(i) = PROJECT_ROOTS.iter().find_map(|m| f.find(&format!("/{m}")).map(|i| i + 1)) {
        f[i..].to_string()
    } else {
        f.rsplit('/').next().unwrap_or("").to_string()
    };
    let rel: Vec<&str> = rel.split('/').map(|seg| if seg.contains('@') { ":email" } else { seg }).collect();
    truncate(&rel.join("/"), 200)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_the_shared_vectors() {
        let v: serde_json::Value = serde_json::from_str(include_str!("../../test-vectors.json")).unwrap();
        let pairs = |k: &str| v[k].as_array().unwrap().iter().map(|p| (p[0].as_str().unwrap().to_string(), p[1].as_str().unwrap().to_string())).collect::<Vec<_>>();
        for (i, want) in pairs("message") {
            assert_eq!(message(&i), want, "message {i:?}");
        }
        for (i, want) in pairs("route") {
            assert_eq!(route(&i), want, "route {i:?}");
        }
        for (i, want) in pairs("file") {
            assert_eq!(file(&i), want, "file {i:?}");
        }
        assert_eq!(route("/orders/{id}/items/{*rest}"), "/orders/:id/items/:rest");
    }
}
