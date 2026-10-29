"use strict";

// Features: areas and features, and one area's page.
// The dashboard is plain scripts loaded in order (see index.html); they share one scope.

// ---------------------------------------------------------------- Features

let featuresDays = Number(store("trace_features_days")) || 30;

const AREA_STATUS = {
  painful: ["Used, but it hurts", "bad", "Many people use it and many of them hit a problem: fixing it helps the most people."],
  rare_painful: ["Little used, and it hurts", "bad", "Few use it and it goes wrong for them: fix it, or ask whether it's needed."],
  rare: ["Little used", "warn", "Few reach it: is it hard to find, or not needed?"],
  healthy: ["Healthy", "good", "Used, and rarely goes wrong."],
  too_little: ["Too little data", "", "Fewer than 10 sessions in this window."],
};
const HURTS = ["painful", "rare_painful"];

const AREAS_EXAMPLE = `"areas": {
  "battles": {
    "name": "Battles",
    "features": {
      "duels": { "name": "Duels", "match": [{ "pageview": "/challenges*" }] },
      "leagues": { "name": "Leagues", "match": [{ "pageview": "/leagues*" }] }
    }
  }
}`;

const PROBLEM_KIND = { error: "Error", rage_click: "Rage click", dead_click: "Dead click" };

function statusBadge(status) {
  const [label, tone] = AREA_STATUS[status] || [status, ""];
  return html`<span class="badge ${tone}">${label}</span>`;
}

function problemLine(p) {
  return html`<strong>${PROBLEM_KIND[p.kind] || p.kind}</strong> on <code>${p.path}</code>: ${p.what}, in ${num(p.sessions)} session${p.sessions === 1 ? "" : "s"}`;
}

// A prompt for a coding agent about one problem, so it can be handed over as a task.
function problemTask(p, node) {
  return `Trace measured a problem on ${project.name} (Trace website "${project.slug}")${node ? ` in "${node.name}"` : ""} over the last ${featuresDays} days: ${(PROBLEM_KIND[p.kind] || p.kind).toLowerCase()} on ${p.path}: ${p.what}, in ${p.sessions} sessions.

Find the cause in this codebase and fix it. Then use the trace MCP tools to save an insight for it (draft_insights, then save_insights) with the share of sessions hitting this problem as evidence to decrease, and mark it released when the fix ships, so Trace can tell whether it worked.`;
}

function analyseButton(key, primary = false, label = "Analyse this") {
  const command = onSite(`${ANALYSE_COMMAND} ${key}`);
  return html`<button type="button" class="${primary ? "primary" : ""}" data-copy="${command}" title="Copies ${command} for Claude Code">${label}</button>`;
}

// Further right is used by more visitors, higher up goes wrong more often.
function useAgainstProblems(nodes) {
  const shown = nodes.filter((n) => n.sessions >= 10 && n.reach != null && n.problem_rate != null);
  if (shown.length < 2) return "";
  const maxX = Math.min(1, Math.max(...shown.map((n) => n.reach)) * 1.15);
  const maxY = Math.max(0.35, Math.max(...shown.map((n) => n.problem_rate)) * 1.15);
  const line = (1 - 0.15 / maxY) * 100;
  // Labels along the floor (no problems) go above their dot, alternating heights so neighbours don't collide.
  let floor = 0;
  const dots = shown
    .slice()
    .sort((p, q) => p.reach - q.reach)
    .map((n) => {
      const x = ((n.reach / maxX) * 100).toFixed(1);
      const y = (1 - n.problem_rate / maxY) * 100;
      const place = y > 92 ? `above${floor++ % 2 ? " high" : ""}` : x > 70 ? "left" : "";
      return html`<span class="dot ${HURTS.includes(n.status) ? "hurts" : ""}" style="left:${x}%;top:${y.toFixed(1)}%"></span>
        <span class="dot-label ${place}" style="left:${x}%;top:${y.toFixed(1)}%">${n.name}</span>`;
    });
  const label = shown.map((n) => `${n.name}: ${pct(n.reach)} reach, ${pct(n.problem_rate)} with problems`).join(". ");
  return html`<div class="card quadrant">
    <div><h2>Use against problems</h2><p class="hint">Further right is used by more visitors; higher up goes wrong more often.</p></div>
    <div class="plot" role="img" aria-label="${label}">
      <div class="hurts-band" style="height:${line.toFixed(1)}%"><span>Hurts: 15% or more</span></div>
      ${dots}
      <span class="axis y-top">${pct(maxY)}</span><span class="axis y-bottom">0%</span>
      <span class="axis x-left">0% reach</span><span class="axis x-right">${pct(maxX)}</span>
    </div>
  </div>`;
}

function areaCard(area, features) {
  const rows = features.map(
    (f) => html`<li><span>${f.name}${HURTS.includes(f.status) ? html` <span class="hurt-dot" title="${AREA_STATUS[f.status][0]}"></span><span class="visually-hidden">(hurts)</span>` : ""}</span>
      <span class="reach-bar"><span style="width:${((f.reach || 0) * 100).toFixed(1)}%"></span></span>
      <span class="hint">${f.reach == null ? "–" : `${Math.round(f.reach * 100)}%`}</span></li>`
  );
  return html`<article class="card area-card">
    <div class="area-head">
      <div class="area-title"><a href="#/understand/features/${encodeURIComponent(area.key)}">${area.name}</a>${statusBadge(area.status)}</div>
      ${area.description ? html`<span class="hint">${area.description}</span>` : ""}
      <div class="area-stats">
        <div><strong>${pct(area.reach)}</strong><span>reach</span></div>
        <div><strong>${area.people ? pct(area.adoption) : "–"}</strong><span>of people</span></div>
        <div><strong class="${HURTS.includes(area.status) ? "bad-text" : ""}">${pct(area.problem_rate)}</strong><span>hit a problem</span></div>
      </div>
    </div>
    ${rows.length ? html`<ul class="area-features">${rows}</ul>` : ""}
  </article>`;
}

// Which parts of the product people use, which they don't, and where it hurts.
async function renderFeatures() {
  if (routeArg) return renderArea(routeArg);
  const a = await api(`/areas?days=${featuresDays}`);
  const areas = a.nodes.filter((n) => !n.area);
  const features = (key) => a.nodes.filter((n) => n.area === key);
  const leaves = a.nodes.filter((n) => n.area || !a.nodes.some((f) => f.area === n.key));
  const first = leaves.filter((n) => HURTS.includes(n.status)).sort((x, y) => y.problem_sessions - x.problem_sessions).slice(0, 3);
  const counts = Object.fromEntries(Object.keys(AREA_STATUS).map((s) => [s, leaves.filter((n) => n.status === s)]));
  const source =
    a.source === "suggested"
      ? html`<div class="card"><p><strong>Suggested from your URLs.</strong> Name your product's areas and features in <code>trace.json</code>, by the pages and events that mean someone used them. In Claude Code, <code>/trace:setup</code> proposes them from your routes.</p>${snippet(AREAS_EXAMPLE)}</div>`
      : "";
  const firstCards = first.map((n, i) => {
    const parent = n.area ? a.nodes.find((x) => x.key === n.area) : null;
    return html`<article class="card look-first">
      <div class="look-head"><strong>${n.name}</strong>${parent ? html`<span class="hint">${parent.name}</span>` : ""}</div>
      <div class="look-rate"><span class="bad-text">${pct(n.problem_rate)}</span><span class="hint">of ${num(n.sessions)} sessions hit a problem</span></div>
      ${n.top_problem ? html`<p>${problemLine(n.top_problem)}</p>` : ""}
      <div class="actions">${analyseButton(n.key, i === 0)}<a class="button" href="#/understand/features/${encodeURIComponent(n.area || n.key)}">Open ${parent ? parent.name : n.name}</a></div>
    </article>`;
  });
  const statusCards = ["painful", "rare_painful", "rare", "healthy"].map((s) => {
    const [label, tone, hint] = AREA_STATUS[s];
    const list = counts[s];
    return html`<div class="card status-card tone-${tone}"><strong>${label} · ${list.length}</strong><span class="hint">${list.length ? list.map((n) => n.name).join(", ") : hint}</span></div>`;
  });
  view.innerHTML = html`
    <div class="toolbar">
      <div><h1>${project.name}, by area</h1><p class="sub">Which parts of the product people use, and where using them goes wrong: rage clicks, dead clicks and errors on each area's own pages. ${
        a.source === "trace.json" ? `${areas.length} area${areas.length === 1 ? "" : "s"} and ${a.nodes.length - areas.length} features, from trace.json.` : ""
      }</p></div>
      <label class="field narrow">Window
        <select id="features-days">${[7, 14, 30, 90].map((n) => html`<option value="${n}" ${n === featuresDays ? raw("selected") : ""}>Last ${n} days</option>`)}</select>
      </label>
    </div>
    ${source}
    ${first.length ? html`<h2>Where to look first</h2><p class="sub">The parts people use that hurt, by sessions affected.</p><div class="look-first-grid">${firstCards}</div>` : ""}
    ${
      areas.length
        ? html`<h2>The product map</h2><div class="area-grid">${areas.map((x) => areaCard(x, features(x.key)))}</div>`
        : html`<div class="card"><p>No areas yet: there are no pages viewed in this window to suggest them from.</p></div>`
    }
    <div class="quadrant-row">
      ${useAgainstProblems(leaves)}
      <div class="status-list">${statusCards}${counts.too_little.length ? html`<p class="hint">Too little data yet: ${counts.too_little.map((n) => n.name).join(", ")}.</p>` : ""}</div>
    </div>
    <p class="hint">Reach counts visitors once a day, like everywhere in Trace. "Of people" and come-back count signed-in people, so they need <code>trace.identify()</code>. Changes against the window before show once both windows have 10 sessions.</p>`;
  view.querySelector("#features-days").addEventListener("change", (ev) => {
    featuresDays = Number(ev.target.value);
    store("trace_features_days", featuresDays);
    renderFeatures();
  });
  wireCopy();
}

// One area: its numbers, its features, what goes wrong on its pages, and what's been done about it.
async function renderArea(key) {
  const [a, funnels, insights] = await Promise.all([api(`/areas?days=${featuresDays}`), api("/funnels").catch(() => []), api("/insights").catch(() => [])]);
  const area = a.nodes.find((n) => n.key === key && !n.area);
  if (!area) {
    view.innerHTML = html`<div class="empty"><h1>No area called "${key}"</h1><p><a href="#/understand/features">Back to features</a></p></div>`;
    return;
  }
  const features = a.nodes.filter((n) => n.area === key);
  const seen = new Set();
  const problems = [area, ...features]
    .filter((n) => n.top_problem)
    .map((n) => ({ ...n.top_problem, node: n }))
    .filter((p) => !seen.has(`${p.kind}|${p.path}|${p.what}`) && seen.add(`${p.kind}|${p.path}|${p.what}`))
    .sort((x, y) => y.sessions - x.sessions);
  const patterns = area.matches.filter((m) => m.startsWith("/"));
  const onPage = (path) => patterns.some((p) => (p.endsWith("*") ? path.startsWith(p.slice(0, -1)) : path === p));
  const through = funnels.filter((f) => f.steps.some((s) => area.matches.includes(s.label) || onPage(s.label)));
  const about = insights.filter((i) => i.status !== "dismissed" && (i.pages || []).some(onPage));
  const hurts = HURTS.includes(area.status);
  const tile = (label, value, sub, tone = "") => html`<div class="card stat ${tone}"><div class="label">${label}</div><div class="value">${value}</div><div class="hint">${sub}</div></div>`;
  const featureRows = features
    .slice()
    .sort((x, y) => y.problem_sessions - x.problem_sessions)
    .map((f) => [
      html`<strong>${f.name}</strong>`,
      html`${pct(f.reach)}<div class="hint">${num(f.sessions)} sessions</div>`,
      f.people ? pct(f.adoption) : "–",
      f.people ? pct(f.repeat) : "–",
      html`<span class="${HURTS.includes(f.status) ? "bad-text" : ""}">${pct(f.problem_rate)}</span>`,
      statusBadge(f.status),
    ]);
  const problemItems = problems.map(
    (p) => html`<li class="card">
      <span class="badge ${p.kind === "error" ? "bad" : "warn"}">${PROBLEM_KIND[p.kind] || p.kind}</span>
      <div class="problem-text"><strong>${p.what}</strong><span class="hint">On <code>${p.path}</code>${p.node !== area ? ` · ${p.node.name}` : ""}</span></div>
      <span class="problem-count">${num(p.sessions)}</span>
      ${PUBLIC ? "" : html`<button type="button" data-task="1" data-copy="${problemTask(p, p.node)}" title="Copies a task for your coding agent">Make it a task</button>`}
    </li>`
  );
  view.innerHTML = html`
    <nav class="breadcrumb" aria-label="Breadcrumb"><a href="#/understand/features">Features</a> <span aria-hidden="true">›</span> <span aria-current="page">${area.name}</span></nav>
    <div class="toolbar">
      <div><div class="area-title"><h1>${area.name}</h1>${statusBadge(area.status)}</div>
        <p class="sub">${area.description ? `${area.description}. ` : ""}Counted from ${area.matches.map((m, i) => html`${i ? ", " : ""}<code>${m}</code>`)}${a.source === "trace.json" ? ", as declared in trace.json" : ""}. Last ${featuresDays} days.</p></div>
      ${PUBLIC ? "" : analyseButton(area.key, true, `Analyse ${area.name}`)}
    </div>
    <div class="stats">
      ${tile("Reach", pct(area.reach), `of visitors · ${num(area.sessions)} sessions`)}
      ${tile("Of signed-in people", area.people ? pct(area.adoption) : "–", area.people ? `${num(area.people)} of ${num(a.people)}` : "needs trace.identify()")}
      ${tile("Come back", area.people ? pct(area.repeat) : "–", "used it on two or more days")}
      ${tile("Hit a problem", html`<span class="${hurts ? "bad-text" : ""}">${pct(area.problem_rate)}</span>`, `${num(area.problem_sessions)} sessions`, hurts ? "hurts" : "")}
    </div>
    ${
      features.length
        ? html`<h2>Its features</h2>${table(
            [{ label: "Feature" }, { label: "Reach", num: 1 }, { label: "Of people", num: 1 }, { label: "Come back", num: 1 }, { label: "Hit a problem", num: 1 }, { label: "Status" }],
            featureRows
          )}`
        : ""
    }
    <h2>What goes wrong here</h2>
    <p class="sub">The most common rage click, dead click or error on ${area.name}'s own pages and on each feature's.</p>
    ${problems.length ? html`<ol class="problem-list">${problemItems}</ol>` : html`<p class="hint">No rage clicks, dead clicks or errors on its pages in this window.</p>`}
    <div class="side-by-side">
      <div class="card"><h2>Funnels through ${area.name}</h2>${
        through.length ? html`<ul>${through.map((f) => html`<li><a href="#/understand/funnels">${f.name}</a></li>`)}</ul>` : html`<p class="hint">No funnel has a step on its pages.</p>`
      }</div>
      <div class="card"><h2>Insights about ${area.name}</h2>${
        about.length
          ? html`<ul>${about.map((i) => html`<li><a href="${i.status === "released" ? "#/improve/releases" : "#/improve/insights"}">${i.title}</a> <span class="hint">${i.status.replace("_", " ")}</span></li>`)}</ul>`
          : html`<p class="hint">None yet. Analysing just this area gives your agent its pages, features and problems, and asks only about them.</p>`
      }${PUBLIC ? "" : snippet(onSite(`${ANALYSE_COMMAND} ${area.key}`))}</div>
    </div>`;
  wireCopy();
}
