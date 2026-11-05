"use strict";

// Work: every change from the insight that suggested it to whether it helped,
// on one board. The board changes nothing: each card shows the command to give
// the coding agent for its next step, as insight cards always have.
// The dashboard is plain scripts loaded in order (see index.html); they share one scope.

let workOpen = null;
let workDismissed = false;
let workFocus = null;

const MEASURE_DAYS = 14;

// A release's verdict is settled once Trace has measured the full window after it.
function settled(i) {
  if (i.evaluation && i.evaluation.settled_at) return true;
  const items = i.item_verdicts || [];
  return items.length > 0 && items.every((v) => v.settled_at);
}

const COLUMNS = [
  { key: "suggested", label: "Suggested", hint: "What your agent found and suggests. Pick the ones worth doing.", has: (i) => i.status === "new" },
  { key: "picked", label: "Picked", hint: "Agreed to do next. Your agent takes one with /trace:fix.", has: (i) => i.status === "selected" },
  { key: "progress", label: "In progress", hint: "Being built. Mark it released once it's live.", has: (i) => i.status === "in_progress" },
  { key: "measuring", label: "Released, measuring", hint: `Live. Trace compares the ${MEASURE_DAYS} days before the release with the ${MEASURE_DAYS} after.`, has: (i) => i.status === "released" && !settled(i) },
  { key: "measured", label: "Measured", hint: "Released, with a verdict: did it help?", has: (i) => i.status === "released" && settled(i) },
];

// The next step's command for the agent: the dashboard never moves an insight itself.
function workNext(i) {
  if (PUBLIC) return "";
  if (i.status === "new") return askAgent(`/trace:manage pick insight ${i.id}`, "Pick it");
  if (i.status === "selected") return askAgent(`/trace:fix ${i.id}`, "Build it");
  if (i.status === "in_progress") return askAgent(`/trace:manage release insight ${i.id}`, "Once it's live");
  return "";
}

function workCard(i) {
  const kpi = kpiCache.find((k) => k.id === i.kpi_id);
  const days = i.released_at ? Math.min(MEASURE_DAYS, Math.max(0, Math.floor((Date.now() - i.released_at) / 86400000))) : 0;
  const e = i.evaluation;
  const verdictText = e ? (i.kind === "defect" ? DEFECT_VERDICTS[e.verdict] : VERDICTS[e.verdict]) || e.verdict : null;
  const items = i.item_verdicts || [];
  return html`<article class="card work-card${workFocus === i.id ? " focused" : ""}" data-id="${i.id}">
    <div class="work-chips">
      <span class="chip${i.kind === "defect" ? " defect" : ""}">${i.kind === "defect" ? "Defect" : "Pattern"}</span>
      <span class="chip ${i.confidence}">${i.confidence} confidence</span>
    </div>
    <button type="button" class="work-title" data-open="${i.id}">${i.title}</button>
    ${i.pages.length ? html`<div class="work-pages">${i.pages.slice(0, 2).map((p) => pageLink(p))}${i.pages.length > 2 ? html`<span class="hint">+${i.pages.length - 2}</span>` : ""}</div>` : ""}
    ${kpi ? html`<div class="hint">Goal: ${kpi.name}</div>` : ""}
    ${i.status === "released" && !settled(i)
      ? html`<div class="work-measuring"><div class="work-measuring-head"><span>Measuring</span><span>day ${days} of ${MEASURE_DAYS}</span></div><div class="funnel-bar"><span style="width:${((days / MEASURE_DAYS) * 100).toFixed(0)}%"></span></div></div>`
      : ""}
    ${i.status === "released" && settled(i)
      ? html`<div class="work-verdict ${e ? e.verdict : items.length ? items[0].verdict : ""}">
          ${e && kpi && e.baseline != null && e.current != null
            ? html`<div class="work-before-after" role="img" aria-label="${kpi.name} ${kpiValue(kpi, e.baseline)} before, ${kpiValue(kpi, e.current)} after">
                <span><b>${kpiValue(kpi, e.baseline)}</b><span class="hint">before</span></span><span aria-hidden="true">→</span><span><b>${kpiValue(kpi, e.current)}</b><span class="hint">after</span></span>
              </div>`
            : ""}
          <strong>${verdictText || (items.length ? (i.kind === "defect" ? DEFECT_VERDICTS[items[0].verdict] : VERDICTS[items[0].verdict]) || items[0].verdict : "Measured")}</strong>
        </div>`
      : ""}
    ${workNext(i)}
  </article>`;
}

function workPanel(i) {
  return html`<aside class="work-panel" role="dialog" aria-label="${i.title}">
    <div class="work-panel-head"><strong>Insight #${i.id}</strong><button type="button" data-close aria-label="Close">×</button></div>
    ${raw(insightCard(i))}
  </aside>`;
}

async function renderWork() {
  const [insights, kpis, analysis] = await Promise.all([api("/insights"), api("/kpis").catch(() => []), PUBLIC ? null : api("/analysis").catch(() => null)]);
  kpiCache = kpis;
  const dismissed = insights.filter((i) => i.status === "dismissed");
  const columns = COLUMNS.map((c) => ({ ...c, cards: insights.filter(c.has).sort((a, b) => (b.released_at || b.updated_at) - (a.released_at || a.updated_at)) }));
  const open = insights.find((i) => i.id === workOpen);
  view.innerHTML = html`
    <div class="pulse-head">
      <div><h1>Work</h1><p class="sub">Every change from the insight that suggested it to whether it helped. ${PUBLIC ? "" : "Your coding agent moves the cards: each one shows the command for its next step."}</p></div>
      ${dismissed.length && !PUBLIC ? html`<button type="button" data-dismissed aria-pressed="${workDismissed ? "true" : "false"}">Dismissed · ${dismissed.length}</button>` : ""}
    </div>
    ${analysis && analysis.next === "analyse" ? raw(analysisPanel(analysis)) : ""}
    <div class="work-board" tabindex="0" aria-label="Insights by stage">
      ${columns.map(
        (c) => html`<section class="work-column" id="work-${c.key}" aria-labelledby="work-${c.key}-h">
          <div class="work-column-head"><h2 id="work-${c.key}-h">${c.label}</h2><span class="hint">${c.cards.length}</span></div>
          ${c.cards.length ? c.cards.map(workCard) : html`<p class="hint work-empty">${c.hint}</p>`}
        </section>`
      )}
    </div>
    ${workDismissed
      ? html`<section class="work-dismissed"><h2>Dismissed</h2><ul>${dismissed.map(
          (i) => html`<li><button type="button" class="work-title" data-open="${i.id}">${i.title}</button><span class="hint">${i.dismiss_reason || "No reason given"}</span></li>`
        )}</ul></section>`
      : ""}
    ${open ? workPanel(open) : ""}`;
  if (analysis) {
    if (analysis.next === "analyse") wireAnalysisPanel(analysis);
    analysisBadge(analysis);
  }
  view.querySelectorAll("[data-open]").forEach((b) =>
    b.addEventListener("click", () => {
      workOpen = Number(b.dataset.open);
      renderWork();
    })
  );
  view.querySelector("[data-close]")?.addEventListener("click", () => {
    workOpen = null;
    renderWork();
  });
  view.querySelector("[data-dismissed]")?.addEventListener("click", () => {
    workDismissed = !workDismissed;
    renderWork();
  });
  const panel = view.querySelector(".work-panel");
  if (panel) {
    panel.querySelector("[data-copy]")?.addEventListener("click", (e) => copyPrompt(open.id, e.currentTarget));
    panel.querySelector("[data-close]").focus();
  }
  // An old link to Releases lands here, on its column.
  if (workFocus === "released") {
    workFocus = null;
    view.querySelector("#work-measuring")?.scrollIntoView({ block: "nearest", inline: "start" });
  }
}

document.addEventListener("keydown", (e) => {
  if (e.key === "Escape" && workOpen != null && view.querySelector(".work-panel")) {
    workOpen = null;
    renderWork();
  }
});
