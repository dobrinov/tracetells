"use strict";

const view = document.getElementById("view");
const toastEl = document.getElementById("toast");

const STATUSES = [
  ["new", "New"],
  ["selected", "Selected"],
  ["in_progress", "In progress"],
  ["released", "Released"],
  ["dismissed", "Dismissed"],
];
const EVENT_TYPES = ["pageview", "click", "rage_click", "dead_click", "error", "form_start", "submit", "custom", "scroll", "leave"];

const esc = (s) =>
  String(s ?? "").replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]);
const pct = (v) => (v == null ? "–" : `${(v * 100).toFixed(1)}%`);
const num = (v) => (v == null ? "–" : Number(v).toLocaleString());

function store(key, value) {
  try {
    if (value === undefined) return localStorage.getItem(key);
    if (value === null) localStorage.removeItem(key);
    else localStorage.setItem(key, value);
  } catch {
    return null;
  }
}

let toastTimer;
function toast(message, isError = false) {
  toastEl.textContent = message;
  toastEl.className = "toast" + (isError ? " error" : "");
  toastEl.hidden = false;
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => (toastEl.hidden = true), isError ? 8000 : 4000);
}

async function api(path, opts = {}) {
  const token = store("telltale_token");
  const headers = { "Content-Type": "application/json" };
  if (token) headers.Authorization = `Bearer ${token}`;
  const res = await fetch(`/api${path}`, { ...opts, headers: { ...headers, ...(opts.headers || {}) } });
  if (res.status === 401) {
    const t = prompt("This Telltale server needs its admin token (TELLTALE_ADMIN_TOKEN):");
    if (t) {
      store("telltale_token", t.trim());
      return api(path, opts);
    }
    throw new Error("Admin token required");
  }
  const type = res.headers.get("content-type") || "";
  const body = type.includes("json") ? await res.json() : await res.text();
  if (!res.ok) throw new Error((body && body.error) || body || res.statusText);
  return body;
}

const post = (path, data) => api(path, { method: "POST", body: JSON.stringify(data ?? {}) });

async function copyText(text) {
  try {
    await navigator.clipboard.writeText(text);
    return true;
  } catch {
    const ta = document.createElement("textarea");
    ta.value = text;
    ta.style.position = "fixed";
    ta.style.opacity = "0";
    document.body.appendChild(ta);
    ta.select();
    const ok = document.execCommand("copy");
    ta.remove();
    return ok;
  }
}

let project = null;
let kpiCache = [];

async function loadProject() {
  project = await api("/project");
  document.getElementById("project-name").textContent = project.name;
}

// ---------------------------------------------------------------- Ideas

let ideaTab = store("telltale_tab") || "new";

function kpiValue(k, v) {
  if (v == null) return "–";
  return k.kind === "per_session" ? Number(v).toFixed(2) : pct(v);
}

function ideaActions(idea) {
  const b = (status, label, cls = "") => `<button class="${cls}" data-status="${status}">${label}</button>`;
  switch (idea.status) {
    case "new":
      return b("selected", "Select") + b("dismissed", "Dismiss", "quiet");
    case "selected":
      return b("in_progress", "Start work") + b("new", "Back to new", "quiet") + b("dismissed", "Dismiss", "quiet");
    case "in_progress":
      return b("released", "Mark released") + b("selected", "Back to selected", "quiet");
    case "released":
      return b("in_progress", "Reopen", "quiet");
    case "dismissed":
      return b("new", "Restore", "quiet");
  }
  return "";
}

function verdictBlock(idea) {
  if (idea.status !== "released") return "";
  if (!idea.kpi_id) return `<div class="verdict no_change">Link a KPI to measure the impact of this release.</div>`;
  const e = idea.evaluation;
  if (!e) return `<div class="verdict insufficient_data">Not evaluated yet. Telltale checks released ideas every hour.</div>`;
  const icon = { improved: "▲", regressed: "▼", no_change: "■", insufficient_data: "…" }[e.verdict];
  return `<div class="verdict ${esc(e.verdict)}">${icon} ${esc(e.summary)}</div>`;
}

function ideaCard(idea) {
  const kpi = kpiCache.find((k) => k.id === idea.kpi_id);
  const released = idea.released_at ? ` · released ${new Date(idea.released_at).toLocaleDateString()}` : "";
  const kpiOptions =
    `<option value="">No KPI</option>` +
    kpiCache.map((k) => `<option value="${k.id}" ${k.id === idea.kpi_id ? "selected" : ""}>${esc(k.name)}</option>`).join("");
  return `
  <article class="card" data-id="${idea.id}">
    <div class="meta">
      <span>#${idea.id}</span>
      ${kpi ? `<span class="chip kpi" title="KPI this idea should move">◎ ${esc(kpi.name)}</span>` : ""}
      <span class="chip ${esc(idea.confidence)}">${esc(idea.confidence)} confidence</span>
      <span>via ${esc(idea.source)}${released}</span>
    </div>
    <h3>${esc(idea.title)}</h3>
    <p class="problem">${esc(idea.problem)}</p>
    ${
      idea.evidence.length
        ? `<details><summary>Evidence (${idea.evidence.length})</summary><ul>${idea.evidence.map((e) => `<li>${esc(e)}</li>`).join("")}</ul></details>`
        : ""
    }
    <div class="proposal"><strong>Proposed change</strong>${esc(idea.proposal)}</div>
    ${idea.expected_impact ? `<p class="hint">Expected impact: ${esc(idea.expected_impact)}</p>` : ""}
    ${idea.dismiss_reason ? `<p class="dismissed-reason">Dismissed: ${esc(idea.dismiss_reason)}</p>` : ""}
    ${verdictBlock(idea)}
    <div class="actions">
      <button class="primary" data-copy>Copy as prompt</button>
      <button class="quiet" data-preview>Preview prompt</button>
      <span class="spacer"></span>
      <select data-kpi aria-label="KPI this idea should move">${kpiOptions}</select>
      ${ideaActions(idea)}
    </div>
  </article>`;
}

async function renderIdeas() {
  const [ideas, kpis] = await Promise.all([api("/ideas"), api("/kpis")]);
  kpiCache = kpis;
  const counts = Object.fromEntries(STATUSES.map(([s]) => [s, ideas.filter((i) => i.status === s).length]));
  const shown = ideas.filter((i) => i.status === ideaTab);
  view.innerHTML = `
    <h1>Ideas</h1>
    <p class="sub">Changes the analyst suggests from real behaviour. Copy one as a prompt for your coding agent, move it along as you work, and once it's released Telltale measures whether it moved its KPI.</p>
    <div class="tabs" role="tablist">
      ${STATUSES.map(
        ([s, label]) =>
          `<button role="tab" data-tab="${s}" class="${s === ideaTab ? "active" : ""}" aria-selected="${s === ideaTab}">${label}<span class="count">${counts[s]}</span></button>`
      ).join("")}
    </div>
    <div class="ideas">
      ${
        shown.length
          ? shown.map(ideaCard).join("")
          : `<div class="empty">${
              ideaTab === "new"
                ? `No new ideas. Click <strong>Run analysis</strong> to look for some.`
                : `Nothing here yet.`
            }</div>`
      }
    </div>`;

  view.querySelectorAll("[data-tab]").forEach((el) =>
    el.addEventListener("click", () => {
      ideaTab = el.dataset.tab;
      store("telltale_tab", ideaTab);
      renderIdeas();
    })
  );

  view.querySelectorAll(".card").forEach((card) => {
    const id = card.dataset.id;
    card.querySelector("[data-copy]").addEventListener("click", async (e) => {
      const text = await api(`/ideas/${id}/prompt`);
      if (await copyText(text)) {
        e.target.textContent = "Copied ✓";
        setTimeout(() => (e.target.textContent = "Copy as prompt"), 1600);
      } else {
        toast("Couldn't copy. Use Preview and copy it from there.", true);
      }
    });
    card.querySelector("[data-preview]").addEventListener("click", async (e) => {
      const existing = card.querySelector(".prompt-preview");
      if (existing) {
        existing.remove();
        e.target.textContent = "Preview prompt";
        return;
      }
      const pre = document.createElement("pre");
      pre.className = "prompt-preview";
      pre.textContent = await api(`/ideas/${id}/prompt`);
      card.appendChild(pre);
      e.target.textContent = "Hide prompt";
    });
    card.querySelector("[data-kpi]").addEventListener("change", async (e) => {
      await post(`/ideas/${id}/kpi`, { kpi_id: e.target.value ? Number(e.target.value) : null });
      toast("KPI updated");
      renderIdeas();
    });
    card.querySelectorAll("[data-status]").forEach((btn) =>
      btn.addEventListener("click", async () => {
        const status = btn.dataset.status;
        let reason = null;
        if (status === "dismissed") {
          reason = prompt("Why dismiss this idea? The analyst uses your reason to avoid similar suggestions.", "");
          if (reason === null) return;
        }
        await post(`/ideas/${id}/status`, { status, reason });
        if (status === "released") {
          await post("/evaluate");
          toast("Released. Telltale will compare the KPI before and after the release.");
        }
        renderIdeas();
      })
    );
  });
}

// ---------------------------------------------------------------- KPIs

function deltaHtml(k) {
  if (k.current == null || k.previous == null) return "";
  const d = k.current - k.previous;
  if (Math.abs(d) < 1e-9) return `<span class="delta">no change</span>`;
  const good = (d > 0) === (k.direction === "increase");
  const text = k.kind === "per_session" ? `${d > 0 ? "+" : ""}${d.toFixed(2)}` : `${d > 0 ? "+" : ""}${(d * 100).toFixed(1)} pts`;
  return `<span class="delta ${good ? "good" : "bad"}">${d > 0 ? "▲" : "▼"} ${text}</span>`;
}

function matcherFields(prefix, legend, m = {}) {
  return `
    <fieldset>
      <legend>${legend}</legend>
      <div class="row">
        <label class="field narrow">Event type
          <select name="${prefix}_type">${EVENT_TYPES.map((t) => `<option ${t === (m.type || "pageview") ? "selected" : ""}>${t}</option>`).join("")}</select>
        </label>
        <label class="field">Path <input name="${prefix}_path" placeholder="/checkout or /products/*" value="${esc(m.path || "")}"></label>
        <label class="field">Selector <input name="${prefix}_selector" placeholder="#pay (optional)" value="${esc(m.selector || "")}"></label>
        <label class="field">Custom event name <input name="${prefix}_name" placeholder="purchase (optional)" value="${esc(m.name || "")}"></label>
      </div>
    </fieldset>`;
}

function readMatcher(fd, prefix) {
  const m = { type: fd.get(`${prefix}_type`) };
  for (const k of ["path", "selector", "name"]) {
    const v = String(fd.get(`${prefix}_${k}`) || "").trim();
    if (v) m[k] = v;
  }
  return m;
}

async function renderKpis() {
  kpiCache = await api("/kpis");
  view.innerHTML = `
    <h1>KPIs</h1>
    <p class="sub">The outcomes you care about. The analyst puts KPIs first when it suggests ideas, and released ideas are judged against them. Values cover the last 14 days, compared with the 14 days before.</p>
    <div class="kpi-grid">
      ${
        kpiCache.length
          ? kpiCache
              .map(
                (k) => `
        <div class="card kpi-card">
          <div class="name">${esc(k.name)}</div>
          <div class="def">${esc(k.description || k.summary)}</div>
          <div class="value">${kpiValue(k, k.current)}</div>
          ${deltaHtml(k)}
          <div class="foot">
            <span>${k.target != null ? `Target ${k.direction === "increase" ? "≥" : "≤"} ${kpiValue(k, k.target)} ${k.target_met ? "✓" : ""}` : "No target"} · n=${num(k.sessions)}</span>
            <button class="quiet" data-delete="${k.id}" aria-label="Delete ${esc(k.name)}">Delete</button>
          </div>
        </div>`
              )
              .join("")
          : `<div class="empty">No KPIs yet. Add the one or two numbers that tell you the product is working.</div>`
      }
    </div>

    <h2>Add a KPI</h2>
    <form class="card kpi-form" id="kpi-form">
      <div class="row">
        <label class="field">Name <input name="name" required placeholder="Checkout conversion"></label>
        <label class="field">Description <input name="description" placeholder="Optional, shown to the analyst"></label>
      </div>
      <div class="row">
        <label class="field narrow">Measure
          <select name="kind">
            <option value="conversion">Conversion (A → B)</option>
            <option value="session_rate">Share of sessions with an event</option>
            <option value="per_session">Events per session</option>
          </select>
        </label>
        <label class="field narrow">Goal
          <select name="direction"><option value="increase">Increase</option><option value="decrease">Decrease</option></select>
        </label>
        <label class="field narrow">Target <input name="target" type="number" step="any" placeholder="e.g. 55"></label>
        <span class="hint" id="target-hint">For conversions and shares, enter the target as a percentage.</span>
      </div>
      <div id="matchers"></div>
      <div><button class="primary" type="submit">Add KPI</button></div>
    </form>`;

  const form = view.querySelector("#kpi-form");
  const matchers = view.querySelector("#matchers");
  const renderMatchers = () => {
    const kind = form.kind.value;
    matchers.innerHTML =
      kind === "conversion"
        ? matcherFields("from", "Sessions that do this…", { type: "pageview", path: "/checkout" }) +
          matcherFields("to", "…and then do this", { type: "pageview", path: "/checkout/success" })
        : matcherFields("match", "Event to count", { type: kind === "per_session" ? "rage_click" : "error" });
    view.querySelector("#target-hint").textContent =
      kind === "per_session" ? "Target is an average number of events per session." : "For conversions and shares, enter the target as a percentage.";
  };
  form.kind.addEventListener("change", renderMatchers);
  renderMatchers();

  form.addEventListener("submit", async (e) => {
    e.preventDefault();
    const fd = new FormData(form);
    const kind = fd.get("kind");
    const targetRaw = String(fd.get("target") || "").trim();
    const body = {
      name: fd.get("name"),
      description: fd.get("description"),
      kind,
      direction: fd.get("direction"),
      target: targetRaw === "" ? null : kind === "per_session" ? Number(targetRaw) : Number(targetRaw) / 100,
    };
    if (kind === "conversion") {
      body.from = readMatcher(fd, "from");
      body.to = readMatcher(fd, "to");
    } else {
      body.match = readMatcher(fd, "match");
    }
    await post("/kpis", body);
    toast("KPI added");
    renderKpis();
  });

  view.querySelectorAll("[data-delete]").forEach((btn) =>
    btn.addEventListener("click", async () => {
      if (!confirm("Delete this KPI? Ideas linked to it will be unlinked.")) return;
      await api(`/kpis/${btn.dataset.delete}`, { method: "DELETE" });
      renderKpis();
    })
  );
}

// ---------------------------------------------------------------- Behaviour

let digestDays = Number(store("telltale_days")) || 14;

function table(headers, rows, empty = "No data in this window.") {
  if (!rows.length) return `<p class="hint">${empty}</p>`;
  return `<div class="table-wrap"><table><thead><tr>${headers
    .map((h) => `<th class="${h.num ? "num" : ""}">${h.label}</th>`)
    .join("")}</tr></thead><tbody>${rows
    .map((r) => `<tr>${r.map((c, i) => `<td class="${headers[i].num ? "num" : ""}">${c}</td>`).join("")}</tr>`)
    .join("")}</tbody></table></div>`;
}

async function renderBehaviour() {
  const d = await api(`/digest?days=${digestDays}`);
  const t = d.totals;
  const clickRows = (list) => list.map((c) => [esc(c.path), `<code>${esc(c.selector)}</code>`, esc(c.text || ""), num(c.count), num(c.sessions)]);
  const clickHeaders = [{ label: "Page" }, { label: "Element" }, { label: "Text" }, { label: "Count", num: 1 }, { label: "Sessions", num: 1 }];
  view.innerHTML = `
    <div class="toolbar">
      <div><h1>Behaviour</h1><p class="sub">This is what the analyst sees: behaviour aggregated over the window, never individual recordings.</p></div>
      <label class="field narrow">Window
        <select id="days">${[1, 7, 14, 30, 90].map((n) => `<option value="${n}" ${n === digestDays ? "selected" : ""}>Last ${n} day${n > 1 ? "s" : ""}</option>`).join("")}</select>
      </label>
    </div>
    <div class="stats">
      ${[
        ["Sessions", t.sessions],
        ["Visitors", t.visitors],
        ["Pageviews", t.pageviews],
        ["Events", t.events],
      ]
        .map(([l, v]) => `<div class="card stat"><div class="value">${num(v)}</div><div class="label">${l}</div></div>`)
        .join("")}
      ${d.devices
        .map((x) => `<div class="card stat"><div class="value">${pct(t.sessions ? x.count / t.sessions : 0)}</div><div class="label">${esc(x.name)}</div></div>`)
        .join("")}
    </div>

    <h2>Pages</h2>
    ${table(
      [{ label: "Path" }, { label: "Views", num: 1 }, { label: "Sessions", num: 1 }, { label: "Exit rate", num: 1 }, { label: "Avg scroll", num: 1 }, { label: "Avg time", num: 1 }],
      d.pages.map((p) => [
        esc(p.path),
        num(p.views),
        num(p.sessions),
        pct(p.exit_rate),
        p.avg_scroll_pct == null ? "–" : `${p.avg_scroll_pct}%`,
        p.avg_seconds == null ? "–" : `${p.avg_seconds}s`,
      ])
    )}

    <h2>Rage clicks</h2>
    ${table(clickHeaders, clickRows(d.rage_clicks), "No rage clicks. Nice.")}

    <h2>Dead clicks</h2>
    ${table(clickHeaders, clickRows(d.dead_clicks), "No dead clicks.")}

    <h2>JavaScript errors</h2>
    ${table(
      [{ label: "Message" }, { label: "Page" }, { label: "Source" }, { label: "Count", num: 1 }, { label: "Sessions", num: 1 }],
      d.errors.map((e) => [esc(e.message), esc(e.path), `<code>${esc(e.source || "")}</code>`, num(e.count), num(e.sessions)]),
      "No errors."
    )}

    <h2>Forms</h2>
    ${table(
      [{ label: "Page" }, { label: "Form" }, { label: "Started", num: 1 }, { label: "Submitted", num: 1 }, { label: "Completion", num: 1 }],
      d.forms.map((f) => [esc(f.path), `<code>${esc(f.selector)}</code>`, num(f.started), num(f.submitted), pct(f.completion_rate)])
    )}

    <h2>Top paths</h2>
    ${table(
      [{ label: "From" }, { label: "To" }, { label: "Count", num: 1 }],
      d.transitions.map((x) => [esc(x.from), esc(x.to), num(x.count)])
    )}

    <h2>Custom events</h2>
    ${table(
      [{ label: "Name" }, { label: "Count", num: 1 }, { label: "Sessions", num: 1 }],
      d.custom_events.map((x) => [esc(x.name), num(x.count), num(x.sessions)]),
      "No custom events. Send them with telltale.track(name, props)."
    )}`;
  view.querySelector("#days").addEventListener("change", (e) => {
    digestDays = Number(e.target.value);
    store("telltale_days", String(digestDays));
    renderBehaviour();
  });
}

// ---------------------------------------------------------------- Setup

async function renderSetup() {
  const runs = await api("/runs");
  const snippet = `<script src="${location.origin}/t.js" data-key="${project.write_key}" defer></script>`;
  const custom = `telltale.track("purchase", { plan: "pro" });\ntelltale.identify(currentUser.id); // optional`;
  view.innerHTML = `
    <h1>Setup</h1>
    <p class="sub">Install the collector, then let the analyst do its work.</p>

    <h2>Product name</h2>
    <form class="row" id="rename">
      <label class="field">Used in prompts so your coding agent knows what it's working on
        <input name="name" value="${esc(project.name)}" required>
      </label>
      <button type="submit">Save</button>
    </form>

    <h2>1. Add the collector to your site</h2>
    <p class="hint">Paste this into every page, before <code>&lt;/head&gt;</code>. It records pageviews, clicks, rage and dead clicks, errors, scroll depth and form starts/submits. It never records what users type. Add <code>data-telltale-mask</code> to any element whose text is private.</p>
    <div class="copy-row"><pre class="snippet">${esc(snippet)}</pre><button data-copy="${esc(snippet)}">Copy</button></div>

    <h2>2. Track the moments that matter (optional)</h2>
    <div class="copy-row"><pre class="snippet">${esc(custom)}</pre><button data-copy="${esc(custom)}">Copy</button></div>

    <h2>3. Analyst</h2>
    <p class="hint">${
      project.claude
        ? `The analyst uses Claude (<code>${esc(project.claude.model)}</code>, effort <code>${esc(project.claude.effort)}</code>). If Claude is unavailable, it falls back to the built-in heuristics.`
        : `No <code>ANTHROPIC_API_KEY</code> is set, so the analyst uses built-in heuristics: rage clicks, dead clicks, errors and form drop-off. Set the key and restart for richer, KPI-aware ideas.`
    }</p>
    <p class="hint">From the command line: <code>telltale analyze</code>, <code>telltale evaluate</code>, <code>telltale prompt &lt;id&gt; | claude</code>.</p>
    ${table(
      [{ label: "Run" }, { label: "When" }, { label: "Engine" }, { label: "Ideas", num: 1 }, { label: "Notes" }],
      runs.map((r) => [
        `#${r.id}`,
        new Date(r.started_at).toLocaleString(),
        esc(r.engine + (r.model ? ` · ${r.model}` : "")),
        num(r.ideas_created),
        esc(r.error || ""),
      ]),
      "No analysis runs yet."
    )}`;
  view.querySelectorAll("[data-copy]").forEach((b) =>
    b.addEventListener("click", async () => {
      if (await copyText(b.dataset.copy)) toast("Copied");
    })
  );
  view.querySelector("#rename").addEventListener("submit", async (e) => {
    e.preventDefault();
    project = await api("/project", { method: "PUT", body: JSON.stringify({ name: e.target.name.value }) });
    document.getElementById("project-name").textContent = project.name;
    toast("Saved");
  });
}

// ---------------------------------------------------------------- shell

const routes = { ideas: renderIdeas, kpis: renderKpis, behaviour: renderBehaviour, setup: renderSetup };

async function route() {
  const name = location.hash.replace(/^#\//, "") || "ideas";
  const render = routes[name] || renderIdeas;
  document.querySelectorAll("#nav a").forEach((a) => a.classList.toggle("active", a.dataset.view === name));
  try {
    if (!project) await loadProject();
    await render();
  } catch (e) {
    view.innerHTML = `<div class="empty">Couldn't load this page: ${esc(e.message)}</div>`;
  }
}

document.getElementById("run-analysis").addEventListener("click", async (e) => {
  const btn = e.currentTarget;
  btn.disabled = true;
  btn.textContent = "Analysing…";
  try {
    const r = await post("/analyze");
    const n = r.created.length;
    toast(
      `${n} new idea${n === 1 ? "" : "s"} via ${r.engine}${r.duplicates ? `, ${r.duplicates} already known` : ""}.` +
        (r.warning ? ` ${r.warning}` : ""),
      Boolean(r.warning && !n)
    );
    ideaTab = "new";
    if (location.hash === "#/ideas" || !location.hash) route();
    else location.hash = "#/ideas";
  } catch (err) {
    toast(`Analysis failed: ${err.message}`, true);
  } finally {
    btn.disabled = false;
    btn.textContent = "Run analysis";
  }
});

window.addEventListener("hashchange", route);
window.addEventListener("unhandledrejection", (e) => toast(e.reason?.message || String(e.reason), true));
route();
