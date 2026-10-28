//! Protection for a server that is reachable from the internet: rate limits,
//! the client's real address behind a reverse proxy, and security headers.

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::Mutex;

use axum::extract::Request;
use axum::http::{HeaderMap, HeaderValue, header};
use axum::middleware::Next;
use axum::response::Response;
use trace_core::util::now_ms;

/// Counts events per key in fixed windows that expire. Memory stays
/// bounded: expired keys are dropped when the map grows, and if attackers
/// fill it with live keys anyway it is cleared rather than grown.
#[derive(Default)]
pub struct Counter(Mutex<HashMap<String, (u32, i64)>>);

const MAX_KEYS: usize = 100_000;

impl Counter {
    /// True if `key` has reached `limit` within its current window.
    pub fn over(&self, key: &str, limit: u32) -> bool {
        let map = self.0.lock().unwrap();
        map.get(key).is_some_and(|(n, until)| *n >= limit && now_ms() < *until)
    }

    /// Counts one event for `key`; its window starts with the first event.
    pub fn add(&self, key: &str, window_ms: i64) {
        let now = now_ms();
        let mut map = self.0.lock().unwrap();
        if map.len() >= MAX_KEYS {
            map.retain(|_, (_, until)| *until > now);
            if map.len() >= MAX_KEYS {
                map.clear();
            }
        }
        let e = map.entry(key.to_string()).or_insert((0, now + window_ms));
        if now >= e.1 {
            *e = (0, now + window_ms);
        }
        e.0 = e.0.saturating_add(1);
    }

    /// Counts one event and says whether it is still within `limit`.
    pub fn hit(&self, key: &str, limit: u32, window_ms: i64) -> bool {
        self.add(key, window_ms);
        !self.over(key, limit.saturating_add(1))
    }

    pub fn clear(&self, key: &str) {
        self.0.lock().unwrap().remove(key);
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.0.lock().unwrap().len()
    }
}

/// The address a request came from. Behind a reverse proxy on a private
/// network (Caddy in Docker), that's the last address the proxy added to
/// `X-Forwarded-For`; a direct client can't fake it, because the header is
/// only believed when the connection itself comes from a private address.
pub fn client_ip(peer: Option<SocketAddr>, headers: &HeaderMap) -> String {
    let Some(peer) = peer.map(|p| p.ip()) else { return "unknown".into() };
    if is_private(peer)
        && let Some(forwarded) = headers
            .get_all("x-forwarded-for")
            .iter()
            .filter_map(|v| v.to_str().ok())
            .flat_map(|v| v.split(','))
            .filter_map(|s| s.trim().parse::<IpAddr>().ok())
            .next_back()
    {
        return forwarded.to_string();
    }
    peer.to_string()
}

fn is_private(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => v4.is_loopback() || v4.is_private() || v4.is_link_local(),
        IpAddr::V6(v6) => {
            v6.is_loopback() || (v6.segments()[0] & 0xfe00) == 0xfc00 || v6.to_ipv4_mapped().is_some_and(|v4| is_private(IpAddr::V4(v4)))
        }
    }
}

/// The dashboard's Content-Security-Policy. Its one inline script (the theme
/// switch in index.html, which must run before the page paints) is allowed by
/// hash; everything else comes from this server. Inline `style` attributes
/// are used throughout the dashboard, hence `'unsafe-inline'` for styles only.
pub fn dashboard_csp(index_html: &str) -> String {
    use sha2::Digest as _;
    let scripts: Vec<String> = inline_scripts(index_html)
        .map(|s| format!("'sha256-{}'", data_encoding::BASE64.encode(&sha2::Sha256::digest(s.as_bytes()))))
        .collect();
    format!(
        "default-src 'self'; script-src 'self' {}; style-src 'self' 'unsafe-inline'; img-src 'self' data:; \
         connect-src 'self'; font-src 'self'; object-src 'none'; base-uri 'none'; form-action 'self'; frame-ancestors 'none'",
        scripts.join(" ")
    )
}

/// The bodies of `<script>` elements without a `src`.
fn inline_scripts(html: &str) -> impl Iterator<Item = &str> {
    html.split("<script").skip(1).filter_map(|rest| {
        let (attrs, body) = rest.split_once('>')?;
        if attrs.contains("src=") {
            return None;
        }
        body.split_once("</script>").map(|(b, _)| b)
    })
}

/// Adds security headers to every response. The collector (`/d.js`,
/// `/v1/collect`) runs on other people's sites, so it only gets the ones that
/// don't restrict how it's embedded.
pub async fn security_headers(csp: &'static str, req: Request, next: Next) -> Response {
    let collector = matches!(req.uri().path(), "/d.js" | "/v1/collect");
    let mut res = next.run(req).await;
    let h = res.headers_mut();
    h.insert("x-content-type-options", HeaderValue::from_static("nosniff"));
    if !collector {
        h.insert("x-frame-options", HeaderValue::from_static("DENY"));
        h.insert("referrer-policy", HeaderValue::from_static("same-origin"));
        h.insert("cross-origin-opener-policy", HeaderValue::from_static("same-origin"));
        let html = h.get(header::CONTENT_TYPE).and_then(|v| v.to_str().ok()).is_some_and(|t| t.starts_with("text/html"));
        if html && let Ok(v) = HeaderValue::from_str(csp) {
            h.insert(header::CONTENT_SECURITY_POLICY, v);
        }
    }
    res
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counters_limit_expire_and_stay_bounded() {
        let c = Counter::default();
        for _ in 0..3 {
            assert!(c.hit("a", 3, 60_000));
        }
        assert!(!c.hit("a", 3, 60_000), "the fourth is over the limit");
        assert!(c.over("a", 3));
        c.add("old", -1); // a window that has already ended
        assert!(!c.over("old", 1), "expired counts don't block");
        c.clear("a");
        assert!(!c.over("a", 1));
        for i in 0..MAX_KEYS + 10 {
            c.add(&format!("k{i}"), 60_000);
        }
        assert!(c.len() <= MAX_KEYS, "never grows past the cap");
    }

    #[test]
    fn forwarded_addresses_are_believed_only_from_a_private_proxy() {
        let mut h = HeaderMap::new();
        h.insert("x-forwarded-for", HeaderValue::from_static("6.6.6.6, 203.0.113.7"));
        let caddy: SocketAddr = "172.18.0.5:41000".parse().unwrap();
        let stranger: SocketAddr = "198.51.100.9:41000".parse().unwrap();
        assert_eq!(client_ip(Some(caddy), &h), "203.0.113.7", "the proxy's own addition, not the client's claim");
        assert_eq!(client_ip(Some(stranger), &h), "198.51.100.9", "a direct client can't choose its address");
        assert_eq!(client_ip(Some(caddy), &HeaderMap::new()), "172.18.0.5");
        assert_eq!(client_ip(None, &h), "unknown");
    }

    #[test]
    fn the_csp_allows_exactly_the_inline_theme_script() {
        let html = "<head><script>document.documentElement.dataset.theme = 'x';</script><script src=\"/assets/app.js\"></script>";
        let csp = dashboard_csp(html);
        assert_eq!(csp.matches("'sha256-").count(), 1);
        assert!(csp.contains("frame-ancestors 'none'") && !csp.contains("script-src 'self' 'unsafe-inline'"));
    }
}
