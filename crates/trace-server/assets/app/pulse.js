"use strict";

// Pulse: the website's home. Is it OK, what changed, what's next, with every
// other place in miniature. Built from reports that already exist; each part
// draws on its own and says so when its report fails.
// The dashboard is plain scripts loaded in order (see index.html); they share one scope.

let pulseDays = [7, 14, 30].includes(Number(store("trace_pulse_days"))) ? Number(store("trace_pulse_days")) : 7;

// Where a page key from Home's items lives now.
const itemUrl = (page) => pageUrl(ALIASES[page] || page);

const sourceLabel = (s) => (s.name === "direct" ? "direct" : s.name === "other" ? "other sources" : s.name);

// A goal's progress to its target, so every goal can share one target line:
// 1 is on target, above is past it. "Decrease" goals invert.
function goalRatio(k) {
  if (k.direction === "decrease") return k.current > 0 ? k.target / k.current : 1.25;
  return k.target > 0 ? k.current / k.target : 1.25;
}

// One sentence from the numbers below it; parts without data are left out.
function pulseSummary({ sessions, days, phone, source, behind, measured, worst }) {
  if (!sessions) return `No visits in the last ${days} days yet.`;
  const most = [];
  if (phone != null && phone > 0.5) most.push("mostly on phones");
  if (source && source.share > 0.5) most.push(source.name === "direct" ? "mostly coming in directly" : `mostly from ${source.name}`);
  let text = `${num(sessions)} sessions in the last ${days} days${most.length ? `, ${most.join(" and ").replace(" and mostly", " and")}` : ""}`;
  const more = [];
  if (measured) more.push(behind ? `${behind} of the ${measured} goals with enough data ${behind === 1 ? "is" : "are"} behind target` : `all ${measured} goals with enough data are at target`);
  if (worst) more.push(`people hit the most problems on ${worst}`);
  if (more.length) text += `; ${more.join(", and ")}`;
  return `${text}.`;
}

function pulseTile(label, value, body, foot, href) {
  return html`<a class="card pulse-tile" href="${href}"><span class="pulse-tile-label">${label}</span><span class="pulse-tile-value">${value}</span>${body}${foot ? html`<span class="hint">${foot}</span>` : ""}</a>`;
}

function shareBar(parts, label) {
  return html`<span class="pulse-share" role="img" aria-label="${label}">${parts.map(([w, cls]) => html`<span class="${cls}" style="width:${(w * 100).toFixed(1)}%"></span>`)}</span>`;
}

function trafficPart(a, d) {
  if (!a) return html`<p class="hint">Couldn't load traffic.</p>`;
  const v = a.visitors;
  const days = v.days || [];
  const max = Math.max(1, ...days.map((x) => x.sessions));
  const busiest = days.reduce((b, x) => (!b || x.sessions > b.sessions ? x : b), null);
  const bars = html`<span class="pulse-days" role="img" aria-label="Sessions per day: ${days.map((x) => x.sessions).join(", ")}">${days.map(
    (x, i) => html`<span class="${i === days.length - 1 ? "now" : ""}" style="height:${Math.max(4, (x.sessions / max) * 100).toFixed(0)}%" title="${x.day}: ${x.sessions}"></span>`
  )}</span>`;
  const sources = (a.sources || []).filter((s) => s.name !== "other");
  const top = sources[0];
  const rest = (a.sources || []).slice(1);
  const devices = d ? d.devices : [];
  const total = d ? d.totals.sessions : 0;
  const phone = total ? (devices.find((x) => x.name === "mobile")?.count || 0) / total : null;
  const desktop = total ? (devices.find((x) => x.name === "desktop")?.count || 0) / total : 0;
  const b = a.bounce;
  return html`<section class="pulse-traffic" aria-label="Traffic">
    ${pulseTile("Sessions", num(v.sessions), bars, html`${busiest ? `Busiest ${busiest.day} · ${num(busiest.sessions)}` : ""} ${raw(change(v.sessions, v.previous_sessions, { neutral: true }))}`, pageUrl("audience"))}
    ${top
      ? pulseTile(
          top.name === "direct" ? "Come in directly" : `From ${top.name}`,
          pct(top.share),
          shareBar([[top.share, "a"], ...rest.slice(0, 2).map((s, i) => [s.share, i ? "c" : "b"])], `Sources: ${(a.sources || []).map((s) => `${sourceLabel(s)} ${pct(s.share)}`).join(", ")}`),
          rest.slice(0, 2).map((s) => `${sourceLabel(s)} ${pct(s.share)}`).join(" · "),
          pageUrl("audience")
        )
      : ""}
    ${phone != null ? pulseTile("On a phone", pct(phone), shareBar([[phone, "a"], [desktop, "b"]], `Phones ${pct(phone)}, desktop ${pct(desktop)}`), `Desktop ${pct(desktop)}`, pageUrl("audience")) : ""}
    ${b.sessions
      ? pulseTile(
          b.pages.length ? "Bounce on marketing pages" : "Leave without doing anything",
          pct(b.rate),
          shareBar([[b.rate || 0, "bad"]], `${pct(b.rate)} of sessions`),
          html`${num(b.bounced)} of ${num(b.sessions)} ${raw(change(b.rate, b.previous_rate, { rate: true, up: false }))}`,
          pageUrl("audience")
        )
      : ""}
  </section>`;
}

function goalsPart(kpis) {
  if (!kpis) return html`<p class="hint">Couldn't load goals.</p>`;
  const targeted = kpis.filter((k) => k.target != null);
  const measured = targeted.filter((k) => k.current != null && (k.sessions || 0) >= SAMPLE_MIN).sort((x, y) => goalRatio(x) - goalRatio(y));
  const at = measured.filter((k) => k.target_met).length;
  const collecting = targeted.length - measured.length;
  if (!kpis.length)
    return html`<p>No goals yet. They live in <code>trace.json</code> in your repository; <a href="${pageUrl("goals")}">Goals</a> shows how.</p>`;
  const row = (k) => {
    const r = goalRatio(k);
    const tone = k.target_met ? "good" : r < 0.7 ? "bad" : "warn";
    return html`<div class="pulse-goal">
      <span class="pulse-goal-name">${k.name}</span>
      <span class="pulse-goal-track" role="img" aria-label="${k.name}: ${kpiValue(k, k.current)}, target ${kpiValue(k, k.target)}"><span class="${tone}" style="width:${((Math.min(r, 1.25) / 1.25) * 100).toFixed(1)}%"></span><i></i></span>
      <span class="pulse-goal-num"><strong class="${tone}-text">${kpiValue(k, k.current)}</strong> <span class="hint">/ ${kpiValue(k, k.target)}</span></span>
    </div>`;
  };
  return html`<div class="pulse-goal-head hint">${measured.length ? `${at} at target · ${measured.length - at} behind` : "None has enough data yet"}${collecting ? ` · ${collecting} still collecting` : ""}</div>
    ${measured.length ? html`<div class="pulse-goals"><div class="pulse-goal pulse-goal-scale" aria-hidden="true"><span></span><span class="pulse-goal-track bare"><b>target</b></span><span class="hint">now / target</span></div>${measured.map(row)}</div>
      <p class="hint">Each bar is a goal against its own target: past the line is on target.</p>` : ""}`;
}

function nextPart(t) {
  if (!t) return html`<p class="hint">Couldn't load what needs attention.</p>`;
  if (!t.items.length) return html`<div class="card"><p><strong>Nothing needs you right now.</strong> Trace keeps measuring; this fills up when there's something to look at.</p></div>`;
  return html`${t.items.slice(0, 3).map(
    (i, n) => html`<a class="card pulse-next tone-${i.tone}${n === 0 ? " first" : ""}" href="${itemUrl(i.page)}" ${i.kind === "analyse" ? raw("data-analyse") : ""}>
      <strong>${i.title}</strong><span>${i.detail}</span><span class="pulse-next-action">${i.action} →</span>
    </a>`
  )}`;
}

function journeysPart(funnels) {
  if (!funnels) return html`<p class="hint">Couldn't load funnels.</p>`;
  const live = funnels.filter((f) => f.steps.length && f.steps[0].sessions > 0).slice(0, 2);
  if (!live.length) return html`<p class="hint">No funnel has sessions in this window yet.${funnels.length ? "" : " Declare funnels in trace.json, or conversion goals that chain into each other."}</p>`;
  return html`<div class="pulse-funnels">${live.map((f) => {
    const first = f.steps[0].sessions;
    const drop = f.biggest_drop ? f.biggest_drop.to_step - 1 : -1;
    return html`<div class="pulse-funnel">
      <div class="pulse-funnel-head"><strong>${f.name}</strong><span>${pct(f.conversion)} get through</span></div>
      <div class="pulse-columns" role="img" aria-label="${f.steps.map((s) => `${s.label} ${s.sessions}`).join(", ")}">${f.steps.map(
        (s, i) => html`<div><span class="hint">${num(s.sessions)}</span><span class="col${i === drop ? " drop" : ""}" style="height:${Math.max(3, (s.sessions / first) * 100).toFixed(0)}%"></span></div>`
      )}</div>
      <div class="pulse-labels">${f.steps.map((s) => html`<span>${s.label}</span>`)}</div>
    </div>`;
  })}</div>`;
}

const AREA_TONE = { painful: "hurts", rare_painful: "hurts", rare: "warn", healthy: "ok", too_little: "none" };

function mapPart(areas) {
  if (!areas) return html`<p class="hint">Couldn't load the product map.</p>`;
  const tops = areas.nodes.filter((n) => !n.area).sort((x, y) => y.sessions - x.sessions);
  if (!tops.length) return html`<p class="hint">No areas yet.</p>`;
  return html`<div class="pulse-map">${tops.map(
    (n) => html`<a href="${pageUrl("features")}/${encodeURIComponent(n.key)}" class="pulse-area ${AREA_TONE[n.status] || "none"}" style="flex-grow:${Math.max(1, n.sessions)}">
      <strong>${n.name}</strong><span>${num(n.sessions)} sessions</span><span class="pulse-area-status">${(AREA_STATUS[n.status] || [n.status])[0]}</span>
    </a>`
  )}</div>${areas.source === "suggested" ? html`<p class="hint">Suggested from your URLs: name your areas in trace.json.</p>` : ""}`;
}

function pulseSection(title, href, linkText, body, extra = "") {
  return html`<section class="pulse-section card">
    <div class="pulse-section-head"><h2>${title}</h2>${extra}${href ? html`<a href="${href}">${linkText} →</a>` : ""}</div>
    ${body}
  </section>`;
}

async function renderPulse() {
  const days = pulseDays;
  const soft = (p) => api(p).catch(() => null);
  const [t, a, kpis, funnels, areas, d, analysis] = await Promise.all([
    soft("/today"),
    soft(`/audience?days=${days}`),
    soft(`/kpis?days=${days}`),
    soft(`/funnels?days=${days}`),
    soft(`/areas?days=${days}`),
    soft(`/digest?days=${days}`),
    soft("/analysis"),
  ]);
  if (days !== pulseDays) return;
  kpiCache = kpis || kpiCache;
  const sessions = a ? a.visitors.sessions : t ? t.sessions_week : 0;
  const total = d ? d.totals.sessions : 0;
  const phone = total ? (d.devices.find((x) => x.name === "mobile")?.count || 0) / total : null;
  const measured = (kpis || []).filter((k) => k.target != null && k.current != null && (k.sessions || 0) >= SAMPLE_MIN);
  const hurt = {};
  if (d) for (const x of [...d.dead_clicks, ...d.rage_clicks, ...d.errors]) if (x.path) hurt[x.path] = Math.max(hurt[x.path] || 0, x.sessions);
  const worst = Object.entries(hurt).sort((x, y) => y[1] - x[1])[0];
  const summary = pulseSummary({
    sessions,
    days,
    phone,
    source: a && (a.sources || []).find((s) => s.name !== "other"),
    behind: measured.filter((k) => !k.target_met).length,
    measured: measured.length,
    worst: worst ? worst[0] : null,
  });
  const empty = !sessions;
  view.innerHTML = html`
    <div class="pulse-head">
      <div><h1>Pulse</h1><p class="sub">${project.name}</p></div>
      <div class="segmented" role="group" aria-label="Period">${[7, 14, 30].map((n) => html`<button type="button" data-days="${n}" aria-pressed="${n === days ? "true" : "false"}">${n} days</button>`)}</div>
    </div>
    <p class="pulse-summary">${summary}</p>
    ${empty ? "" : trafficPart(a, d)}
    <div class="pulse-pair">
      ${empty ? "" : pulseSection("Goals", pageUrl("goals"), "All goals", goalsPart(kpis))}
      <section class="pulse-next-list" aria-label="Next up"><h2>Next up</h2>${nextPart(t && analysis && analysis.next === "analyse" ? { ...t, items: t.items.filter((i) => i.kind !== "analyse") } : t)}</section>
    </div>
    ${empty ? "" : pulseSection("Journeys", pageUrl("funnels"), "Open Journeys", journeysPart(funnels))}
    ${empty ? "" : pulseSection("Product map", pageUrl("features"), "Open Product map", mapPart(areas), html`<span class="hint">size is sessions, colour is how often something goes wrong</span>`)}
    ${analysis && analysis.next === "analyse" ? raw(analysisPanel(analysis)) : ""}`;
  view.querySelectorAll("[data-days]").forEach((b) =>
    b.addEventListener("click", () => {
      pulseDays = Number(b.dataset.days);
      store("trace_pulse_days", String(pulseDays));
      renderPulse();
    })
  );
  if (analysis) {
    if (analysis.next === "analyse") wireAnalysisPanel(analysis);
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
