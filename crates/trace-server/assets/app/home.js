"use strict";

// Home, and the funnel cards and tracking table other pages use.
// The dashboard is plain scripts loaded in order (see index.html); they share one scope.

// ---------------------------------------------------------------- Home

// The website's home: what needs the team now, most important first, each with
// a way to act on it. Built by the server from what Trace already measures.
async function renderHome() {
  const [t, a, analysis] = await Promise.all([api("/today"), api("/audience?days=7").catch(() => null), api("/analysis").catch(() => null)]);
  // The analysis panel says it better than a line in the list.
  if (analysis) t.items = t.items.filter((i) => i.kind !== "analyse");
  const change = t.sessions_week_before
    ? (() => {
        const d = (t.sessions_week - t.sessions_week_before) / t.sessions_week_before;
        if (Math.abs(d) < 0.005) return `<span class="delta">same as the week before</span>`;
        return `<span class="delta ${d > 0 ? "good" : "bad"}">${d > 0 ? "▲" : "▼"} ${Math.abs(d * 100).toFixed(0)}% vs the week before</span>`;
      })()
    : "";
  const items = t.items.length
    ? `<ol class="today">${t.items
        .map(
          (i, n) => `<li class="today-item tone-${esc(i.tone)}">
            <div class="today-text"><strong>${esc(i.title)}</strong><span>${esc(i.detail)}</span></div>
            <a class="button${n === 0 ? " primary" : ""}" href="#/${esc(i.page)}" ${i.kind === "analyse" ? "data-analyse" : ""}>${esc(i.action)}</a>
          </li>`
        )
        .join("")}</ol>`
    : `<div class="card empty-today"><p><strong>Nothing needs you right now.</strong> Trace keeps measuring; this page fills up when there's something to look at.</p></div>`;
  view.innerHTML = `
    <div class="page-head">
      <div><h1>${esc(project.name)}</h1><p class="sub">What needs attention now, most important first.</p></div>
      <div class="week"><span class="value">${num(t.sessions_week)}</span> <span class="hint">sessions in the last 7 days</span> ${change}</div>
    </div>
    ${a && a.visitors.sessions ? audienceStrip(a) : ""}
    ${analysis && analysis.next === "analyse" ? analysisPanel(analysis) : ""}
    ${items}
    ${analysis && analysis.next !== "analyse" ? analysisPanel(analysis) : ""}`;
  if (analysis) {
    wireAnalysisPanel(analysis);
    analysisBadge(analysis);
  }
  view.querySelector("[data-analyse]")?.addEventListener("click", (e) => {
    const run = document.getElementById("run-analysis");
    if (run && !run.hidden) {
      e.preventDefault();
      run.click();
    }
  });
}

// The audience at a glance on Home, last 7 days; the Audience page has the rest.
function audienceStrip(a) {
  const link = (body) => `<a class="card stat link" href="#/audience">${body}</a>`;
  const cell = (value, label, extra = "") => link(`<div class="value">${value}</div><div class="label">${label}</div>${extra ? `<div class="hint">${extra}</div>` : ""}`);
  return `<div class="stats audience-strip">
    ${cell(num(a.visitors.visitors), "Visitors", change(a.visitors.visitors, a.visitors.previous_visitors, { neutral: true }))}
    ${a.bounce.sessions ? cell(pct(a.bounce.rate), "Bounce on marketing pages", change(a.bounce.rate, a.bounce.previous_rate, { rate: true, up: false })) : ""}
    ${a.engaged.declared ? cell(pct(a.engaged.rate), "Engaged", change(a.engaged.rate, a.engaged.previous_rate, { rate: true })) : ""}
    ${a.active.identified ? cell(num(Math.round(a.active.dau * 10) / 10), "Daily active people", `of ${num(a.active.mau)} a month · ${pct(a.active.stickiness)}`) : ""}
  </div>`;
}

// One funnel: each step as a bar sized against the first, the share that got
// there from the step before, and the step losing the most sessions marked.
function funnelCard(f) {
  const first = f.steps[0] ? f.steps[0].sessions : 0;
  const drop = f.biggest_drop ? f.biggest_drop.to_step - 1 : -1;
  const change =
    f.conversion != null && f.previous_conversion != null
      ? deltaHtml({ current: f.conversion, previous: f.previous_conversion, direction: "increase", definition: { conversion: true } })
      : "";
  const steps = f.steps
    .map((s, i) => {
      const width = first ? Math.max(2, (s.sessions / first) * 100) : 0;
      const step = i === 0
        ? `<span class="hint">start</span>`
        : `<strong>${pct(s.of_previous)}</strong> <span class="hint">continue${s.lost ? `, ${num(s.lost)} lost` : ""}${s.previous_of_previous != null ? ` · was ${pct(s.previous_of_previous)}` : ""}</span>`;
      return `<li class="funnel-step${i === drop ? " is-drop" : ""}">
        <div class="funnel-label"><code>${esc(s.label)}</code>${i === drop ? ` <span class="badge bad">biggest drop</span>` : ""}</div>
        <div class="funnel-bar"><span style="width:${width.toFixed(1)}%"></span></div>
        <div class="funnel-nums"><span class="funnel-count">${num(s.sessions)}</span> ${step}</div>
      </li>`;
    })
    .join("");
  const devices = f.by_device.length > 1
    ? `<p class="hint">By device: ${f.by_device.map((d) => `${esc(d.device)} ${pct(d.conversion)}`).join(" · ")}</p>`
    : "";
  return `<div class="card funnel">
    <div class="funnel-head">
      <div><strong>${esc(f.name)}</strong> <span class="hint">${f.source === "kpis" ? "recognised from your KPIs" : "from trace.json"}</span></div>
      <div class="value-row"><span class="value">${pct(f.conversion)}</span> ${change}</div>
    </div>
    ${f.description && f.source !== "kpis" ? `<p class="hint">${esc(f.description)}</p>` : ""}
    <ol class="funnel-steps">${steps}</ol>
    ${first && !f.steps[f.steps.length - 1].sessions ? `<p class="hint">No one has finished this funnel in ${f.days} days yet, so there's no biggest drop to point at.</p>` : ""}
    ${devices}
  </div>`;
}

// Whether a declared event arrives: missing only once the website has
// collected for the whole window, so a new website reads "not seen yet".
function eventStatus(e, since) {
  if (e.status === "receiving") return `<span class="status-dot good">● receiving</span> <span class="hint">${esc(ago(e.last_seen))}</span>`;
  if (e.status === "stopped") return `<span class="status-dot bad">● stopped</span> <span class="hint">last seen ${esc(ago(e.last_seen))}</span>`;
  if (e.status === "missing") return `<span class="status-dot bad">● never arrived</span> <span class="hint">in 14 days</span>`;
  return `<span class="status-dot">● not seen yet</span>${since ? ` <span class="hint">first visit ${esc(ago(since))}</span>` : ""}`;
}

function funnelsSection(funnels) {
  const intro = `<p class="sub">Steps people take in order within one visit, over the last 14 days. Each step counts the sessions that reached it after the step before; once someone finishes, the biggest drop is where more conversion is won, and where analysis looks first.</p>`;
  if (!funnels.length)
    return `<h2>Funnels</h2>${intro}<div class="card"><p>No funnels yet. Declare them in <code>trace.json</code>, or define conversion KPIs whose goal is the next one's start and Trace links them up.</p>
      <div class="copy-row"><pre class="snippet">${esc(FUNNEL_EXAMPLE)}</pre><button data-copy="${esc(FUNNEL_EXAMPLE)}">Copy</button></div></div>`;
  return `<h2>Funnels</h2>${intro}<div class="funnel-grid">${funnels.map(funnelCard).join("")}</div>`;
}

const FUNNEL_EXAMPLE = `"funnels": {
  "checkout": {
    "name": "Checkout",
    "steps": [
      { "pageview": "/cart" },
      { "pageview": "/checkout" },
      { "event": "purchase" }
    ]
  }
}`;

async function renderKpis() {
  const [kpis, cfg] = await Promise.all([api("/kpis"), api("/config")]);
  kpiCache = kpis;
  const push = `trace push trace.json --server ${location.origin} --token $TRACE_TOKEN   # create one under Account`;
  const source = cfg.synced_at
    ? `<span class="source" title="To change a goal, edit trace.json in your repository and push it."><code>${esc(cfg.source || "trace.json")}</code> · synced ${esc(ago(cfg.synced_at))}</span>`
    : "";
  const cards = kpis.length
    ? `<div class="kpi-grid">${kpis.map(kpiCard).join("")}</div>`
    : `<div class="card">
        <p><strong>No goals yet.</strong> Goals (KPIs) live in your application's code, next to the events that feed them. Add a <code>trace.json</code> to your repository:</p>
        ${snippet(CONFIG_EXAMPLE)}
        <p class="hint">Check it with <code>trace check</code>, then publish it, for example from CI:</p>
        ${snippet(push)}
      </div>`;
  const steps = `trace check\ntrace push --server ${location.origin} --site ${project.slug}`;
  view.innerHTML = `
    <div class="page-head">
      <div><h1>Goals</h1><p class="sub">The numbers you're trying to move, over the last 14 days against the 14 before. They're defined in trace.json; released insights are judged against them.</p></div>
      ${source}
    </div>
    ${cards}
    <div class="dark-panel" ${PUBLIC ? "hidden" : ""}>
      <strong>Change a goal in your repo</strong>
      <span>Edit <code>trace.json</code> next to the code that sends the events, then:</span>
      <pre>${esc(steps)}</pre>
      <span class="hint">Run them in CI with a personal API token (<code>TRACE_TOKEN</code>), so the dashboard always matches your main branch. Whether each event arrives is under <a href="#/setup/install">Setup › Install and tracking</a>.</span>
    </div>`;
  wireCopy();
}

async function renderFunnels() {
  const funnels = await api("/funnels");
  view.innerHTML = funnelsSection(funnels).replace("<h2>Funnels</h2>", "<h1>Funnels</h1>");
  wireCopy();
}

// Declared events and whether each arrives, for Setup › Install and tracking.
async function trackingSection() {
  const cfg = await api("/config");
  const declared = cfg.events.map((e) => [`<code>${esc(e.name)}</code>`, esc(e.description), num(e.count), eventStatus(e, cfg.collecting_since)]);
  const undeclared = cfg.undeclared_events.map((e) => [
    `<code>${esc(e.name)}</code>`,
    `<span class="hint">Sent by your app but not declared in trace.json</span>`,
    num(e.count),
    `<span class="status-dot">● undeclared</span>`,
  ]);
  return `<h2 id="tracking">Tracking</h2>
    <p class="sub">The custom events trace.json declares, and whether each arrives. Send them with <code>trace.track("name")</code>, or without JavaScript with <code>data-trace-event="name"</code> on a button, link or form.</p>
    ${table([{ label: "Event" }, { label: "What it means" }, { label: "Last 14 days", num: 1 }, { label: "Status" }], [...declared, ...undeclared], "No custom events declared or received yet.")}`;
}

async function renderInstall() {
  await renderSetup();
  view.insertAdjacentHTML("beforeend", await trackingSection());
}

function wireCopy() {
  view.querySelectorAll("[data-copy]").forEach((b) => {
    if (b.dataset.wired) return;
    b.dataset.wired = "1";
    b.addEventListener("click", async () => {
      if (await copyText(b.dataset.copy)) {
        toast("Copied");
        measure("copied", { what: b.dataset.copy.startsWith(ANALYSE_COMMAND) ? "analyse" : b.dataset.task ? "task" : "snippet" });
      }
    });
  });
}
