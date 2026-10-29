"use strict";

// Audience: visitors, bounce, engagement, active people and retention.
// The dashboard is plain scripts loaded in order (see index.html); they share one scope.

// ---------------------------------------------------------------- Audience

let audienceDays = Number(store("trace_audience_days")) || 30;

// A change against the window before: rates in points, counts in percent.
// Colour follows the goal, not the arrow, and says so in words; numbers
// without a goal (visitors, sessions) are neutral.
function change(current, previous, { rate = false, up = true, neutral = false } = {}) {
  if (current == null || previous == null) return "";
  if (!rate && !previous) return current ? `<span class="delta">new</span>` : "";
  const d = rate ? current - previous : (current - previous) / previous;
  if (Math.abs(d) < (rate ? 0.005 : 0.005)) return `<span class="delta">no change</span>`;
  const text = rate ? `${(Math.abs(d) * 100).toFixed(1)} pts` : `${Math.abs(d * 100).toFixed(0)}%`;
  const better = (d > 0) === up;
  return `<span class="delta ${neutral ? "neutral" : better ? "good" : "bad"}">${d > 0 ? "▲" : "▼"} ${text}${neutral ? "" : better ? ", better" : ", worse"}</span>`;
}

const AUDIENCE_EXAMPLE = `"audience": {
  "marketing": ["/", "/pricing", "/blog/*"],
  "engaged": [{ "event": "signup" }, { "event": "login" }]
}`;

const IDENTIFY_EXAMPLE = `<script>trace.identify("<%= hmac(current_user.id) %>")</script>`;

function snippet(text) {
  return html`<div class="copy-row"><pre class="snippet">${text}</pre><button data-copy="${text}">Copy</button></div>`;
}

// One bar per day of the window, empty days included.
function visitorBars(days, window) {
  const by = Object.fromEntries(days.map((d) => [d.day, d]));
  const list = [];
  for (let i = window - 1; i >= 0; i--) {
    const day = new Date(Date.now() - i * 86400000).toISOString().slice(0, 10);
    list.push(by[day] || { day, visitors: 0, sessions: 0, signed_in: 0 });
  }
  const max = Math.max(1, ...list.map((d) => d.visitors));
  const every = Math.ceil(list.length / 8);
  return `<div class="card day-bars">${list
    .map(
      (d, i) => `<div class="day-bar" title="${esc(d.day)}: ${num(d.visitors)} visitors, ${num(d.sessions)} sessions${d.signed_in ? `, ${num(d.signed_in)} signed in` : ""}">
        <span style="height:${d.visitors ? Math.max(2, (d.visitors / max) * 100).toFixed(1) : 0}%"></span><small>${i % every === 0 ? esc(d.day.slice(5)) : "&nbsp;"}</small></div>`
    )
    .join("")}</div>`;
}

function stat(value, label, extra = "", hint = "") {
  return `<div class="card stat"${hint ? ` title="${esc(hint)}"` : ""}><div class="value">${value}</div><div class="label">${label}</div>${extra ? `<div class="hint">${extra}</div>` : ""}</div>`;
}

function retentionTable(r) {
  if (!r.cohorts.length) return `<p class="hint">No one signed in during the last 8 weeks yet.</p>`;
  const weeks = r.average.length;
  const cell = (v) =>
    v == null ? `<td class="num muted">·</td>` : `<td class="num heat" style="--heat:${(v * 55).toFixed(0)}%">${(v * 100).toFixed(0)}%</td>`;
  const head = `<tr><th>First signed in</th><th class="num">People</th>${Array.from({ length: weeks }, (_, i) => `<th class="num">Week ${i + 1}</th>`).join("")}</tr>`;
  const rows = r.cohorts.map((c) => `<tr><td>Week of ${esc(c.week)}</td><td class="num">${num(c.people)}</td>${c.returned.map(cell).join("")}</tr>`).join("");
  const avg = `<tr class="total"><td>Average</td><td></td>${r.average.map(cell).join("")}</tr>`;
  return `<div class="table-wrap"><table class="retention">${head}${rows}${avg}</table></div>`;
}

// Who uses the website: visitors, bounce on the marketing pages, engagement,
// and for signed-in people DAU/MAU and retention. Counts only.
async function renderAudience() {
  const a = await api(`/audience?days=${audienceDays}`);
  const v = a.visitors;
  const b = a.bounce;
  const e = a.engaged;
  const act = a.active;
  const visitorsHint = "An anonymous visitor counts once a day: nothing is stored on their device to recognise them by. Signed-in people count once.";
  const bounce = b.sessions
    ? `<div class="stats">
        ${stat(pct(b.rate), "Bounce rate", `${num(b.bounced)} of ${num(b.sessions)} sessions ${change(b.rate, b.previous_rate, { rate: true, up: false })}`)}
        ${b.by_device.map((d) => stat(pct(d.rate), esc(d.device), `${num(d.sessions)} sessions`)).join("")}
      </div>
      ${table(
        [{ label: "Landing page" }, { label: "Sessions", num: 1 }, { label: "Bounced", num: 1 }, { label: "Bounce rate", num: 1 }],
        b.by_page.map((p) => [esc(p.path), num(p.sessions), num(p.bounced), pct(p.rate)])
      )}`
    : `<p class="hint">No sessions landed on ${b.pages.length ? "these pages" : "the website"} in this window.</p>`;
  const engaged = e.declared
    ? `<div class="stats">
        ${stat(pct(e.rate), "Engaged visitors", `${num(e.visitors)} of ${num(v.visitors)} ${change(e.rate, e.previous_rate, { rate: true })}`, visitorsHint)}
        ${act.identified ? stat(num(e.people), "Signed-in people engaged") : ""}
      </div>
      ${table([{ label: "Action" }, { label: "Sessions", num: 1 }], e.by_action.map((x) => [`<code>${esc(x.label)}</code>`, num(x.sessions)]))}`
    : `<div class="card"><p>Say which actions make a visitor engaged, a sign-up, a sign-in or a first real use, under <code>audience</code> in <code>trace.json</code>:</p>${snippet(AUDIENCE_EXAMPLE)}</div>`;
  const active = act.identified
    ? `<div class="stats">
        ${stat(num(Math.round(act.dau * 10) / 10), "DAU", "average over the last 7 days")}
        ${stat(num(act.wau), "WAU", "last 7 days")}
        ${stat(num(act.mau), "MAU", "last 30 days")}
        ${stat(pct(act.stickiness), "DAU ÷ MAU", "how often people come back")}
      </div>
      <h3>Retention</h3>
      <p class="sub">People grouped by the week they first signed in, and the share who came back in each week after. A week shows once it's over.</p>
      ${retentionTable(a.retention)}`
    : `<div class="card"><p><strong>Needs sign-in.</strong> Anonymous visitors can't be followed from one day to the next, so DAU, MAU and retention count people your app signs in. Call <code>trace.identify()</code> on every page while someone is signed in, with an id that isn't their email: a keyed hash of their user id is best.</p>${snippet(IDENTIFY_EXAMPLE)}</div>`;
  view.innerHTML = `
    <div class="toolbar">
      <div><h1>Audience</h1><p class="sub">How many people come, how many leave straight away, how many get engaged and how many come back. Counts only, never a person.</p></div>
      <label class="field narrow">Window
        <select id="audience-days">${[7, 14, 30, 90].map((n) => `<option value="${n}" ${n === audienceDays ? "selected" : ""}>Last ${n} days</option>`).join("")}</select>
      </label>
    </div>
    <h2>Visitors</h2>
    <div class="stats">
      ${stat(num(v.visitors), "Visitors", change(v.visitors, v.previous_visitors, { neutral: true }), visitorsHint)}
      ${stat(num(Math.round(v.daily_average * 10) / 10), "A day on average")}
      ${stat(num(v.sessions), "Sessions", change(v.sessions, v.previous_sessions, { neutral: true }))}
      ${act.identified ? stat(num(v.signed_in), "Signed-in people") : ""}
    </div>
    ${visitorBars(v.days, a.days)}
    <h2>Bounce on the marketing pages</h2>
    <p class="sub">Sessions that landed on ${b.pages.length ? b.pages.map((p) => `<code>${esc(p)}</code>`).join(", ") : "any page (declare the marketing pages under <code>audience.marketing</code> in trace.json)"} and left without clicking, filling in a form or going to another page.</p>
    ${bounce}
    <h2>Engaged</h2>
    <p class="sub">Visitors who did at least one of the actions trace.json names as engaging.</p>
    ${engaged}
    <h2>Active people</h2>
    ${active}`;
  view.querySelector("#audience-days").addEventListener("change", (ev) => {
    audienceDays = Number(ev.target.value);
    store("trace_audience_days", audienceDays);
    renderAudience();
  });
  view.querySelectorAll("[data-copy]").forEach((b) =>
    b.addEventListener("click", async () => {
      if (await copyText(b.dataset.copy)) toast("Copied");
    })
  );
}
