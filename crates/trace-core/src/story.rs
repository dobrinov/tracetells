//! A page story: how people use one page, as aggregated patterns of action
//! rather than recordings of visits. Which elements they use, the orders of
//! action visits share, where they came from and went next, how far they
//! scroll and how long they stay.
//!
//! Every row covers at least [`MIN_SESSIONS`] sessions; smaller ones are only
//! counted as hidden. Nothing here describes one visit: no ids, no timestamps,
//! and times only as medians and quartiles.

use std::collections::{HashMap, HashSet};

use anyhow::Result;
use serde::Serialize;

use crate::Pool;
use crate::digest::Window;
use crate::util::{DAY_MS, now_ms};

/// Actions this close together are one step, in whatever order they arrived.
const BEAT_MS: i64 = 300;
/// Patterns are runs of this many steps (a single step only when people repeat it).
const RUN_STEPS: std::ops::RangeInclusive<usize> = 1..=4;

/// Rows from fewer sessions than this are hidden: ordered actions on one page
/// are more distinctive than a count, so the public view's stricter rule applies.
pub const MIN_SESSIONS: i64 = crate::public::MIN_SESSIONS;
/// The most recent sessions that viewed the page that a story reads.
pub const MAX_SESSIONS: i64 = 2000;
pub const MAX_PATTERNS: usize = 6;
pub const MAX_ELEMENTS: usize = 50;
/// A dead or rage click marks the click it follows within this long.
const FOLD_MS: i64 = 1000;

#[derive(Debug, Serialize)]
pub struct PageStory {
    pub path: String,
    pub window: Window,
    /// Sessions that viewed the page (at most [`MAX_SESSIONS`], the newest).
    pub sessions: i64,
    /// More sessions viewed the page than the story read.
    pub capped: bool,
    /// Under [`MIN_SESSIONS`] sessions: nothing but the count is given.
    pub too_little_data: bool,
    pub min_sessions: i64,
    pub elements: Vec<ElementUse>,
    pub hidden_elements: usize,
    pub patterns: Vec<Pattern>,
    /// Branches of the pattern tree from fewer than [`MIN_SESSIONS`] sessions.
    pub hidden_patterns: usize,
    /// Patterns with enough sessions beyond the [`MAX_PATTERNS`] shown.
    pub more_patterns: usize,
    /// Sessions that followed one of the shown patterns to its end.
    pub pattern_sessions: i64,
    pub arrivals: Vec<Neighbour>,
    pub hidden_arrivals: usize,
    pub exits: Vec<Neighbour>,
    pub hidden_exits: usize,
    pub scroll: Vec<ScrollDepth>,
    /// Seconds on the page per session.
    pub time_on_page: Option<Spread>,
    /// Seconds from arriving on the page to the first action.
    pub time_to_first_action: Option<Spread>,
    /// Dead clicks, rage clicks and errors on the page, as the behaviour summary counts them.
    pub problems: Vec<Problem>,
    /// Insights whose pages include this one.
    pub insights: Vec<InsightSummary>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Problem {
    /// dead_click, rage_click or error.
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selector: Option<String>,
    /// The element's label, or the error's message: visitor text.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// The error's kind (exception, rejection, console, network, resource).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_kind: Option<String>,
    pub events: i64,
    pub sessions: i64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct InsightSummary {
    pub id: i64,
    pub title: String,
    pub kind: String,
    pub confidence: String,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ElementUse {
    pub selector: String,
    /// The element's label as the collector scrubbed it: visitor text.
    pub text: Option<String>,
    pub sessions: i64,
    /// Of the page's sessions.
    pub share: f64,
    pub clicks_per_session: f64,
    /// Sessions in which a click on it did nothing (dead or rage).
    pub dead_sessions: i64,
    /// Of the sessions that used it.
    pub dead_share: f64,
}

/// A run of steps that at least [`MIN_SESSIONS`] sessions did, anywhere in a visit.
/// No steps: sessions that viewed the page and did nothing on it.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Pattern {
    pub sessions: i64,
    pub steps: Vec<Step>,
    /// A step in it did nothing in at least a quarter of its sessions.
    pub goes_wrong: bool,
    /// Most of its sessions did the whole run more than once in a visit.
    pub repeats: bool,
}

/// One step: an action, or several that happened at the same moment.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Step {
    pub actions: Vec<Action>,
    /// Most of the pattern's sessions did this step more than once in a row.
    pub repeated: bool,
    /// Of the pattern's sessions, how many clicked here and nothing happened.
    pub did_nothing_sessions: i64,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq, Hash)]
pub struct Action {
    /// click, form_start, submit, error or custom.
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selector: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// The custom event, or the error's kind.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// The page didn't react to the click.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub did_nothing: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub rage: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Neighbour {
    /// None: outside the website (arrived from elsewhere, or left it).
    pub path: Option<String>,
    pub sessions: i64,
    pub share: f64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ScrollDepth {
    pub depth: u32,
    pub share: f64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Spread {
    pub p25: f64,
    pub median: f64,
    pub p75: f64,
}

/// One stored event, as the builder needs it.
#[derive(Debug, Clone, Default)]
pub struct Ev {
    pub ts: i64,
    pub kind: String,
    pub path: Option<String>,
    pub selector: Option<String>,
    pub text: Option<String>,
    pub name: Option<String>,
    pub value: Option<f64>,
}

impl Ev {
    pub fn new(ts: i64, kind: &str, path: &str) -> Self {
        Ev { ts, kind: kind.into(), path: Some(path.into()), ..Default::default() }
    }
    pub fn sel(mut self, selector: &str, text: &str) -> Self {
        self.selector = Some(selector.into());
        self.text = (!text.is_empty()).then(|| text.into());
        self
    }
    pub fn name(mut self, name: &str) -> Self {
        self.name = Some(name.into());
        self
    }
    pub fn value(mut self, v: f64) -> Self {
        self.value = Some(v);
        self
    }
}

pub async fn build(pool: &Pool, site: i64, path: &str, days: u32) -> Result<PageStory> {
    let to = now_ms();
    let from = to - days as i64 * DAY_MS;
    let (viewed,): (i64,) = sqlx::query_as(
        "SELECT COUNT(DISTINCT session_id) FROM events WHERE project_id = $1 AND type = 'pageview' AND path = $2 AND ts >= $3 AND ts < $4",
    )
    .bind(site).bind(path).bind(from).bind(to)
    .fetch_one(pool).await?;
    let window = Window { from, to, days };
    let problems = problems(pool, site, path, from, to).await?;
    let insights = insights_about(pool, site, path).await?;
    if viewed < MIN_SESSIONS {
        return Ok(PageStory { problems, insights, ..PageStory::empty(path, window, viewed) }.visitor_text_on_one_line());
    }
    // The newest sessions that viewed the page, and every event they sent in the window.
    let rows = sqlx::query_as::<_, (String, i64, String, Option<String>, Option<String>, Option<String>, Option<String>, Option<f64>)>(
        "SELECT session_id, ts, type, path, selector, text, name, value FROM events
         WHERE project_id = $1 AND ts >= $3 AND ts < $4 AND session_id IN (
           SELECT session_id FROM (
             SELECT session_id, MAX(ts) AS last FROM events
             WHERE project_id = $1 AND type = 'pageview' AND path = $2 AND ts >= $3 AND ts < $4
             GROUP BY session_id ORDER BY last DESC LIMIT $5
           ) AS recent
         ) ORDER BY session_id, ts, id",
    )
    .bind(site).bind(path).bind(from).bind(to).bind(MAX_SESSIONS)
    .fetch_all(pool).await?;
    let mut sessions: Vec<Vec<Ev>> = Vec::new();
    let mut last: Option<String> = None;
    for (sid, ts, kind, path, selector, text, name, value) in rows {
        if last.as_deref() != Some(sid.as_str()) {
            sessions.push(Vec::new());
            last = Some(sid);
        }
        sessions.last_mut().unwrap().push(Ev { ts, kind, path, selector, text, name, value });
    }
    let mut story = assemble(path, window, &sessions);
    story.capped = viewed > MAX_SESSIONS;
    story.problems = problems;
    story.insights = insights;
    Ok(story.visitor_text_on_one_line())
}

impl PageStory {
    fn empty(path: &str, window: Window, sessions: i64) -> Self {
        PageStory {
            path: path.into(),
            window,
            sessions,
            capped: false,
            too_little_data: true,
            min_sessions: MIN_SESSIONS,
            elements: vec![],
            hidden_elements: 0,
            patterns: vec![],
            hidden_patterns: 0,
            more_patterns: 0,
            pattern_sessions: 0,
            arrivals: vec![],
            hidden_arrivals: 0,
            exits: vec![],
            hidden_exits: 0,
            scroll: vec![],
            time_on_page: None,
            time_to_first_action: None,
            problems: vec![],
            insights: vec![],
        }
    }

    /// Ingest already does this; doing it again covers events stored before it did.
    fn visitor_text_on_one_line(mut self) -> Self {
        use crate::privacy::{one_line, one_line_path};
        let opt = |s: &mut Option<String>| {
            if let Some(v) = s {
                *v = one_line(v)
            }
        };
        for e in &mut self.elements {
            e.selector = one_line(&e.selector);
            opt(&mut e.text);
        }
        for a in self.patterns.iter_mut().flat_map(|p| p.steps.iter_mut()).flat_map(|s| s.actions.iter_mut()) {
            opt(&mut a.selector);
            opt(&mut a.text);
            opt(&mut a.name);
        }
        for p in &mut self.problems {
            opt(&mut p.selector);
            opt(&mut p.text);
        }
        for i in &mut self.insights {
            i.title = one_line(&i.title);
        }
        for n in self.arrivals.iter_mut().chain(self.exits.iter_mut()) {
            if let Some(p) = &mut n.path {
                *p = one_line_path(p);
            }
        }
        self
    }
}

/// The page's dead clicks, rage clicks and errors, grouped as the behaviour summary groups them.
async fn problems(pool: &Pool, site: i64, path: &str, from: i64, to: i64) -> Result<Vec<Problem>> {
    let clicks = sqlx::query_as::<_, (String, Option<String>, Option<String>, i64, i64)>(
        "SELECT type, selector, MAX(text), COUNT(*), COUNT(DISTINCT session_id) FROM events
         WHERE project_id = $1 AND path = $2 AND type IN ('dead_click', 'rage_click') AND ts >= $3 AND ts < $4
         GROUP BY type, selector",
    )
    .bind(site).bind(path).bind(from).bind(to)
    .fetch_all(pool).await?;
    let errors = sqlx::query_as::<_, (String, String, i64, i64)>(
        "SELECT COALESCE(name, 'exception'), COALESCE(text, 'Unknown error'), COUNT(*), COUNT(DISTINCT session_id) FROM events
         WHERE project_id = $1 AND path = $2 AND type = 'error' AND ts >= $3 AND ts < $4
         GROUP BY 1, 2",
    )
    .bind(site).bind(path).bind(from).bind(to)
    .fetch_all(pool).await?;
    let mut out: Vec<Problem> = clicks
        .into_iter()
        .map(|(kind, selector, text, events, sessions)| Problem { kind, selector, text, error_kind: None, events, sessions })
        .chain(errors.into_iter().map(|(error_kind, text, events, sessions)| Problem { kind: "error".into(), selector: None, text: Some(text), error_kind: Some(error_kind), events, sessions }))
        .collect();
    out.sort_by(|a, b| b.sessions.cmp(&a.sessions).then(b.events.cmp(&a.events)).then(a.kind.cmp(&b.kind)).then(a.text.cmp(&b.text)));
    out.truncate(20);
    Ok(out)
}

/// Whether an insight's page (a path, or a pattern ending in `*`) covers `path`.
fn covers(page: &str, path: &str) -> bool {
    match page.strip_suffix('*') {
        Some(prefix) => path.starts_with(prefix),
        None => page.trim_end_matches('/') == path.trim_end_matches('/'),
    }
}

async fn insights_about(pool: &Pool, site: i64, path: &str) -> Result<Vec<InsightSummary>> {
    use crate::model::InsightStatus;
    let mut found: Vec<_> = crate::insights::list(pool, site, None).await?.into_iter().filter(|i| i.pages.iter().any(|p| covers(p, path))).collect();
    found.sort_by_key(|i| (i.status == InsightStatus::Dismissed, -i.updated_at));
    Ok(found
        .into_iter()
        .map(|i| InsightSummary { id: i.id, title: i.title, kind: i.kind.as_str().into(), confidence: i.confidence, status: i.status.as_str().into() })
        .collect())
}

/// A visit to the page: a run of a session's events on its path.
struct Visit<'a> {
    events: &'a [Ev],
    before: Option<&'a str>,
    after: Option<&'a str>,
}

fn visits<'a>(session: &'a [Ev], path: &str) -> Vec<Visit<'a>> {
    let on = |e: &Ev| e.path.as_deref() == Some(path);
    let mut out = Vec::new();
    let mut i = 0;
    while i < session.len() {
        if !on(&session[i]) {
            i += 1;
            continue;
        }
        let start = i;
        while i < session.len() && on(&session[i]) {
            i += 1;
        }
        let before = start.checked_sub(1).and_then(|b| session[b].path.as_deref());
        let after = session.get(i).and_then(|e| e.path.as_deref());
        out.push(Visit { events: &session[start..i], before, after });
    }
    out
}

/// The visit's actions in order, each dead or rage click marking the click it follows.
fn actions(visit: &[Ev]) -> Vec<(Action, i64)> {
    let mut out: Vec<(Action, i64)> = Vec::new();
    for e in visit {
        match e.kind.as_str() {
            "click" => out.push((Action { kind: "click".into(), selector: e.selector.clone(), text: e.text.clone(), name: None, did_nothing: false, rage: false }, e.ts)),
            "dead_click" | "rage_click" => {
                let rage = e.kind == "rage_click";
                let marked = out.iter_mut().rev().find(|(a, ts)| a.kind == "click" && a.selector == e.selector && e.ts - *ts <= FOLD_MS && e.ts >= *ts);
                match marked {
                    Some((a, _)) => {
                        a.did_nothing = true;
                        a.rage |= rage;
                    }
                    None => out.push((Action { kind: "click".into(), selector: e.selector.clone(), text: e.text.clone(), name: None, did_nothing: true, rage }, e.ts)),
                }
            }
            "form_start" | "submit" => out.push((Action { kind: e.kind.clone(), selector: e.selector.clone(), text: None, name: None, did_nothing: false, rage: false }, e.ts)),
            "error" => out.push((Action { kind: "error".into(), selector: None, text: None, name: Some(e.name.clone().unwrap_or_else(|| "exception".into())), did_nothing: false, rage: false }, e.ts)),
            "custom" => out.push((Action { kind: "custom".into(), selector: None, text: None, name: e.name.clone(), did_nothing: false, rage: false }, e.ts)),
            _ => {}
        }
    }
    out
}

/// A step of one visit: its actions, whether the visit repeated it, and whether a click in it did nothing.
#[derive(Debug, Clone, PartialEq)]
pub struct RawStep {
    pub actions: Vec<Action>,
    pub repeated: bool,
    pub did_nothing: bool,
}

/// An action as a step knows it: whether it did nothing is counted, not part of what it is.
fn plain(a: &Action) -> String {
    format!("{}\u{1}{:?}\u{1}{:?}\u{1}{:?}", a.kind, a.selector, a.text, a.name)
}

fn step_key(actions: &[Action]) -> String {
    actions.iter().map(plain).collect::<Vec<_>>().join("\u{3}")
}

/// The visit's steps: actions within [`BEAT_MS`] of each other become one
/// step (in a fixed order), and a step done again right away becomes one repeated step.
pub fn steps(actions: &[(Action, i64)]) -> Vec<RawStep> {
    let mut beats: Vec<(i64, Vec<Action>)> = Vec::new();
    for (a, ts) in actions {
        match beats.last_mut() {
            Some((start, acts)) if ts - *start <= BEAT_MS => acts.push(a.clone()),
            _ => beats.push((*ts, vec![a.clone()])),
        }
    }
    let mut out: Vec<RawStep> = Vec::new();
    for (_, mut acts) in beats {
        acts.sort_by_key(plain);
        // The same action twice at one moment (a double or rage tap) is a repeat.
        let before = acts.len();
        acts.dedup_by(|later, kept| {
            let same = plain(later) == plain(kept);
            if same {
                kept.did_nothing |= later.did_nothing;
                kept.rage |= later.rage;
            }
            same
        });
        let burst = acts.len() < before;
        let dead = acts.iter().any(|a| a.did_nothing);
        match out.last_mut() {
            Some(s) if step_key(&s.actions) == step_key(&acts) => {
                s.repeated = true;
                s.did_nothing |= dead;
            }
            _ => out.push(RawStep { actions: acts, repeated: burst, did_nothing: dead }),
        }
    }
    out
}

/// Every run of [`RUN_STEPS`] steps the visits did, by what it is.
struct Run {
    steps: Vec<RawStep>,
    keys: Vec<String>,
    sessions: HashSet<usize>,
    /// Sessions that did the whole run twice or more in one visit.
    again: HashSet<usize>,
    /// Per step: sessions where it did nothing, and where it was repeated.
    dead: Vec<HashSet<usize>>,
    repeated: Vec<HashSet<usize>>,
    /// Per session, the earliest step it started at.
    first: HashMap<usize, usize>,
}

impl Run {
    fn n(&self) -> i64 {
        n(&self.sessions)
    }
    fn earliest(&self) -> f64 {
        self.first.values().sum::<usize>() as f64 / self.first.len().max(1) as f64
    }
}

fn runs(visits: &[(usize, Vec<RawStep>)]) -> Vec<Run> {
    let mut found: HashMap<String, Run> = HashMap::new();
    for (si, st) in visits {
        let keys: Vec<String> = st.iter().map(|s| step_key(&s.actions)).collect();
        let mut seen: HashMap<String, usize> = HashMap::new();
        for len in RUN_STEPS {
            if st.len() < len {
                break;
            }
            for i in 0..=st.len() - len {
                // A single step counts only where the visit repeated it.
                if len == 1 && !st[i].repeated {
                    continue;
                }
                let k = keys[i..i + len].join("\u{2}");
                let r = found.entry(k.clone()).or_insert_with(|| Run {
                    steps: st[i..i + len].to_vec(),
                    keys: keys[i..i + len].to_vec(),
                    sessions: HashSet::new(),
                    again: HashSet::new(),
                    dead: vec![HashSet::new(); len],
                    repeated: vec![HashSet::new(); len],
                    first: HashMap::new(),
                });
                r.sessions.insert(*si);
                let at = r.first.entry(*si).or_insert(i);
                *at = (*at).min(i);
                for (p, step) in st[i..i + len].iter().enumerate() {
                    if step.did_nothing {
                        r.dead[p].insert(*si);
                    }
                    if step.repeated {
                        r.repeated[p].insert(*si);
                    }
                }
                let times = seen.entry(k).or_insert(0);
                *times += 1;
                if *times >= 2 {
                    r.again.insert(*si);
                }
            }
        }
    }
    found.into_values().collect()
}

/// Whether `a` covers at least 80% of `b`'s sessions.
fn nearly(a: &Run, b: &Run) -> bool {
    a.n() * 5 >= b.n() * 4
}

/// The runs worth showing, best first, and how many 2-step runs were too rare to show.
fn patterns(visits: &[(usize, Vec<RawStep>)]) -> (Vec<(Pattern, HashSet<usize>)>, usize) {
    let all = runs(visits);
    let hidden = all.iter().filter(|r| r.keys.len() == 2 && r.n() >= 2 && r.n() < MIN_SESSIONS).count();
    // A run that starts and ends on the same step is a loop seen from its joint.
    // Single steps were only counted where repeated (see `runs`).
    let cands: Vec<&Run> = all.iter().filter(|r| r.n() >= MIN_SESSIONS && !(r.keys.len() > 2 && r.keys[0] == r.keys[r.keys.len() - 1])).collect();
    let inside = |big: &Run, small: &Run| big.keys.len() > small.keys.len() && big.keys.windows(small.keys.len()).any(|w| w == small.keys.as_slice());
    let maximal: Vec<&Run> = cands.iter().copied().filter(|r| !cands.iter().any(|b| inside(b, r) && nearly(b, r))).collect();
    let rotation = |a: &Run, b: &Run| {
        a.keys.len() == b.keys.len() && a.keys != b.keys && (1..a.keys.len()).any(|s| a.keys[s..].iter().chain(&a.keys[..s]).eq(b.keys.iter()))
    };
    // Of a loop's rotations, the one visits reach first.
    let kept: Vec<&Run> = maximal
        .iter()
        .enumerate()
        .filter(|(i, r)| {
            !maximal.iter().enumerate().any(|(j, o)| {
                rotation(r, o) && nearly(o, r) && nearly(r, o) && (o.earliest() < r.earliest() || (o.earliest() == r.earliest() && j < *i))
            })
        })
        .map(|(_, r)| *r)
        .collect();
    let mut out: Vec<(Pattern, HashSet<usize>)> = kept
        .into_iter()
        .map(|r| {
            let total = r.n();
            let steps: Vec<Step> = r
                .steps
                .iter()
                .enumerate()
                .map(|(p, s)| Step {
                    actions: s.actions.iter().map(|a| Action { did_nothing: false, rage: false, ..a.clone() }).collect(),
                    repeated: 2 * n(&r.repeated[p]) > total,
                    did_nothing_sessions: n(&r.dead[p]),
                })
                .collect();
            let goes_wrong = steps.iter().any(|s| s.did_nothing_sessions * 4 >= total && s.did_nothing_sessions > 0);
            (Pattern { sessions: total, steps, goes_wrong, repeats: 2 * n(&r.again) > total }, r.sessions.clone())
        })
        .collect();
    out.sort_by(|a, b| {
        b.0.goes_wrong.cmp(&a.0.goes_wrong).then(b.0.sessions.cmp(&a.0.sessions)).then(b.0.steps.len().cmp(&a.0.steps.len())).then_with(|| format!("{:?}", a.0.steps).cmp(&format!("{:?}", b.0.steps)))
    });
    (out, hidden)
}

fn n(set: &HashSet<usize>) -> i64 {
    set.len() as i64
}

fn quantile(sorted: &[f64], q: f64) -> f64 {
    let at = q * (sorted.len() - 1) as f64;
    let (lo, hi) = (at.floor() as usize, at.ceil() as usize);
    let v = sorted[lo] + (sorted[hi] - sorted[lo]) * (at - lo as f64);
    (v * 10.0).round() / 10.0
}

fn spread(mut values: Vec<f64>) -> Option<Spread> {
    if (values.len() as i64) < MIN_SESSIONS {
        return None;
    }
    values.sort_by(|a, b| a.total_cmp(b));
    Some(Spread { p25: quantile(&values, 0.25), median: quantile(&values, 0.5), p75: quantile(&values, 0.75) })
}

fn share(part: i64, whole: i64) -> f64 {
    if whole == 0 { 0.0 } else { ((part as f64 / whole as f64) * 1000.0).round() / 1000.0 }
}

/// Builds the story from each session's events in time order.
pub fn assemble(path: &str, window: Window, sessions: &[Vec<Ev>]) -> PageStory {
    let per_session: Vec<Vec<Visit>> = sessions.iter().map(|s| visits(s, path)).collect();
    let total = per_session.iter().filter(|v| !v.is_empty()).count() as i64;
    if total < MIN_SESSIONS {
        return PageStory::empty(path, window, total);
    }
    let mut story = PageStory { too_little_data: false, ..PageStory::empty(path, window, total) };

    // Elements: a (selector, label) pair stands alone with enough sessions, else folds into its selector.
    let mut pair_sessions: HashMap<(Option<String>, Option<String>), HashSet<usize>> = HashMap::new();
    let mut pair_clicks: HashMap<(Option<String>, Option<String>), i64> = HashMap::new();
    for (si, vs) in per_session.iter().enumerate() {
        for e in vs.iter().flat_map(|v| v.events).filter(|e| e.kind == "click") {
            let k = (e.selector.clone(), e.text.clone());
            pair_sessions.entry(k.clone()).or_default().insert(si);
            *pair_clicks.entry(k).or_default() += 1;
        }
    }
    let stands = |sel: &Option<String>, text: &Option<String>| pair_sessions.get(&(sel.clone(), text.clone())).is_some_and(|s| n(s) >= MIN_SESSIONS);
    // The label a folded selector shows: its most clicked one.
    let mut folded_label: HashMap<Option<String>, (i64, Option<String>)> = HashMap::new();
    for ((sel, text), clicks) in &pair_clicks {
        if !stands(sel, text) {
            let best = folded_label.entry(sel.clone()).or_insert((0, None));
            if *clicks > best.0 || (*clicks == best.0 && text < &best.1) {
                *best = (*clicks, text.clone());
            }
        }
    }
    let element_of = |sel: &Option<String>, text: &Option<String>| -> (Option<String>, Option<String>) {
        if stands(sel, text) { (sel.clone(), text.clone()) } else { (sel.clone(), folded_label.get(sel).and_then(|(_, t)| t.clone())) }
    };

    #[derive(Default)]
    struct Use {
        clicks: HashMap<usize, i64>,
        dead: HashSet<usize>,
    }
    let mut uses: HashMap<(Option<String>, Option<String>), Use> = HashMap::new();
    let mut visit_steps: Vec<(usize, Vec<RawStep>)> = Vec::new();
    let mut acted: HashSet<usize> = HashSet::new();
    let mut arrivals: HashMap<Option<String>, HashSet<usize>> = HashMap::new();
    let mut exits: HashMap<Option<String>, HashSet<usize>> = HashMap::new();
    let mut scroll: Vec<f64> = Vec::new();
    let mut time_on: Vec<f64> = Vec::new();
    let mut first_action: Vec<f64> = Vec::new();

    for (si, vs) in per_session.iter().enumerate() {
        if vs.is_empty() {
            continue;
        }
        let (mut max_scroll, mut on_ms, mut first) = (0.0f64, None::<f64>, None::<f64>);
        for v in vs {
            arrivals.entry(v.before.map(String::from)).or_default().insert(si);
            exits.entry(v.after.map(String::from)).or_default().insert(si);
            for e in v.events {
                match e.kind.as_str() {
                    "scroll" => max_scroll = max_scroll.max(e.value.unwrap_or(0.0)),
                    "leave" => on_ms = Some(on_ms.unwrap_or(0.0) + e.value.unwrap_or(0.0)),
                    "click" | "dead_click" | "rage_click" => {
                        let u = uses.entry(element_of(&e.selector, &e.text)).or_default();
                        if e.kind == "click" {
                            *u.clicks.entry(si).or_default() += 1;
                        } else {
                            u.dead.insert(si);
                        }
                    }
                    _ => {}
                }
            }
            let mut acts = actions(v.events);
            if first.is_none()
                && let Some((_, ts)) = acts.first()
            {
                first = Some((ts - v.events[0].ts) as f64 / 1000.0);
            }
            for (a, _) in &mut acts {
                if a.kind == "click" {
                    (a.selector, a.text) = element_of(&a.selector, &a.text);
                }
            }
            if !acts.is_empty() {
                acted.insert(si);
            }
            visit_steps.push((si, steps(&acts)));
        }
        scroll.push(max_scroll);
        if let Some(ms) = on_ms {
            time_on.push(ms / 1000.0);
        }
        if let Some(s) = first {
            first_action.push(s.max(0.0));
        }
    }

    // Elements.
    let mut elements: Vec<ElementUse> = Vec::new();
    for ((selector, text), u) in uses {
        let users: HashSet<usize> = u.clicks.keys().copied().chain(u.dead.iter().copied()).collect();
        if n(&users) < MIN_SESSIONS {
            story.hidden_elements += 1;
            continue;
        }
        let mut per: Vec<f64> = users.iter().map(|s| *u.clicks.get(s).unwrap_or(&0) as f64).collect();
        per.sort_by(|a, b| a.total_cmp(b));
        elements.push(ElementUse {
            selector: selector.unwrap_or_default(),
            text,
            sessions: n(&users),
            share: share(n(&users), total),
            clicks_per_session: quantile(&per, 0.5),
            dead_sessions: n(&u.dead),
            dead_share: share(n(&u.dead), n(&users)),
        });
    }
    elements.sort_by(|a, b| b.sessions.cmp(&a.sessions).then(b.dead_sessions.cmp(&a.dead_sessions)).then(a.selector.cmp(&b.selector)).then(a.text.cmp(&b.text)));
    elements.truncate(MAX_ELEMENTS);
    story.elements = elements;

    // Patterns. Sessions that did nothing on the page are a pattern of their own.
    let (mut found, hidden) = patterns(&visit_steps);
    let idle: HashSet<usize> = per_session.iter().enumerate().filter(|(si, v)| !v.is_empty() && !acted.contains(si)).map(|(si, _)| si).collect();
    if n(&idle) >= MIN_SESSIONS {
        let at = found.iter().position(|(p, _)| !p.goes_wrong && p.sessions < n(&idle)).unwrap_or(found.len());
        found.insert(at, (Pattern { sessions: n(&idle), steps: vec![], goes_wrong: false, repeats: false }, idle));
    }
    story.more_patterns = found.len().saturating_sub(MAX_PATTERNS);
    found.truncate(MAX_PATTERNS);
    story.pattern_sessions = found.iter().flat_map(|(_, s)| s.iter().copied()).collect::<HashSet<_>>().len() as i64;
    story.hidden_patterns = hidden;
    story.patterns = found.into_iter().map(|(p, _)| p).collect();

    // Arrivals and exits.
    let neighbours = |m: HashMap<Option<String>, HashSet<usize>>| {
        let mut hidden = 0;
        let mut rows: Vec<Neighbour> = m
            .into_iter()
            .filter(|(_, s)| {
                let keep = n(s) >= MIN_SESSIONS;
                hidden += usize::from(!keep);
                keep
            })
            .map(|(path, s)| Neighbour { path, sessions: n(&s), share: share(n(&s), total) })
            .collect();
        rows.sort_by(|a, b| b.sessions.cmp(&a.sessions).then(a.path.cmp(&b.path)));
        rows.truncate(25);
        (rows, hidden)
    };
    (story.arrivals, story.hidden_arrivals) = neighbours(arrivals);
    (story.exits, story.hidden_exits) = neighbours(exits);

    story.scroll = [25, 50, 75, 100].iter().map(|&d| ScrollDepth { depth: d, share: share(scroll.iter().filter(|&&s| s >= d as f64).count() as i64, total) }).collect();
    story.time_on_page = spread(time_on);
    story.time_to_first_action = spread(first_action);
    story
}

#[cfg(test)]
mod tests {
    use super::*;

    fn window() -> Window {
        Window { from: 0, to: 1, days: 7 }
    }

    const Q: &str = "/questions/:id";

    fn click(sel: &str, text: &str) -> Action {
        Action { kind: "click".into(), selector: Some(sel.into()), text: (!text.is_empty()).then(|| text.into()), name: None, did_nothing: false, rage: false }
    }

    /// A session viewing the page and doing `acts` (selector, text, dead), a second apart.
    fn session(acts: &[(&str, &str, bool)]) -> Vec<Ev> {
        let mut out = vec![Ev::new(0, "pageview", Q)];
        for (i, (sel, text, dead)) in acts.iter().enumerate() {
            let t = (i as i64 + 1) * 2000;
            out.push(Ev::new(t, "click", Q).sel(sel, text));
            if *dead {
                out.push(Ev::new(t, "dead_click", Q).sel(sel, text));
            }
        }
        out
    }

    #[test]
    fn dead_clicks_mark_the_click_they_follow() {
        let visit = vec![
            Ev::new(0, "pageview", Q),
            Ev::new(1000, "click", Q).sel("button.sq", ""),
            Ev::new(1000, "dead_click", Q).sel("button.sq", ""),
            Ev::new(1500, "scroll", Q).value(50.0),
            Ev::new(3000, "click", Q).sel("button.sq", ""),
            Ev::new(9000, "leave", Q).value(9000.0),
        ];
        let a: Vec<Action> = actions(&visit).into_iter().map(|(a, _)| a).collect();
        assert_eq!(a.len(), 2, "scroll, leave and pageview are not steps; the dead click is not a step of its own");
        assert!(a[0].did_nothing);
        assert!(!a[1].did_nothing);
    }

    fn at(a: Action, ts: i64) -> (Action, i64) {
        (a, ts)
    }

    #[test]
    fn steps_fold_repeats_and_same_moment_actions() {
        let (a, b) = (click("a", ""), click("b", ""));
        let dead_a = Action { did_nothing: true, ..a.clone() };
        let names = |st: &[RawStep]| st.iter().map(|s| (s.actions.iter().map(|x| x.selector.clone().unwrap_or_else(|| x.name.clone().unwrap())).collect::<Vec<_>>().join("+"), s.repeated, s.did_nothing)).collect::<Vec<_>>();
        assert_eq!(names(&steps(&[at(dead_a.clone(), 0), at(a.clone(), 2000), at(a.clone(), 4000), at(b.clone(), 6000)])), [("a".into(), true, true), ("b".into(), false, false)], "a tap that did nothing and one that worked are one repeated step");
        let submit = Action { kind: "submit".into(), selector: Some("form".into()), text: None, name: None, did_nothing: false, rage: false };
        let answered = Action { kind: "custom".into(), selector: None, text: None, name: Some("problem_answered".into()), did_nothing: false, rage: false };
        let one = steps(&[at(submit.clone(), 0), at(answered.clone(), 120)]);
        let other = steps(&[at(answered.clone(), 0), at(submit.clone(), 80)]);
        assert_eq!(one.len(), 1, "within 300 ms: one step");
        assert_eq!(one, other, "in either order");
        assert_eq!(steps(&[at(submit.clone(), 0), at(answered, 900)]).len(), 2, "further apart: two steps");
        let burst = steps(&[at(a.clone(), 0), at(a.clone(), 150), at(Action { rage: true, did_nothing: true, ..a.clone() }, 280)]);
        assert_eq!((burst.len(), burst[0].repeated, burst[0].did_nothing), (1, true, true), "a rage burst is one repeated step that did nothing");
    }

    fn seq(p: &Pattern) -> Vec<String> {
        p.steps.iter().map(|s| s.actions.iter().map(|a| a.selector.clone().unwrap_or_default()).collect::<Vec<_>>().join("+")).collect()
    }

    #[test]
    fn patterns_from_the_questions_page() {
        let mut sessions = Vec::new();
        // 10 sessions answer and move on, several times.
        let round = [("div.pad > button", "#", false), ("input.primary", "", false), ("a.next", "Next", false)];
        for _ in 0..10 {
            sessions.push(session(&[round, round, round].concat()));
        }
        // 7 tap an answer square that doesn't respond, tap it again, then check: after reading the help first.
        for i in 0..7 {
            let lead: &[(&str, &str, bool)] = if i % 2 == 0 { &[("a.help", "Help", false)] } else { &[] };
            sessions.push(session(&[lead, &[("button.sq", "", true), ("button.sq", "", false), ("input.primary", "", false)]].concat()));
        }
        // 3 open the scratch paper and tap the writing field, 2 have the problem read aloud: too few.
        for _ in 0..3 {
            sessions.push(session(&[("button.scratch", "Scratch paper", false), ("div.field", "Writing field", true)]));
        }
        for _ in 0..2 {
            sessions.push(session(&[("button.read", "Read to me", false)]));
        }
        let s = assemble(Q, window(), &sessions);
        assert_eq!(s.sessions, 22);
        let found: Vec<(Vec<String>, i64)> = s.patterns.iter().map(|p| (seq(p), p.sessions)).collect();
        assert_eq!(found, [(vec!["button.sq".to_string(), "input.primary".into()], 7), (vec!["div.pad > button".to_string(), "input.primary".into(), "a.next".into()], 10)], "{:#?}", s.patterns);
        let twice = &s.patterns[0];
        assert!(twice.goes_wrong, "where it goes wrong comes first");
        assert!(twice.steps[0].repeated);
        assert_eq!(twice.steps[0].did_nothing_sessions, 7);
        let answer = &s.patterns[1];
        assert!(answer.repeats && !answer.goes_wrong, "done again and again; no rotation of it is listed too");
        assert_eq!(s.hidden_patterns, 2, "help → square (4 sessions) and scratch paper → writing field (3) are hidden");
        assert_eq!(s.pattern_sessions, 17);
        assert!(s.elements.iter().all(|e| e.sessions >= MIN_SESSIONS));
        assert_eq!(s.hidden_elements, 4, "help (4 sessions), scratch paper, writing field, read aloud");
    }

    #[test]
    fn an_element_that_often_ignores_taps() {
        let mut sessions = Vec::new();
        for i in 0..23 {
            if i < 10 {
                sessions.push(session(&[("button.sq", "", true), ("button.sq", "", false)]));
            } else if i < 11 {
                sessions.push(session(&[("button.sq", "", false)]));
            } else {
                sessions.push(session(&[("a.next", "Next", false)]));
            }
        }
        let s = assemble(Q, window(), &sessions);
        let sq = s.elements.iter().find(|e| e.selector == "button.sq").unwrap();
        assert_eq!((sq.sessions, sq.share, sq.dead_sessions, sq.dead_share), (11, 0.478, 10, 0.909));
        assert_eq!(sq.clicks_per_session, 2.0);
    }

    #[test]
    fn a_repeated_tap_is_a_pattern_of_its_own() {
        // 150 pay once; 55 tap Pay now again and again and the page ignores them.
        let mut sessions: Vec<Vec<Ev>> = (0..150).map(|_| session(&[("button#pay", "Pay now", false)])).collect();
        sessions.extend((0..55).map(|_| session(&[("button#pay", "Pay now", false), ("button#pay", "Pay now", false), ("button#pay", "Pay now", true)])));
        let s = assemble(Q, window(), &sessions);
        let p = &s.patterns[0];
        assert_eq!((seq(p), p.sessions, p.goes_wrong, p.steps[0].repeated, p.steps[0].did_nothing_sessions), (vec!["button#pay".to_string()], 55, true, true, 55));
    }

    #[test]
    fn a_run_inside_a_longer_one_is_not_listed_twice() {
        let mut sessions: Vec<Vec<Ev>> = (0..12).map(|_| session(&[("a.cart", "Cart", false), ("a.pay", "Pay", false), ("a.done", "Done", false)])).collect();
        sessions.extend((0..6).map(|_| session(&[("a.cart", "Cart", false), ("a.pay", "Pay", false)])));
        let s = assemble(Q, window(), &sessions);
        let found: Vec<(Vec<String>, i64)> = s.patterns.iter().map(|p| (seq(p), p.sessions)).collect();
        // Cart → Pay (18) is not nearly covered by Cart → Pay → Done (12), so both show; Pay → Done (12) is.
        assert_eq!(found, [(vec!["a.cart".to_string(), "a.pay".into()], 18), (vec!["a.cart".to_string(), "a.pay".into(), "a.done".into()], 12)]);
    }

    #[test]
    fn labels_stand_alone_only_with_enough_sessions() {
        let mut sessions = Vec::new();
        for i in 0..12 {
            let text = if i < 6 { "Save" } else { ["One", "Two", "Three", "Four", "Five", "Six"][i - 6] };
            sessions.push(session(&[("button.b", text, false)]));
        }
        let s = assemble(Q, window(), &sessions);
        let mut rows: Vec<(Option<String>, i64)> = s.elements.iter().map(|e| (e.text.clone(), e.sessions)).collect();
        rows.sort();
        assert_eq!(rows.len(), 2, "{rows:?}");
        assert!(rows.contains(&(Some("Save".into()), 6)));
        assert!(rows.iter().any(|(t, n)| *n == 6 && t.as_deref() != Some("Save")), "the rare labels fold into one element");
    }

    #[test]
    fn arrivals_and_exits() {
        let mut sessions = Vec::new();
        for i in 0..98 {
            let mut s = vec![Ev::new(0, "pageview", "/competition"), Ev::new(1000, "leave", "/competition").value(30_000.0)];
            if i >= 79 {
                s.push(Ev::new(2000, "pageview", "/sign-up"));
            }
            sessions.push(s);
        }
        let s = assemble("/competition", window(), &sessions);
        assert_eq!(s.arrivals, [Neighbour { path: None, sessions: 98, share: 1.0 }]);
        assert_eq!(s.exits[0], Neighbour { path: None, sessions: 79, share: 0.806 });
        assert_eq!(s.exits[1].path.as_deref(), Some("/sign-up"));
        assert_eq!(s.time_on_page.as_ref().map(|t| t.median), Some(30.0));
        assert_eq!(s.patterns, [Pattern { sessions: 98, steps: vec![], goes_wrong: false, repeats: false }], "visits that did nothing are a pattern");
    }

    #[test]
    fn a_quiet_page_has_too_little_data() {
        let sessions: Vec<Vec<Ev>> = (0..3).map(|_| session(&[("a.next", "Next", false)])).collect();
        let s = assemble(Q, window(), &sessions);
        assert!(s.too_little_data);
        assert_eq!(s.sessions, 3);
        assert!(s.elements.is_empty() && s.patterns.is_empty() && s.arrivals.is_empty() && s.exits.is_empty());
    }

    #[test]
    fn scroll_and_time_to_first_action() {
        let sessions: Vec<Vec<Ev>> = (0..8)
            .map(|i| {
                let mut s = session(&[("a.next", "Next", false)]);
                s.push(Ev::new(5000, "scroll", Q).value(if i < 6 { 100.0 } else { 40.0 }));
                s
            })
            .collect();
        let s = assemble(Q, window(), &sessions);
        assert_eq!(s.scroll.iter().map(|d| d.share).collect::<Vec<_>>(), [1.0, 0.75, 0.75, 0.75]);
        assert_eq!(s.time_to_first_action.as_ref().map(|t| t.median), Some(2.0));
    }

    #[tokio::test]
    async fn stories_never_name_a_session() {
        let target = crate::db::test_target("story_private").await;
        let pool = crate::db::connect(&target).await.unwrap();
        let (_, site) = crate::db::create_account_with_site(&pool, "Shop", None).await.unwrap();
        let t = now_ms() - DAY_MS;
        for i in 0..7 {
            let id = format!("sess-secret-{i}");
            sqlx::query("INSERT INTO sessions (id, project_id, visitor_id, user_id, started_at, last_seen_at, device, user_agent) VALUES ($1, $2, 'vis-secret', 'user-secret', $3, $3, 'mobile', 'UA-secret')")
                .bind(&id).bind(site.id).bind(t).execute(&pool).await.unwrap();
            for (k, (kind, sel)) in [("pageview", ""), ("click", "button.sq"), ("dead_click", "button.sq"), ("leave", "")].iter().enumerate() {
                sqlx::query("INSERT INTO events (project_id, session_id, ts, type, path, selector, value) VALUES ($1, $2, $3, $4, $5, $6, $7)")
                    .bind(site.id).bind(&id).bind(t + 123_457 + k as i64).bind(*kind).bind(Q).bind((!sel.is_empty()).then_some(*sel)).bind(Some(4000.0))
                    .execute(&pool).await.unwrap();
            }
        }
        let story = build(&pool, site.id, Q, 7).await.unwrap();
        assert_eq!(story.sessions, 7);
        assert!(!story.too_little_data);
        let json = serde_json::to_string(&story).unwrap();
        for secret in ["sess-secret", "vis-secret", "user-secret", "UA-secret", &(t + 123_457).to_string()] {
            assert!(!json.contains(secret), "{secret} leaked: {json}");
        }
        let quiet = build(&pool, site.id, "/nowhere", 7).await.unwrap();
        assert!(quiet.too_little_data);
        crate::db::drop_test_target(&target).await;
    }

    #[tokio::test]
    async fn problems_and_insights_of_a_page() {
        let target = crate::db::test_target("story_problems").await;
        let pool = crate::db::connect(&target).await.unwrap();
        let (_, site) = crate::db::create_account_with_site(&pool, "Kids", None).await.unwrap();
        let t = now_ms() - DAY_MS;
        let duel = "/challenges/:id";
        for (sid, n) in [("a", 37), ("b", 2)] {
            sqlx::query("INSERT INTO events (project_id, session_id, ts, type, path) VALUES ($1, $2, $3, 'pageview', $4)").bind(site.id).bind(sid).bind(t).bind(duel).execute(&pool).await.unwrap();
            for k in 0..n {
                sqlx::query("INSERT INTO events (project_id, session_id, ts, type, path, name, text) VALUES ($1, $2, $3, 'error', $4, 'rejection', 'Missing target element yourScore')")
                    .bind(site.id).bind(sid).bind(t + 1 + k).bind(duel).execute(&pool).await.unwrap();
            }
        }
        sqlx::query("INSERT INTO events (project_id, session_id, ts, type, path, selector, text) VALUES ($1, 'b', $2, 'dead_click', $3, 'button.ready', 'Ready')")
            .bind(site.id).bind(t + 100).bind(duel).execute(&pool).await.unwrap();
        let insight = |title: &str, pages: &[&str]| crate::model::NewInsight {
            kind: Default::default(),
            title: title.into(),
            problem: "p".into(),
            evidence: vec![],
            proposal: "p".into(),
            acceptance_criteria: vec![],
            pages: pages.iter().map(|p| p.to_string()).collect(),
            kpi_id: None,
            expected_impact: String::new(),
            confidence: "high".into(),
            source: "agent".into(),
            fingerprint: None,
            created_by: None,
            readings: vec![],
        };
        crate::insights::insert(&pool, site.id, &insight("Stop the duel waiting room throwing on every poll", &[duel])).await.unwrap();
        crate::insights::insert(&pool, site.id, &insight("Battles in general", &["/challenges*"])).await.unwrap();
        crate::insights::insert(&pool, site.id, &insight("Faster sign-in", &["/sign-in"])).await.unwrap();

        let story = build(&pool, site.id, duel, 7).await.unwrap();
        assert!(story.too_little_data, "2 sessions are too few for patterns, but problems and insights still show");
        assert_eq!(story.problems[0], Problem {
            kind: "error".into(), selector: None, text: Some("Missing target element yourScore".into()), error_kind: Some("rejection".into()), events: 39, sessions: 2,
        });
        assert_eq!(story.problems[1].selector.as_deref(), Some("button.ready"));
        let titles: Vec<&str> = story.insights.iter().map(|i| i.title.as_str()).collect();
        assert_eq!(titles.len(), 2);
        assert!(titles.contains(&"Stop the duel waiting room throwing on every poll") && titles.contains(&"Battles in general"));
        assert_eq!(story.insights[0].status, "new");
        let other = build(&pool, site.id, "/sign-in", 7).await.unwrap();
        assert_eq!(other.insights.iter().map(|i| i.title.as_str()).collect::<Vec<_>>(), ["Faster sign-in"]);
        assert!(other.problems.is_empty());
        crate::db::drop_test_target(&target).await;
    }

    #[tokio::test]
    async fn busy_pages_read_the_newest_sessions() {
        let target = crate::db::test_target("story_capped").await;
        let pool = crate::db::connect(&target).await.unwrap();
        let (_, site) = crate::db::create_account_with_site(&pool, "Shop", None).await.unwrap();
        let t = now_ms() - DAY_MS;
        let mut tx = pool.begin().await.unwrap();
        for i in 0..(MAX_SESSIONS + 100) {
            sqlx::query("INSERT INTO events (project_id, session_id, ts, type, path) VALUES ($1, $2, $3, 'pageview', $4)")
                .bind(site.id).bind(format!("s{i}")).bind(t + i).bind(Q).execute(&mut *tx).await.unwrap();
        }
        tx.commit().await.unwrap();
        let story = build(&pool, site.id, Q, 7).await.unwrap();
        assert!(story.capped);
        assert_eq!(story.sessions, MAX_SESSIONS);
        crate::db::drop_test_target(&target).await;
    }
}
