"use strict";

// Insights and Releases.
// The dashboard is plain scripts loaded in order (see index.html); they share one scope.

// ---------------------------------------------------------------- Insights

let insightTab = store("trace_tab") || "new";

function kpiValue(k, v) {
  if (v == null) return "–";
  return k.kind === "per_session" ? Number(v).toFixed(2) : pct(v);
}

// What to ask your agent next, by where the insight is: the dashboard doesn't change insights.
function insightNext(insight) {
  const id = insight.id;
  switch (insight.status) {
    case "new":
    case "selected":
      return askAgent(`/trace:fix ${id}`, "Implement it");
    case "in_progress":
      return askAgent(`/trace:manage release insight ${id}`, "Once it's live");
  }
  return "";
}

const VERDICTS = { measuring: "Measuring", improved: "Improved after release", regressed: "Worse after release", no_change: "No change", insufficient_data: "Not enough data" };

// A defect's evidence is judged by whether it happens again, not by a comparison.
const DEFECT_VERDICTS = { improved: "Fixed", no_change: "Not fixed", insufficient_data: "Not enough traffic" };

// The KPI's verdict, and each measured evidence item's: did the problem itself shrink?
function verdictBlock(insight) {
  if (insight.status !== "released") return "";
  const measured = insight.evidence.some((e) => e.metric);
  const defect = insight.kind === "defect";
  let kpi;
  if (defect) {
    kpi = "";
  } else if (!insight.kpi_id) {
    kpi = measured ? "" : `<div class="verdict"><span class="badge">Not measured</span><span>No goal is linked, so there's no verdict. Your agent sets the goal when it drafts an insight.</span></div>`;
  } else if (!insight.evaluation) {
    kpi = `<div class="verdict"><span class="badge">Pending</span><span>Not evaluated yet. Trace checks released insights every hour.</span></div>`;
  } else {
    const e = insight.evaluation;
    kpi = `<div class="verdict ${esc(e.verdict)}"><span class="badge">${esc(VERDICTS[e.verdict] || e.verdict)}</span><span>${esc(e.summary)}</span></div>`;
  }
  const badge = (v) =>
    defect ? DEFECT_VERDICTS[v.verdict] || v.verdict : v.verdict === "regressed" ? "Worse" : v.verdict === "improved" ? "Better" : VERDICTS[v.verdict] || v.verdict;
  const items = (insight.item_verdicts || []).map(
    (v) => `<div class="verdict ${esc(v.verdict)} item"><span class="badge">${esc(badge(v))}</span><span>${esc(v.summary)}</span></div>`
  );
  if (!items.length && measured && insight.released_at) {
    const due = shortDate(insight.released_at + 14 * 86400000);
    items.push(
      defect
        ? `<div class="verdict measuring item"><span class="badge">Watching</span><span>${
            insight.evidence.some((e) => e.metric && e.goal === "increase")
              ? `Trace says as soon as it starts happening. If it hasn't by ${esc(due)}, it's still missing.`
              : `Trace says as soon as it happens again. If it doesn't, it counts as fixed on ${esc(due)}.`
          }</span></div>`
        : `<div class="verdict measuring item"><span class="badge">Measuring</span><span>The evidence is compared over the 14 days before and after the release; the result comes on ${esc(due)}.</span></div>`
    );
  }
  return kpi + items.join("");
}

let focusedInsight = null;

const shortDate = (ms) => new Date(ms).toLocaleDateString(undefined, { day: "numeric", month: "short" });

// One evidence item: a claim with Trace's latest measurement and its history, or a note.
function evidenceItem(insight, e, i) {
  if (e.note !== undefined) return `<li>${esc(e.note)} <span class="hint">(not measured)</span></li>`;
  const rs = (insight.readings || []).filter((r) => r.item === i && (r.kind === "first" || r.kind === "refresh"));
  const last = rs[rs.length - 1];
  const value = last
    ? `<strong>${esc(last.formatted || "no data")}</strong> <span class="hint">of ${num(last.sessions)} sessions, ${shortDate(last.from)} to ${shortDate(last.to)}</span>`
    : `<span class="hint">not measured yet</span>`;
  const history = rs.length > 1 ? `<div class="history hint">${rs.map((r) => `<span title="${esc(shortDate(r.at))}">${esc(r.formatted || "–")}</span>`).join(" → ")}</div>` : "";
  return `<li>${esc(e.claim)}: ${value}${history}</li>`;
}

function insightCard(insight) {
  const kpi = kpiCache.find((k) => k.id === insight.kpi_id);
  const focused = insight.id === focusedInsight;
  const released = insight.released_at ? ` · released ${new Date(insight.released_at).toLocaleDateString()}` : "";
  return `
  <article class="card ${focused ? "focused" : ""}" data-id="${insight.id}">
    <div class="meta">
      ${insight.kind === "defect" ? `<span class="chip defect" title="Something broken, found in the code: judged by whether it happens again">Defect</span>` : ""}
      <span class="chip ${esc(insight.confidence)}">${esc(insight.confidence)} confidence</span>
      <span>#${insight.id} · ${insight.created_by ? `by ${esc(insight.created_by)}` : `via ${esc(insight.source)}`} · ${shortDate(insight.created_at)}${released}</span>
      ${kpi ? `<span class="kpi-name" title="KPI this insight should move">${esc(kpi.name)}</span>` : ""}
    </div>
    <h3>${esc(insight.title)}</h3>
    <p class="problem">${esc(insight.problem)}</p>
    ${
      insight.evidence.length
        ? `<details><summary>Evidence (${insight.evidence.length})</summary><ul>${insight.evidence.map((e, i) => evidenceItem(insight, e, i)).join("")}</ul></details>`
        : ""
    }
    <div class="proposal"><strong>Proposed change</strong>${esc(insight.proposal)}</div>
    ${insight.expected_impact ? `<p class="hint">Expected impact: ${esc(insight.expected_impact)}</p>` : ""}
    ${insight.dismiss_reason ? `<p class="dismissed-reason">Dismissed: ${esc(insight.dismiss_reason)}</p>` : ""}
    ${verdictBlock(insight)}
    <div class="actions">
      <button class="${focused ? "primary" : ""}" data-copy>Copy as prompt</button>
      <span class="spacer"></span>
      ${PUBLIC ? "" : insightNext(insight)}
    </div>
  </article>`;
}

async function showPrompt(id) {
  const panel = view.querySelector(".prompt-panel");
  if (!panel) return;
  panel.querySelector("pre").textContent = "Loading…";
  panel.dataset.id = id;
  panel.querySelector(".cli").textContent = `trace prompt ${id} | claude`;
  const text = await api(`/insights/${id}/prompt`);
  if (panel.dataset.id === String(id)) panel.querySelector("pre").textContent = text;
}

async function copyPrompt(id, btn) {
  const text = await api(`/insights/${id}/prompt`);
  if (await copyText(text)) {
    measure("prompt_copied");
    const label = btn.textContent;
    btn.textContent = "Copied ✓";
    setTimeout(() => (btn.textContent = label), 1600);
  } else {
    toast("Couldn't copy. Select the prompt in the panel and copy it from there.", true);
  }
}

// Insights in progress, or (as Releases) the shipped ones and their verdicts.
let insightsMode = "insights";

async function renderInsights() {
  const releases = insightsMode === "releases";
  const [insights, kpis, analysis] = await Promise.all([api("/insights"), api("/kpis"), releases || PUBLIC ? null : api("/analysis").catch(() => null)]);
  kpiCache = kpis;
  if (!releases && insightTab === "released") insightTab = "new";
  const tab = releases ? "released" : insightTab;
  const counts = Object.fromEntries(STATUSES.map(([s]) => [s, insights.filter((i) => i.status === s).length]));
  const shown = insights.filter((i) => i.status === tab).sort((a, b) => (releases ? (b.released_at || 0) - (a.released_at || 0) : 0));
  if (!shown.some((i) => i.id === focusedInsight)) focusedInsight = shown.length ? shown[0].id : null;
  view.innerHTML = `
    <h1>${releases ? "Releases" : "Insights"}</h1>
    <p class="sub">${
      releases
        ? "Shipped changes, newest first, and whether each moved its goal: Trace compares the 14 days before the release with the 14 after."
        : PUBLIC
        ? `What a coding agent found in how people use ${esc(project.name)}, with the change it suggests. Each one is also a ready prompt for a coding agent.`
        : "What your coding agent found in real behaviour, with the change it suggests. Your agent moves them along: /trace:fix implements one, and /trace:manage picks, dismisses or releases one. Once released, it moves to Releases, where Trace measures whether it worked."
    }</p>
    ${analysis ? analysisPanel(analysis) : ""}
    <div class="chips" role="tablist" aria-label="Status" ${releases ? "hidden" : ""}>
      ${STATUSES.filter(([s]) => s !== "released" && !(PUBLIC && s === "dismissed")).map(
        ([s, label]) =>
          `<button role="tab" data-tab="${s}" class="${s === tab ? "active" : ""}" aria-selected="${s === tab}">${label}<span class="count">${counts[s]}</span></button>`
      ).join("")}
    </div>
    ${
      shown.length
        ? `<div class="insights-layout">
            <div class="insights">${shown.map(insightCard).join("")}</div>
            <aside class="prompt-panel" aria-label="Prompt for your coding agent">
              <div class="head"><strong>Prompt for your coding agent</strong><button data-panel-copy>Copy</button></div>
              <pre></pre>
              <div class="cli"></div>
            </aside>
          </div>`
        : `<div class="empty">${
            PUBLIC
              ? "Nothing here yet."
              : releases
              ? `Nothing released yet. When a change from an insight ships, mark it released and its verdict appears here.`
              : tab === "new"
              ? `No new insights. The panel above says when an analysis is worth running and how.`
              : `Nothing here yet.`
          }</div>`
    }`;
  if (analysis) {
    wireAnalysisPanel(analysis);
    analysisBadge(analysis);
  }

  view.querySelectorAll("[data-tab]").forEach((el) =>
    el.addEventListener("click", () => {
      insightTab = el.dataset.tab;
      store("trace_tab", insightTab);
      renderInsights();
    })
  );
  if (focusedInsight != null) showPrompt(focusedInsight);
  view.querySelector("[data-panel-copy]")?.addEventListener("click", (e) => copyPrompt(focusedInsight, e.currentTarget));

  view.querySelectorAll(".card").forEach((card) => {
    const id = Number(card.dataset.id);
    // Clicking a card shows its prompt; controls inside it keep their own behaviour.
    card.addEventListener("click", (e) => {
      if (e.target.closest("button, select, a, summary") || focusedInsight === id) return;
      focusedInsight = id;
      view.querySelectorAll(".card").forEach((c) => {
        const on = Number(c.dataset.id) === id;
        c.classList.toggle("focused", on);
        c.querySelector("[data-copy]").classList.toggle("primary", on);
      });
      showPrompt(id);
    });
    card.querySelector("[data-copy]").addEventListener("click", (e) => copyPrompt(id, e.currentTarget));
  });
}
