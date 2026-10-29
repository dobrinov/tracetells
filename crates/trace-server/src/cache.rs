//! The dashboard's heavy reports (Home, Audience, Features, Funnels, the
//! behaviour summary, KPIs), computed once and served from memory.
//!
//! Each one reads every event in its window, which on a busy website takes
//! seconds. A report younger than [`FRESH`] is served as is; an older one is
//! served at once and recomputed in the background; one nobody has asked for
//! in [`KEEP`] is dropped. A background task also refreshes the reports people
//! are looking at, so a page that stays open never waits. Any change made
//! through the API (a pushed trace.json, an insight moved, an analysis run)
//! drops the website's reports, so what someone just did shows at once.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::Value;

/// Served without recomputing.
pub const FRESH: Duration = Duration::from_secs(60);
/// Kept (and refreshed in the background) while someone asked for it this recently.
pub const KEEP: Duration = Duration::from_secs(15 * 60);

type Compute = Arc<dyn Fn() -> Pin<Box<dyn Future<Output = anyhow::Result<Value>> + Send>> + Send + Sync>;

struct Entry {
    site: i64,
    value: Value,
    computed: Instant,
    asked: Instant,
    refreshing: bool,
    compute: Compute,
}

#[derive(Default)]
pub struct Reports {
    entries: Mutex<HashMap<String, Entry>>,
}

impl Reports {
    /// The report under `key` for website `site`, computing it with `compute` when there's none yet.
    pub async fn get<F, Fut>(self: &Arc<Self>, site: i64, key: String, compute: F) -> anyhow::Result<Value>
    where
        F: Fn() -> Fut + Send + Sync + 'static,
        Fut: Future<Output = anyhow::Result<Value>> + Send + 'static,
    {
        let now = Instant::now();
        let stale = {
            let mut entries = self.entries.lock().unwrap();
            match entries.get_mut(&key) {
                Some(e) => {
                    e.asked = now;
                    let stale = now.duration_since(e.computed) >= FRESH && !e.refreshing;
                    if stale {
                        e.refreshing = true;
                    }
                    Some((e.value.clone(), stale))
                }
                None => None,
            }
        };
        match stale {
            Some((value, false)) => Ok(value),
            Some((value, true)) => {
                // Served as it is; the next request gets the recomputed one.
                let me = self.clone();
                tokio::spawn(async move { me.refresh(&key).await });
                Ok(value)
            }
            None => {
                let compute: Compute = Arc::new(move || Box::pin(compute()));
                let value = compute().await?;
                self.entries.lock().unwrap().insert(
                    key,
                    Entry { site, value: value.clone(), computed: Instant::now(), asked: now, refreshing: false, compute },
                );
                Ok(value)
            }
        }
    }

    async fn refresh(&self, key: &str) {
        let Some(compute) = self.entries.lock().unwrap().get(key).map(|e| e.compute.clone()) else { return };
        let result = compute().await;
        let mut entries = self.entries.lock().unwrap();
        if let Some(e) = entries.get_mut(key) {
            e.refreshing = false;
            match result {
                Ok(v) => {
                    e.value = v;
                    e.computed = Instant::now();
                }
                Err(err) => tracing::warn!(key, error = %err, "refreshing a report failed"),
            }
        }
    }

    /// Drops a website's reports: something about it just changed.
    pub fn forget_site(&self, site: i64) {
        self.entries.lock().unwrap().retain(|_, e| e.site != site);
    }

    /// Drops reports nobody asked for lately, and refreshes the rest that are
    /// getting old, one at a time so a busy server isn't swamped.
    pub async fn tend(&self) {
        let now = Instant::now();
        let due: Vec<String> = {
            let mut entries = self.entries.lock().unwrap();
            entries.retain(|_, e| now.duration_since(e.asked) < KEEP);
            entries
                .iter_mut()
                .filter(|(_, e)| !e.refreshing && now.duration_since(e.computed) >= FRESH - Duration::from_secs(10))
                .map(|(k, e)| {
                    e.refreshing = true;
                    k.clone()
                })
                .collect()
        };
        for key in due {
            self.refresh(&key).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[tokio::test]
    async fn computes_once_and_forgets_on_change() {
        let reports = Arc::new(Reports::default());
        let runs = Arc::new(AtomicUsize::new(0));
        let get = |reports: Arc<Reports>, runs: Arc<AtomicUsize>| async move {
            reports
                .get(1, "1:today".into(), move || {
                    let runs = runs.clone();
                    async move { Ok(Value::from(runs.fetch_add(1, Ordering::SeqCst) + 1)) }
                })
                .await
                .unwrap()
        };
        assert_eq!(get(reports.clone(), runs.clone()).await, 1);
        assert_eq!(get(reports.clone(), runs.clone()).await, 1, "fresh: served from memory");
        reports.forget_site(2);
        assert_eq!(get(reports.clone(), runs.clone()).await, 1, "another website's change doesn't matter");
        reports.forget_site(1);
        assert_eq!(get(reports.clone(), runs.clone()).await, 2, "after a change it's computed again");
    }
}
