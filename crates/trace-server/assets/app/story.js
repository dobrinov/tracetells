"use strict";

// Page story: how people use one page, as patterns, never single visits.
// Reached from the Pages tables, area pages and the error list (#/map/pages/<path>).
// The dashboard is plain scripts loaded in order (see index.html); they share one scope.

let storyDays = Math.min(90, Number(store("trace_days")) || 14);
let storyPlay = null;

// The address of a page's story, from anywhere a page path is shown.
function storyUrl(path) {
  return `#/map/pages/${encodeURIComponent(path)}`;
}
// A page path as a link to its story (plain text in the public view, which has no stories).
function pageLink(path) {
  if (PUBLIC || !path || path[0] !== "/") return html`<code>${path}</code>`;
  return html`<a href="${storyUrl(path)}"><code>${path}</code></a>`;
}

const ACTION_KIND = { form_start: "Starts the form", submit: "Submits the form" };

function actionText(a) {
  if (a.kind === "click") return a.text ? `“${a.text}”` : a.selector;
  if (a.kind === "error") return `Error (${a.name})`;
  if (a.kind === "custom") return `Event ${a.name}`;
  return ACTION_KIND[a.kind] || a.kind;
}

function stepChip(step, now, total) {
  const dead = step.did_nothing_sessions > 0;
  return html`<li class="story-step${dead && step.did_nothing_sessions * 4 >= total ? " dead" : ""}${now ? " now" : ""}">
    ${step.actions.map((a, i) => html`${i ? html`<span class="story-then" aria-hidden="true">+</span>` : ""}<span class="story-action"><span class="story-kind">${a.kind === "click" ? "Click" : ""}</span> ${actionText(a)}</span>`)}
    ${dead ? html`<span class="badge bad">did nothing in ${num(step.did_nothing_sessions)} of ${num(total)}</span>` : ""}
    ${step.repeated ? html`<span class="story-again" title="Most of these sessions did this more than once in a row">↻ again and again</span>` : ""}
  </li>`;
}

function patternCard(p, i, s) {
  const playing = storyPlay && storyPlay.i === i;
  if (!p.steps.length)
    return html`<div class="card story-pattern"><div class="story-pattern-head"><strong>Looked, did nothing here</strong><span class="value">${num(p.sessions)} <span class="hint">of ${num(s.sessions)}</span></span></div>
      <p class="hint">Sessions that viewed the page and left or moved on without clicking.</p></div>`;
  return html`<div class="card story-pattern${p.goes_wrong ? " goes-wrong" : ""}">
    <div class="story-pattern-head">
      <strong>${p.goes_wrong ? "Where it goes wrong" : "How it's used"}</strong>
      ${p.repeats ? html`<span class="chip">done again and again</span>` : ""}
      <span class="value">${num(p.sessions)} <span class="hint">of ${num(s.sessions)} sessions</span></span>
      <button type="button" data-play="${i}" aria-pressed="${playing ? "true" : "false"}">${playing ? "Stop" : "Play"}</button>
    </div>
    <div class="funnel-bar"><span style="width:${Math.max(2, (p.sessions / s.sessions) * 100).toFixed(1)}%"></span></div>
    <ol class="story-steps">${p.steps.map((st, k) => stepChip(st, playing && storyPlay.step === k, p.sessions))}</ol>
  </div>`;
}

function spreadText(t, unit = "s") {
  if (!t) return "–";
  const f = (v) => (v >= 120 ? `${Math.round(v / 60)} min` : `${Math.round(v)}${unit}`);
  return `${f(t.median)}`;
}

async function renderStory(path) {
  const s = await api(`/pages/story?path=${encodeURIComponent(path)}&days=${storyDays}`);
  clearInterval(storyPlay && storyPlay.timer);
  if (storyPlay && storyPlay.path !== path) storyPlay = null;
  const origins = (project.collection?.sources || []).map((x) => x.origin);
  const plain = !/[:*]/.test(path);
  const suggested = origins.length ? origins[0] + (plain ? path : "") : "";
  const elements = s.elements.map((e) => [
    html`${e.text ? html`<strong>${e.text}</strong><br>` : ""}<code>${e.selector}</code>`,
    html`<div class="story-share"><div class="funnel-bar"><span style="width:${(e.share * 100).toFixed(1)}%"></span></div><span>${pct(e.share)}</span></div>`,
    num(e.clicks_per_session),
    e.dead_sessions ? html`<span class="${e.dead_share >= 0.25 ? "delta bad" : ""}">${pct(e.dead_share)}</span> <span class="hint">of ${num(e.sessions)}</span>` : "–",
  ]);
  const neighbour = (rows, outside) => rows.map((n) => [n.path ? pageLink(n.path) : html`<span class="hint">${outside}</span>`, num(n.sessions), pct(n.share)]);
  const problems = s.problems.map((p) => [
    html`<span class="chip">${{ dead_click: "Dead click", rage_click: "Rage click", error: "Error" }[p.kind] || p.kind}</span>`,
    p.kind === "error" ? html`${p.text}${p.error_kind ? html` <span class="hint">(${p.error_kind})</span>` : ""}` : html`${p.text ? html`<strong>${p.text}</strong> ` : ""}<code>${p.selector || ""}</code>`,
    num(p.events),
    num(p.sessions),
  ]);
  const hiddenNote = (n, what) => (n ? html`<p class="hint">${n} more ${what}${n === 1 ? " is" : "s are"} hidden: each from fewer than ${s.min_sessions} sessions.</p>` : "");

  view.innerHTML = html`
    <div class="toolbar">
      <div><p class="hint"><a href="#/map/pages">Pages</a> ›</p><h1><code>${path}</code></h1>
        <p class="sub">How ${num(s.sessions)} session${s.sessions === 1 ? "" : "s"}${s.capped ? " (the latest)" : ""} used this page, as patterns. No visit is shown on its own: nothing from fewer than ${s.min_sessions} sessions appears.</p></div>
      <label class="field narrow">Window
        <select id="story-days">${[7, 14, 30, 90].map((n) => html`<option value="${n}" ${n === storyDays ? raw("selected") : ""}>Last ${n} days</option>`)}</select>
      </label>
    </div>
    ${s.too_little_data
      ? html`<div class="card"><p><strong>Too few sessions here yet.</strong> Patterns appear once at least ${s.min_sessions} sessions viewed the page in the window. Try a longer window.</p></div>`
      : html`
    <div class="stats">
      <div class="card stat"><div class="value">${num(s.sessions)}</div><div class="label">Sessions</div></div>
      <div class="card stat"><div class="value">${spreadText(s.time_on_page)}</div><div class="label">Typical time here</div></div>
      <div class="card stat"><div class="value">${s.scroll.length ? pct(s.scroll[s.scroll.length - 1].share) : "–"}</div><div class="label">Scroll to the end</div></div>
      <div class="card stat"><div class="value">${spreadText(s.time_to_first_action)}</div><div class="label">Until the first click</div></div>
    </div>

    <h2>How people use it</h2>
    <p class="sub">Runs of actions that at least ${s.min_sessions} sessions did, anywhere in their visit, with repeats folded together and the ones where a click did nothing first. Play one to step through it${PUBLIC ? "" : ", or show it on the page itself below"}.</p>
    ${s.patterns.length ? html`<div class="story-patterns">${s.patterns.map((p, i) => patternCard(p, i, s))}</div>` : html`<p class="hint">No way of using this page is shared by ${s.min_sessions} or more sessions yet.</p>`}
    ${s.patterns.length ? html`<p class="hint">These patterns cover ${num(s.pattern_sessions)} of ${num(s.sessions)} sessions.${s.more_patterns ? ` ${s.more_patterns} more are common enough but not shown.` : ""}</p>` : ""}
    ${hiddenNote(s.hidden_patterns, "sequence")}

    <h2>What people click</h2>
    ${table([{ label: "Element" }, { label: "Sessions" }, { label: "Clicks each", num: 1 }, { label: "Did nothing", num: 1 }], elements, "No element was used by enough sessions.")}
    ${hiddenNote(s.hidden_elements, "element")}

    <div class="story-columns">
      <div><h2>Came from</h2>${table([{ label: "Page" }, { label: "Sessions", num: 1 }, { label: "Share", num: 1 }], neighbour(s.arrivals, "Outside the website"), "Too few sessions from any one page.")}</div>
      <div><h2>Went next</h2>${table([{ label: "Page" }, { label: "Sessions", num: 1 }, { label: "Share", num: 1 }], neighbour(s.exits, "Left the website"), "Too few sessions to any one page.")}</div>
    </div>`}

    <h2>Problems on this page</h2>
    ${table([{ label: "Kind" }, { label: "What" }, { label: "Events", num: 1 }, { label: "Sessions", num: 1 }], problems, "No dead clicks, rage clicks or errors here in this window.")}

    <h2>Insights about this page</h2>
    ${s.insights.length
      ? html`<ul class="story-insights">${s.insights.map((i) => html`<li><a href="${i.status === "released" ? "#/work/released" : "#/work"}">${i.title}</a> <span class="chip">${i.kind}</span> <span class="hint">${i.status.replace("_", " ")} · ${i.confidence} confidence</span></li>`)}</ul>`
      : html`<p class="hint">No insight names this page yet.</p>`}
    ${PUBLIC ? "" : askAgent(`Use Trace's get_page_story to explain how people use ${path} and what to fix`, "Ask your agent")}

    ${PUBLIC || s.too_little_data ? "" : html`
    <div class="dark-panel story-overlay">
      <strong>Show it on my page</strong>
      <span>Opens your own page in a new tab with these numbers drawn on its elements and the patterns ready to play. Nothing from any visitor's screen is recorded; the link works for 15 minutes.</span>
      <form id="overlay-form" class="copy-row">
        <label class="visually-hidden" for="overlay-url">Address of the page</label>
        <input id="overlay-url" type="url" required placeholder="${origins[0] ? `${origins[0]}/…` : "https://your-website/page"}" value="${suggested}">
        <button class="primary" type="submit">Open</button>
      </form>
      ${origins.length ? html`<span class="hint">On ${origins.map(host).join(", ")}${plain ? "" : `: pick a real address for ${path}`}.</span>` : html`<span class="hint">Trace hasn't seen the collector on any address yet.</span>`}
    </div>`}`;

  view.querySelector("#story-days").addEventListener("change", (e) => {
    storyDays = Number(e.target.value);
    renderStory(path);
  });
  wirePlay(s);
  view.querySelector("#overlay-form")?.addEventListener("submit", async (e) => {
    e.preventDefault();
    const tab = window.open("about:blank", "_blank");
    try {
      const r = await post("/overlay", { url: view.querySelector("#overlay-url").value });
      if (tab) tab.location.href = r.url;
      else location.href = r.url;
      measure("overlay_opened");
    } catch (err) {
      if (tab) tab.close();
      toast(err.message, true);
    }
  });
  wireCopy();
}

// Playing a pattern steps through it at a fixed pace; redrawing only the
// patterns, so playing doesn't reload the page.
function wirePlay(s) {
  view.querySelectorAll("[data-play]").forEach((b) =>
    b.addEventListener("click", () => {
      const i = Number(b.dataset.play);
      const again = storyPlay && storyPlay.i === i;
      clearInterval(storyPlay && storyPlay.timer);
      storyPlay = null;
      if (!again) {
        const play = { i, step: 0, path: s.path, timer: null };
        const steps = s.patterns[i].steps.length;
        play.timer = setInterval(() => {
          if (storyPlay !== play || play.step >= steps - 1) return clearInterval(play.timer);
          play.step++;
          paintPlay(s);
        }, 1200);
        storyPlay = play;
      }
      paintPlay(s);
    })
  );
}

function paintPlay(s) {
  const box = view.querySelector(".story-patterns");
  if (!box || !view.contains(box)) return clearInterval(storyPlay && storyPlay.timer);
  box.innerHTML = html`${s.patterns.map((p, i) => patternCard(p, i, s))}`;
  wirePlay(s);
}
