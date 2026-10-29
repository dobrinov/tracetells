"use strict";

// Goals (KPIs) and how a change is shown.
// The dashboard is plain scripts loaded in order (see index.html); they share one scope.

// ---------------------------------------------------------------- KPIs

function deltaHtml(k) {
  if (k.current == null || k.previous == null) return "";
  const d = k.current - k.previous;
  const rate = k.kind !== "per_session";
  if (Math.abs(d) < (rate ? 0.005 : 0.005)) return `<span class="delta">no change</span>`;
  const good = (d > 0) === (k.direction === "increase");
  const text = rate ? `${(Math.abs(d) * 100).toFixed(1)} pts` : Math.abs(d).toFixed(2);
  return `<span class="delta ${good ? "good" : "bad"}">${d > 0 ? "▲" : "▼"} ${text}, ${good ? "better" : "worse"}</span>`;
}

// Enough, early or not enough: every number says how sure it is.
const SAMPLE_MIN = 10;
const SAMPLE_EARLY = 30;
function sampleState(n) {
  return n < SAMPLE_MIN ? "none" : n < SAMPLE_EARLY ? "early" : "enough";
}

function ago(ts) {
  if (!ts) return "never";
  const m = Math.round((Date.now() - ts) / 60000);
  if (m < 1) return "just now";
  if (m < 60) return `${m} min ago`;
  const h = Math.round(m / 60);
  return h < 48 ? `${h} h ago` : `${Math.round(h / 24)} days ago`;
}

const CONFIG_EXAMPLE = `{
  "product": "Your product",
  "events": {
    "purchase": "Payment succeeded"
  },
  "kpis": {
    "checkout_conversion": {
      "name": "Checkout conversion",
      "conversion": { "from": { "pageview": "/checkout" }, "to": { "event": "purchase" } },
      "goal": "increase",
      "target": "55%"
    }
  }
}`;

// Where the value and target sit on a bar: rates use 0–100%, per-session counts
// scale to whichever of value and target is larger.
function kpiBar(k) {
  if (k.current == null) return "";
  const max = k.kind === "per_session" ? Math.max(k.current, k.target ?? 0) * 1.25 || 1 : 1;
  const at = (v) => Math.max(0, Math.min(100, (v / max) * 100));
  const tone = k.target == null || k.target_met ? "" : k.direction === "decrease" ? "bad" : "";
  return `<div class="bar ${tone}" role="img" aria-label="${esc(k.name)}: ${kpiValue(k, k.current)}${k.target != null ? `, target ${kpiValue(k, k.target)}` : ""}">
    <span style="width:${at(k.current).toFixed(1)}%"></span>
    ${k.target != null ? `<i class="target" style="left:${at(k.target).toFixed(1)}%"></i>` : ""}
  </div>`;
}

function kpiCard(k) {
  const target = k.target != null ? `Target ${k.direction === "increase" ? "≥" : "≤"} ${kpiValue(k, k.target)}${k.target_met ? " ✓" : ""}` : "No target";
  const state = sampleState(k.sessions || 0);
  const value =
    state === "none"
      ? `<span class="value muted">–</span><span class="sample none">Needs ${SAMPLE_MIN} sessions, has ${num(k.sessions || 0)}</span>`
      : state === "early"
      ? `<span class="value muted">${kpiValue(k, k.current)}</span><span class="sample early">Early · ${num(k.sessions)} sessions</span>`
      : `<span class="value">${kpiValue(k, k.current)}</span>${deltaHtml(k)}`;
  return `
    <div class="card kpi-card sample-${state}">
      <div class="head"><span class="name">${esc(k.name)}</span><span class="key" title="Key in trace.json">${esc(k.key || "")}</span></div>
      <div class="value-row">${value}</div>
      ${state === "none" ? "" : kpiBar(k)}
      <span class="hint">${target}${state === "enough" ? ` · ${num(k.sessions)} sessions` : ""}</span>
      <div class="def">${esc(k.description || k.summary)}</div>
    </div>`;
}
