//! Rule-based findings that need no LLM: rage clicks, dead clicks, JS errors
//! and form abandonment. Used when no API key is set, and as a fallback.

use trace_core::digest::{Digest, ErrorStat};
use trace_core::model::{Direction, EventMatch, Evidence, Kpi, NewInsight, pattern_matches};
use trace_core::query::{Measure, Metric};
use trace_core::util::pct;

const MIN_RAGE_SESSIONS: i64 = 3;
const MIN_DEAD_SESSIONS: i64 = 5;
const MIN_ERROR_SESSIONS: i64 = 3;
const MIN_FORM_STARTS: i64 = 10;
const MAX_FORM_COMPLETION: f64 = 0.6;
/// A funnel step worth a look: at least this many sessions lost, and fewer than
/// this share getting through.
const MIN_FUNNEL_LOST: i64 = 10;
const MAX_STEP_CONVERSION: f64 = 0.7;

/// What built-in insights record as their creator.
pub const CREATOR: &str = "Trace's built-in checks";

fn event(kind: &str, path: &str, selector: Option<&str>, name: Option<&str>) -> EventMatch {
    EventMatch { kind: kind.into(), path: Some(path.into()), selector: selector.map(Into::into), name: name.map(Into::into) }
}

/// Of the sessions that viewed this page, the share that then had this event
/// there: the same base as the digest's "of sessions that viewed /checkout".
fn on_page(kind: &str, path: &str, selector: Option<&str>, name: Option<&str>) -> Metric {
    Metric {
        measure: Measure::Conversion,
        matcher: None,
        from: Some(event("pageview", path, None, None)),
        to: Some(event(kind, path, selector, name)),
    }
}

pub fn find(digest: &Digest, kpis: &[Kpi]) -> Vec<NewInsight> {
    let days = digest.window.days;
    let mut found: Vec<(i64, NewInsight)> = Vec::new();
    let share = |path: &str, sessions: i64| {
        digest
            .page(path)
            .filter(|p| p.sessions > 0)
            .map(|p| format!(" ({} of sessions that viewed {path})", pct(sessions as f64 / p.sessions as f64)))
            .unwrap_or_default()
    };
    let page_line = |path: &str| {
        digest.page(path).map(|p| {
            format!("{path}: {} views in {} sessions, {} of those sessions ended here", p.views, p.sessions, pct(p.exit_rate))
        })
    };
    let label = |text: &Option<String>, selector: &str| {
        text.as_deref().filter(|t| !t.is_empty()).map(|t| format!("\"{t}\"")).unwrap_or_else(|| format!("`{selector}`"))
    };

    for c in digest.rage_clicks.iter().filter(|c| c.sessions >= MIN_RAGE_SESSIONS) {
        let what = label(&c.text, &c.selector);
        let mut evidence = vec![
            Evidence::Measured {
                claim: format!("Of sessions that view {}, those that rage-click {what}", c.path),
                metric: on_page("rage_click", &c.path, Some(&c.selector), None),
                goal: Direction::Decrease,
            },
            Evidence::note(format!("{} rage-click bursts (3+ rapid clicks) from {} sessions in the last {days} days", c.count, c.sessions)),
            Evidence::note(format!("Element: `{}` on {}", c.selector, c.path)),
        ];
        evidence.extend(page_line(&c.path).map(Evidence::note));
        found.push((
            c.sessions,
            NewInsight {
                title: format!("Fix the unresponsive {what} on {}", c.path),
                problem: format!(
                    "{} sessions{} clicked {what} several times in quick succession. Users expect it to do something it isn't doing, \
                     or it responds too slowly to feel like it worked.",
                    c.sessions,
                    share(&c.path, c.sessions)
                ),
                evidence,
                proposal: format!(
                    "Check what `{}` does on click. If the action is slow, give immediate feedback (pressed/disabled state and a spinner) \
                     and speed it up. If it fails silently, show an error that says what to do next. If it's deliberately inactive \
                     (for example a form is incomplete), say why right next to it.",
                    c.selector
                ),
                acceptance_criteria: vec![
                    "Clicking the element gives visible feedback within 100 ms".into(),
                    "Failures show a message telling the user what to do next".into(),
                    format!("No new console errors on {}", c.path),
                ],
                pages: vec![c.path.clone()],
                kpi_id: link_kpi(kpis, "rage_click", &c.path),
                expected_impact: "Fewer rage clicks on this element and fewer sessions abandoning this step".into(),
                confidence: if c.sessions >= 10 { "high" } else { "medium" }.into(),
                source: "heuristics".into(),
                created_by: Some(CREATOR.into()),
                readings: Vec::new(),
                fingerprint: Some(format!("rage:{}:{}", c.path, c.selector)),
            },
        ));
    }

    for c in digest.dead_clicks.iter().filter(|c| c.sessions >= MIN_DEAD_SESSIONS) {
        let what = label(&c.text, &c.selector);
        found.push((
            c.sessions,
            NewInsight {
                title: format!("Make {what} on {} do what users expect", c.path),
                problem: format!(
                    "{} sessions{} clicked {what} and nothing visible happened within a second. Users think it's interactive.",
                    c.sessions,
                    share(&c.path, c.sessions)
                ),
                evidence: vec![
                    Evidence::Measured {
                        claim: format!("Of sessions that view {}, those that click {what} and nothing happens", c.path),
                        metric: on_page("dead_click", &c.path, Some(&c.selector), None),
                        goal: Direction::Decrease,
                    },
                    Evidence::note(format!("{} dead clicks from {} sessions in the last {days} days", c.count, c.sessions)),
                    Evidence::note(format!("Element: `{}` on {}", c.selector, c.path)),
                ],
                proposal: format!(
                    "Decide what users are trying to do when they click `{}`. Either make it interactive (for example link it to the \
                     detail it previews), or remove the cues that make it look clickable (pointer cursor, hover styles, button-like styling).",
                    c.selector
                ),
                acceptance_criteria: vec![
                    "Clicking the element either does something visible or it no longer looks clickable".into(),
                    "Keyboard and screen-reader users get the same behaviour".into(),
                ],
                pages: vec![c.path.clone()],
                kpi_id: link_kpi(kpis, "dead_click", &c.path),
                expected_impact: "Fewer dead clicks and smoother navigation from this page".into(),
                confidence: "medium".into(),
                source: "heuristics".into(),
                created_by: Some(CREATOR.into()),
                readings: Vec::new(),
                fingerprint: Some(format!("dead:{}:{}", c.path, c.selector)),
            },
        ));
    }

    // Apps usually console.error the requests that fail. A console error on the same page,
    // hitting about as many sessions as a failing request, is that request's symptom,
    // so it becomes evidence on the request's insight, not an insight of its own.
    let same_cause = |console: &ErrorStat, network: &ErrorStat| {
        console.kind == "console"
            && network.kind == "network"
            && console.path == network.path
            && (console.sessions - network.sessions).abs() * 4 <= network.sessions.max(console.sessions)
    };
    let symptoms: Vec<&ErrorStat> = digest
        .errors
        .iter()
        .filter(|c| digest.errors.iter().any(|n| n.sessions >= MIN_ERROR_SESSIONS && same_cause(c, n)))
        .collect();

    for e in digest.errors.iter().filter(|e| e.sessions >= MIN_ERROR_SESSIONS) {
        if symptoms.iter().any(|s| std::ptr::eq(*s, e)) {
            continue;
        }
        let short = trace_core::util::ellipsize(&e.message, 70);
        let at = e.source.as_deref().map(|s| format!(" (at `{s}`)")).unwrap_or_default();
        let (verb, title, problem, proposal, check) = match e.kind.as_str() {
            "network" => (
                "failed",
                format!("Fix the failing request {short} on {}", e.path),
                format!(
                    "The request `{}` failed for {} sessions{} on {}. When a request fails, part of the page is usually \
                     missing or an action silently doesn't happen.",
                    e.message, e.sessions, share(&e.path, e.sessions), e.path
                ),
                format!(
                    "Find where {} calls this endpoint and why it fails (server logs for the status code). Fix the cause, and make \
                     the page handle a failure visibly: show a message or a retry, never a silent gap.",
                    e.path
                ),
                format!("`{}` succeeds when using {}, and if it ever fails the page says so", e.message.split(" → ").next().unwrap_or(&e.message), e.path),
            ),
            "resource" => (
                "happened",
                format!("Fix \"{short}\" on {}", e.path),
                format!(
                    "A file the page needs didn't load for {} sessions{} on {}: {}.",
                    e.sessions, share(&e.path, e.sessions), e.path, e.message
                ),
                "Check that the file exists in the deployed build and that its URL is correct (CDN paths, hashed file names, \
                 case). If it's optional, make the page work without it."
                    .to_string(),
                format!("{} loads every file it references", e.path),
            ),
            "console" => (
                "was logged",
                format!("Fix \"{short}\" on {}", e.path),
                format!(
                    "The app reported an error with console.error in {} sessions{} on {}. Frameworks log the errors they catch \
                     this way, so part of the page probably failed to render or update.",
                    e.sessions, share(&e.path, e.sessions), e.path
                ),
                format!(
                    "Find what logs `{}`{at} and fix the underlying failure. Make sure the user sees something useful when it happens.",
                    e.message
                ),
                format!("Using {} no longer logs `{short}`", e.path),
            ),
            _ => (
                "thrown",
                format!("Fix \"{short}\" on {}", e.path),
                format!(
                    "A JavaScript error hits {} sessions{} on {}. Errors like this often break part of the page without the user \
                     noticing why.",
                    e.sessions, share(&e.path, e.sessions), e.path
                ),
                format!(
                    "Reproduce the error on {}{at} and fix the root cause. Then add a guard so that if this code path fails again, \
                     the page degrades gracefully instead of breaking.",
                    e.path
                ),
                format!("Using {} no longer logs `{short}` in the console", e.path),
            ),
        };
        let mut evidence = vec![
            Evidence::Measured {
                claim: format!("Of sessions that view {}, those with {} errors{}", e.path, e.kind, e.source.as_deref().map(|s| format!(" from `{s}`")).unwrap_or_default()),
                metric: on_page("error", &e.path, e.source.as_deref(), Some(&e.kind)),
                goal: Direction::Decrease,
            },
            Evidence::note(format!("{} error `{}` {verb} {} times in {} sessions in the last {days} days", e.kind, e.message, e.count, e.sessions)),
        ];
        for c in symptoms.iter().filter(|c| same_cause(c, e)) {
            evidence.push(Evidence::note(format!("The app logs `{}` with console.error in {} sessions, likely this failure", c.message, c.sessions)));
        }
        evidence.extend(page_line(&e.path).map(Evidence::note));
        found.push((
            e.sessions * 2, // broken pages usually matter more than friction
            NewInsight {
                title,
                problem,
                evidence,
                proposal,
                acceptance_criteria: vec![check, "A test covers the case that used to fail".into()],
                pages: vec![e.path.clone()],
                kpi_id: link_kpi(kpis, "error", &e.path),
                expected_impact: "Fewer sessions with errors, and better progression from this page".into(),
                confidence: "high".into(),
                source: "heuristics".into(),
                created_by: Some(CREATOR.into()),
                readings: Vec::new(),
                fingerprint: Some(format!("error:{}:{}:{}", e.kind, e.path, e.message)),
            },
        ));
    }

    for f in digest
        .forms
        .iter()
        .filter(|f| f.started >= MIN_FORM_STARTS && f.completion_rate < MAX_FORM_COMPLETION)
    {
        found.push((
            f.started - f.submitted,
            NewInsight {
                title: format!("Reduce drop-off in the {} form", f.path),
                problem: format!(
                    "{} sessions started filling in the form `{}` on {}, but only {} ({}) submitted it.",
                    f.started,
                    f.selector,
                    f.path,
                    f.submitted,
                    pct(f.completion_rate)
                ),
                evidence: vec![
                    Evidence::Measured {
                        claim: format!("Sessions that start the form `{}` on {} and submit it", f.selector, f.path),
                        metric: Metric {
                            measure: Measure::Conversion,
                            matcher: None,
                            from: Some(event("form_start", &f.path, Some(&f.selector), None)),
                            to: Some(event("submit", &f.path, Some(&f.selector), None)),
                        },
                        goal: Direction::Increase,
                    },
                    Evidence::note(format!("{} form starts, {} submits in the last {days} days", f.started, f.submitted)),
                ],
                proposal: "Look for friction in the form: remove fields that aren't essential, validate inline with clear messages, \
                           keep what the user typed when there's an error, support autofill (correct `autocomplete` attributes and input \
                           types), and make it obvious when the submit button is ready. Trace doesn't record field values, so start by \
                           checking the form for the field most likely to stop people."
                    .into(),
                acceptance_criteria: vec![
                    "Every field has a visible label and the correct autocomplete attribute".into(),
                    "Validation errors appear next to the field and don't clear other input".into(),
                    "Only fields needed to finish the task are required".into(),
                ],
                pages: vec![f.path.clone()],
                kpi_id: link_kpi(kpis, "submit", &f.path).or_else(|| link_kpi(kpis, "form_start", &f.path)),
                expected_impact: format!("Higher completion than today's {}", pct(f.completion_rate)),
                confidence: "medium".into(),
                source: "heuristics".into(),
                created_by: Some(CREATOR.into()),
                readings: Vec::new(),
                fingerprint: Some(format!("form:{}:{}", f.path, f.selector)),
            },
        ));
    }

    // Funnels: the step that loses the most sessions, with what is wrong on the
    // page before it. Weighted by sessions lost, so it competes with the rest.
    for f in &digest.funnels {
        let Some(drop) = f.biggest_drop.as_ref().filter(|d| d.lost >= MIN_FUNNEL_LOST && 1.0 - d.rate < MAX_STEP_CONVERSION) else { continue };
        let i = drop.to_step - 1;
        let (before, step) = (&f.funnel.steps[i - 1], &f.funnel.steps[i]);
        let through = f.steps[i].of_previous.unwrap_or(0.0);
        let page = (before.matcher.kind == "pageview").then(|| before.matcher.path.clone()).flatten().filter(|p| !p.ends_with('*'));
        let mut evidence = vec![
            Evidence::Measured {
                claim: format!("Of sessions that reach {}, those that go on to {}", before.label, step.label),
                metric: Metric { measure: Measure::Conversion, matcher: None, from: Some(before.matcher.clone()), to: Some(step.matcher.clone()) },
                goal: Direction::Increase,
            },
            Evidence::note(format!(
                "Funnel \"{}\": {} sessions reached {}, {} went on to {}; {} lost here in the last {days} days, the most of any step",
                f.funnel.name, f.steps[i - 1].sessions, before.label, f.steps[i].sessions, step.label, drop.lost
            )),
        ];
        if let Some(p) = &page {
            evidence.extend(page_line(p).map(Evidence::note));
            let on = |list: &[trace_core::digest::ClickStat]| list.iter().filter(|c| &c.path == p).map(|c| c.sessions).sum::<i64>();
            let (rage, dead) = (on(&digest.rage_clicks), on(&digest.dead_clicks));
            let errors: i64 = digest.errors.iter().filter(|e| &e.path == p).map(|e| e.sessions).sum();
            if rage + dead + errors > 0 {
                evidence.push(Evidence::note(format!("On {p}: {rage} sessions with rage clicks, {dead} with dead clicks, {errors} with errors")));
            }
        }
        let mobile = f.by_device.iter().find(|d| d.device == "mobile");
        let desktop = f.by_device.iter().find(|d| d.device == "desktop");
        if let (Some(m), Some(d)) = (mobile, desktop)
            && m.sessions[i - 1] > 0
            && d.sessions[i - 1] > 0
        {
            evidence.push(Evidence::note(format!(
                "This step by device: {} on mobile, {} on desktop",
                pct(m.sessions[i] as f64 / m.sessions[i - 1] as f64),
                pct(d.sessions[i] as f64 / d.sessions[i - 1] as f64)
            )));
        }
        let kpi_id = kpis
            .iter()
            .find(|k| matches!(&k.definition, trace_core::model::KpiDefinition::Conversion { from, to } if *from == before.matcher && *to == step.matcher))
            .or_else(|| kpis.iter().find(|k| k.definition.matchers().iter().any(|m| **m == step.matcher)))
            .map(|k| k.id);
        found.push((
            drop.lost,
            NewInsight {
                title: format!("Get more people from {} to {}", before.label, step.label),
                problem: format!(
                    "In the funnel \"{}\", only {} of sessions that reach {} go on to {}: {} sessions stop here, more than at any other step.",
                    f.funnel.name,
                    pct(through),
                    before.label,
                    step.label,
                    drop.lost
                ),
                evidence,
                proposal: format!(
                    "Find what stops people {}. Make the way to {} the obvious next action there: one clear primary button, no \
                     competing choices above it, nothing broken or slow on the way (check the rage clicks, dead clicks and errors above), \
                     and the information people need to decide right next to it.",
                    page.as_deref().map(|p| format!("on {p}")).unwrap_or_else(|| format!("after {}", before.label)),
                    step.label
                ),
                acceptance_criteria: vec![
                    format!("The step from {} to {} converts better over the next 14 days", before.label, step.label),
                    "No new errors on the pages involved".into(),
                ],
                pages: page.into_iter().collect(),
                kpi_id,
                expected_impact: format!("More of the {} sessions lost at this step reach {}, and the funnel converts better overall", drop.lost, step.label),
                confidence: if drop.lost >= 50 { "high" } else { "medium" }.into(),
                source: "heuristics".into(),
                created_by: Some(CREATOR.into()),
                readings: Vec::new(),
                fingerprint: Some(format!("funnel:{}:{}", f.funnel.key, drop.to_step)),
            },
        ));
    }

    found.sort_by_key(|(weight, _)| std::cmp::Reverse(*weight));
    found.into_iter().map(|(_, insight)| insight).collect()
}

/// Picks the KPI an insight most plausibly moves: one that counts the same kind
/// of event, otherwise one whose definition names the same page.
fn link_kpi(kpis: &[Kpi], event_type: &str, path: &str) -> Option<i64> {
    kpis.iter()
        .find(|k| k.definition.matchers().iter().any(|m| m.kind == event_type))
        .or_else(|| {
            kpis.iter().find(|k| {
                k.definition
                    .matchers()
                    .iter()
                    .any(|m| m.path.is_some() && pattern_matches(&m.path, Some(path)))
            })
        })
        .map(|k| k.id)
}
