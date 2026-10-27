//! Rule-based findings that need no LLM: rage clicks, dead clicks, JS errors
//! and form abandonment. Used when no API key is set, and as a fallback.

use telltale_core::digest::Digest;
use telltale_core::model::{Kpi, NewIdea, pattern_matches};
use telltale_core::util::pct;

const MIN_RAGE_SESSIONS: i64 = 3;
const MIN_DEAD_SESSIONS: i64 = 5;
const MIN_ERROR_SESSIONS: i64 = 3;
const MIN_FORM_STARTS: i64 = 10;
const MAX_FORM_COMPLETION: f64 = 0.6;

pub fn find(digest: &Digest, kpis: &[Kpi]) -> Vec<NewIdea> {
    let days = digest.window.days;
    let mut found: Vec<(i64, NewIdea)> = Vec::new();
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
            format!("{} rage-click bursts (3+ rapid clicks) from {} sessions in the last {days} days", c.count, c.sessions),
            format!("Element: `{}` on {}", c.selector, c.path),
        ];
        evidence.extend(page_line(&c.path));
        found.push((
            c.sessions,
            NewIdea {
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
                fingerprint: Some(format!("rage:{}:{}", c.path, c.selector)),
            },
        ));
    }

    for c in digest.dead_clicks.iter().filter(|c| c.sessions >= MIN_DEAD_SESSIONS) {
        let what = label(&c.text, &c.selector);
        found.push((
            c.sessions,
            NewIdea {
                title: format!("Make {what} on {} do what users expect", c.path),
                problem: format!(
                    "{} sessions{} clicked {what} and nothing visible happened within a second. Users think it's interactive.",
                    c.sessions,
                    share(&c.path, c.sessions)
                ),
                evidence: vec![
                    format!("{} dead clicks from {} sessions in the last {days} days", c.count, c.sessions),
                    format!("Element: `{}` on {}", c.selector, c.path),
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
                fingerprint: Some(format!("dead:{}:{}", c.path, c.selector)),
            },
        ));
    }

    for e in digest.errors.iter().filter(|e| e.sessions >= MIN_ERROR_SESSIONS) {
        let short = telltale_core::util::ellipsize(&e.message, 70);
        let mut evidence = vec![
            format!("`{}` thrown {} times in {} sessions in the last {days} days", e.message, e.count, e.sessions),
        ];
        if let Some(src) = &e.source {
            evidence.push(format!("Source: `{src}`"));
        }
        evidence.extend(page_line(&e.path));
        found.push((
            e.sessions * 2, // broken pages usually matter more than friction
            NewIdea {
                title: format!("Fix \"{short}\" on {}", e.path),
                problem: format!(
                    "A JavaScript error hits {} sessions{} on {}. Errors like this often break part of the page without the user noticing why.",
                    e.sessions,
                    share(&e.path, e.sessions),
                    e.path
                ),
                evidence,
                proposal: format!(
                    "Reproduce the error on {}{} and fix the root cause. Then add a guard so that if this code path fails again, \
                     the page degrades gracefully instead of breaking.",
                    e.path,
                    e.source.as_deref().map(|s| format!(" (thrown at `{s}`)")).unwrap_or_default()
                ),
                acceptance_criteria: vec![
                    format!("Using {} no longer logs `{short}` in the console", e.path),
                    "A test covers the input that used to throw".into(),
                ],
                pages: vec![e.path.clone()],
                kpi_id: link_kpi(kpis, "error", &e.path),
                expected_impact: "Fewer sessions with errors, and better progression from this page".into(),
                confidence: "high".into(),
                source: "heuristics".into(),
                fingerprint: Some(format!("error:{}:{}", e.path, e.message)),
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
            NewIdea {
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
                    format!("{} form starts, {} submits in the last {days} days", f.started, f.submitted),
                    format!("Form: `{}` on {}", f.selector, f.path),
                ],
                proposal: "Look for friction in the form: remove fields that aren't essential, validate inline with clear messages, \
                           keep what the user typed when there's an error, support autofill (correct `autocomplete` attributes and input \
                           types), and make it obvious when the submit button is ready. Telltale doesn't record field values, so start by \
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
                fingerprint: Some(format!("form:{}:{}", f.path, f.selector)),
            },
        ));
    }

    found.sort_by_key(|(weight, _)| std::cmp::Reverse(*weight));
    found.into_iter().map(|(_, idea)| idea).collect()
}

/// Picks the KPI an idea most plausibly moves: one that counts the same kind
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
