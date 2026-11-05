"use strict";

// Journeys: how people get through the website, step by step, where they stop,
// and where they arrive and leave. One funnel at a time, as columns.
// The dashboard is plain scripts loaded in order (see index.html); they share one scope.

let journeysDays = [7, 14, 30].includes(Number(store("trace_journeys_days"))) ? Number(store("trace_journeys_days")) : 14;
let journeysFunnel = store("trace_journeys_funnel") || null;

const hasSessions = (f) => f.steps.length && f.steps[0].sessions > 0;

function funnelColumns(f) {
  const first = f.steps[0].sessions || 1;
  const drop = f.biggest_drop ? f.biggest_drop.to_step - 1 : -1;
  return html`<ol class="journey-steps">${f.steps.map((s, i) => {
    const prev = i ? f.steps[i - 1].sessions : s.sessions;
    return html`<li class="journey-step${i === drop ? " drop" : ""}">
      <div class="journey-step-head"><span class="journey-n" aria-hidden="true">${i + 1}</span><code>${s.label}</code></div>
      <div class="journey-bar"><span style="height:${Math.max(3, (s.sessions / first) * 100).toFixed(1)}%"><b>${num(s.sessions)}</b></span></div>
      <div class="journey-step-foot">
        ${i === 0
          ? html`<span class="hint">everyone who started</span>`
          : html`<span><strong>${pct(s.of_previous)}</strong> carried on${s.previous_of_previous != null ? html` <span class="hint">· was ${pct(s.previous_of_previous)}</span>` : ""}</span>
            <span class="${i === drop ? "bad-text" : "hint"}">${num(prev - s.sessions)} stopped here${i === drop ? " · biggest drop" : ""}</span>`}
      </div>
    </li>`;
  })}</ol>`;
}

function edgeList(rows, kind, total) {
  if (!rows || !rows.length) return html`<p class="hint">No sessions in this window.</p>`;
  const max = Math.max(...rows.filter((r) => r.path).map((r) => r.sessions), 1);
  return html`<ul class="journey-edges">${rows.map(
    (r) => html`<li>
      <div class="journey-edge-head">${r.path ? pageLink(r.path) : html`<span class="hint">Other pages (each under 5 sessions, or beyond the top 10)</span>`}<span>${num(r.sessions)}</span></div>
      ${r.path ? html`<div class="funnel-bar ${kind === "exits" ? "warn" : ""}"><span style="width:${((r.sessions / max) * 100).toFixed(1)}%"></span></div>` : ""}
      ${kind === "arrivals" && r.path && r.one_page ? html`<span class="hint">${num(r.one_page)} of ${num(r.sessions)} saw no second page</span>` : ""}
    </li>`
  )}</ul>${total ? html`<p class="hint">Of ${num(total)} sessions in the window.</p>` : ""}`;
}

async function renderJourneys() {
  const days = journeysDays;
  // The public view has funnels only.
  const data = PUBLIC ? { funnels: await api(`/funnels?days=${days}`) } : await api(`/journeys?days=${days}`);
  if (days !== journeysDays) return;
  const funnels = data.funnels;
  const live = funnels.filter(hasSessions);
  const current = live.find((f) => f.key === journeysFunnel) || live[0] || null;
  const sessions = data.arrivals ? data.arrivals.reduce((n, r) => n + r.sessions, 0) : 0;
  view.innerHTML = html`
    <div class="pulse-head">
      <div><h1>Journeys</h1><p class="sub">How people get through the website, step by step, and where they stop.</p></div>
      <div class="segmented" role="group" aria-label="Period">${[7, 14, 30].map((n) => html`<button type="button" data-days="${n}" aria-pressed="${n === days ? "true" : "false"}">${n} days</button>`)}</div>
    </div>
    ${funnels.length
      ? html`<div class="journey-picker" role="group" aria-label="Funnels">${funnels.map((f) => {
          const on = current && f.key === current.key;
          return hasSessions(f)
            ? html`<button type="button" data-funnel="${f.key}" aria-pressed="${on ? "true" : "false"}"><span>${f.name}</span><span class="journey-conv">${pct(f.conversion)}</span></button>`
            : html`<button type="button" disabled title="No sessions reached its first step in this window"><span>${f.name}</span><span class="journey-conv">collecting</span></button>`;
        })}</div>`
      : ""}
    ${current
      ? html`<section class="card journey">
          <div class="journey-head">
            <div><h2>${current.name}</h2><p class="hint">${current.description || ""}${current.source === "kpis" ? " Recognised from your goals." : ""}</p></div>
            <div class="journey-total"><span class="pulse-tile-value">${pct(current.conversion)}</span> <span class="hint">get to the end</span> ${raw(
              current.conversion != null && current.previous_conversion != null ? deltaHtml({ current: current.conversion, previous: current.previous_conversion, direction: "increase" }) : ""
            )}</div>
          </div>
          <div class="journey-scroll">${funnelColumns(current)}</div>
          ${current.biggest_drop
            ? html`<p class="journey-note"><strong>${num(current.biggest_drop.lost)} sessions stopped between <code>${current.biggest_drop.from_label}</code> and <code>${current.biggest_drop.to_label}</code></strong>: the step that loses the most, and where analysis looks first.</p>`
            : current.steps[current.steps.length - 1].sessions
            ? ""
            : html`<p class="hint">No one finished this funnel in ${current.days} days yet, so there's no biggest drop to point at.</p>`}
          ${current.by_device.length > 1 ? html`<p class="hint">By device: ${current.by_device.map((d) => `${d.device} ${pct(d.conversion)}`).join(" · ")}</p>` : ""}
        </section>`
      : funnels.length
      ? html`<div class="card"><p>No funnel had sessions in the last ${days} days.</p></div>`
      : html`<div class="card"><p>No funnels yet. Declare them in <code>trace.json</code>, or define conversion goals whose goal is the next one's start and Trace links them up.</p>${raw(snippet(FUNNEL_EXAMPLE))}</div>`}
    ${data.arrivals
      ? html`<div class="journey-pair">
          <section class="card"><h2>Where people arrive</h2><p class="hint">The first page of a session.</p>${edgeList(data.arrivals, "arrivals", sessions)}</section>
          <section class="card"><h2>Where people leave</h2><p class="hint">The last page of a session.</p>${edgeList(data.exits, "exits")}</section>
        </div>`
      : ""}`;
  view.querySelectorAll("[data-days]").forEach((b) =>
    b.addEventListener("click", () => {
      journeysDays = Number(b.dataset.days);
      store("trace_journeys_days", String(journeysDays));
      renderJourneys();
    })
  );
  view.querySelectorAll("[data-funnel]").forEach((b) =>
    b.addEventListener("click", () => {
      journeysFunnel = b.dataset.funnel;
      store("trace_journeys_funnel", journeysFunnel);
      renderJourneys();
    })
  );
  wireCopy();
}
