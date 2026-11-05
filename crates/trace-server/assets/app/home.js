"use strict";

// The funnel cards, Goals and the tracking table other pages use. The website's home is Pulse (pulse.js).
// The dashboard is plain scripts loaded in order (see index.html); they share one scope.

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
      <span class="hint">Run them in CI with a personal API token (<code>TRACE_TOKEN</code>), so the dashboard always matches your main branch. Whether each event arrives is under <a href="#/settings/install">Settings › Install and tracking</a>.</span>
    </div>`;
  wireCopy();
}

async function renderFunnels() {
  const funnels = await api("/funnels");
  view.innerHTML = funnelsSection(funnels).replace("<h2>Funnels</h2>", "<h1>Funnels</h1>");
  wireCopy();
}

// Declared events and whether each arrives, for Settings › Install and tracking.
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
