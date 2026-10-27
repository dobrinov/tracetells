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

// Every request is JSON: the server rejects cookie-authenticated writes that
// aren't, which is what stops other sites from forging requests (CSRF).
async function api(path, opts = {}) {
  const res = await fetch(`/api${path}`, { ...opts, headers: { "Content-Type": "application/json", ...(opts.headers || {}) } });
  const type = res.headers.get("content-type") || "";
  const body = type.includes("json") ? await res.json() : await res.text();
  if (res.status === 401 && !path.startsWith("/auth/")) {
    // Session ended or a second factor is due: back to the sign-in flow.
    auth = null;
    project = null;
    boot();
  }
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

function host(origin) {
  return String(origin || "").replace(/^https?:\/\//, "");
}

// Setup is onboarding: once the collector is clearly delivering data it leaves the
// navigation, and the header pill is both the confirmation and the way back to it.
function showCollection() {
  const c = project.collection;
  const pill = document.getElementById("collect-status");
  const setupLink = document.querySelector('#nav a[data-view="setup"]');
  setupLink.hidden = c.active;
  pill.hidden = false;
  if (c.active) {
    const sites = c.sources.filter((s) => Date.now() - s.last_seen < 86400000).map((s) => host(s.origin));
    pill.className = "collect-status ok";
    pill.textContent = `Collecting${sites.length ? ` from ${sites[0]}${sites.length > 1 ? ` +${sites.length - 1}` : ""}` : ""}`;
    pill.title = `Last event ${ago(c.last_event_at)}. Open the install snippet to add another site.`;
  } else {
    pill.className = "collect-status waiting";
    pill.textContent = c.last_event_at ? `No data since ${ago(c.last_event_at)}` : "Not collecting yet";
    pill.title = "Open the install snippet";
  }
}

async function loadProject() {
  project = await api("/project");
  document.getElementById("project-name").textContent = project.name;
  showCollection();
}

// ---------------------------------------------------------------- Ideas

let ideaTab = store("diagnostic_tab") || "new";

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
  if (!e) return `<div class="verdict insufficient_data">Not evaluated yet. Diagnostic checks released ideas every hour.</div>`;
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
      ${can("member") ? `<select data-kpi aria-label="KPI this idea should move">${kpiOptions}</select>${ideaActions(idea)}` : ""}
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
    <p class="sub">Changes the analyst suggests from real behaviour. Copy one as a prompt for your coding agent, move it along as you work, and once it's released Diagnostic measures whether it moved its KPI.</p>
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
      store("diagnostic_tab", ideaTab);
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
    card.querySelector("[data-kpi]")?.addEventListener("change", async (e) => {
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
          toast("Released. Diagnostic will compare the KPI before and after the release.");
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

function ago(ts) {
  if (!ts) return "never";
  const m = Math.round((Date.now() - ts) / 60000);
  if (m < 1) return "just now";
  if (m < 60) return `${m} min ago`;
  const h = Math.round(m / 60);
  return h < 48 ? `${h} h ago` : `${Math.round(h / 24)} days ago`;
}

const CONFIG_EXAMPLE = `{
  "product": "Your product",
  "events": {
    "purchase": "Payment succeeded"
  },
  "kpis": {
    "checkout_conversion": {
      "name": "Checkout conversion",
      "conversion": { "from": { "pageview": "/checkout" }, "to": { "event": "purchase" } },
      "goal": "increase",
      "target": "55%"
    }
  }
}`;

async function renderKpis() {
  const [kpis, cfg] = await Promise.all([api("/kpis"), api("/config")]);
  kpiCache = kpis;
  const push = `diagnostic push diagnostic.json --server ${location.origin} --token $DIAGNOSTIC_TOKEN   # create one under Account`;
  const source = cfg.synced_at
    ? `<p class="hint">Defined in <code>${esc(cfg.source || "diagnostic.json")}</code>, synced ${esc(ago(cfg.synced_at))}. To change a KPI, edit <code>diagnostic.json</code> in your repository and push it.</p>`
    : "";
  const cards = kpis.length
    ? `<div class="kpi-grid">${kpis
        .map(
          (k) => `
        <div class="card kpi-card">
          <div class="name">${esc(k.name)}</div>
          <div class="def">${esc(k.description || k.summary)}</div>
          <div class="value">${kpiValue(k, k.current)}</div>
          ${deltaHtml(k)}
          <div class="foot">
            <span>${k.target != null ? `Target ${k.direction === "increase" ? "≥" : "≤"} ${kpiValue(k, k.target)} ${k.target_met ? "✓" : ""}` : "No target"} · n=${num(k.sessions)}</span>
          </div>
          <div class="kpi-key" title="Key in diagnostic.json"><code>${esc(k.key || "")}</code></div>
        </div>`
        )
        .join("")}</div>`
    : `<div class="card">
        <p><strong>No KPIs yet.</strong> KPIs live in your application's code, next to the events that feed them. Add a <code>diagnostic.json</code> to your repository:</p>
        <div class="copy-row"><pre class="snippet">${esc(CONFIG_EXAMPLE)}</pre><button data-copy="${esc(CONFIG_EXAMPLE)}">Copy</button></div>
        <p class="hint">Check it with <code>diagnostic check</code>, then publish it, for example from CI:</p>
        <div class="copy-row"><pre class="snippet">${esc(push)}</pre><button data-copy="${esc(push)}">Copy</button></div>
      </div>`;

  const declared = cfg.events.map((e) => [
    `<code>${esc(e.name)}</code>`,
    esc(e.description),
    num(e.count),
    e.count ? `<span class="delta good">● receiving</span> <span class="hint">${esc(ago(e.last_seen))}</span>` : `<span class="delta bad">● not seen in 14 days</span>`,
  ]);
  const undeclared = cfg.undeclared_events.map((e) => [
    `<code>${esc(e.name)}</code>`,
    `<span class="hint">Sent by your app but not declared in diagnostic.json</span>`,
    num(e.count),
    `<span class="delta">● undeclared</span>`,
  ]);

  view.innerHTML = `
    <h1>KPIs</h1>
    <p class="sub">The outcomes your team set in code. The analyst puts KPIs first when it suggests ideas, and released ideas are judged against them. Values cover the last 14 days, compared with the 14 days before.</p>
    ${source}
    ${cards}
    <h2>Tracking health</h2>
    <p class="hint">Custom events declared in <code>diagnostic.json</code>, and whether your application is sending them. Send them with <code>diagnostic.track("name")</code>, or without JavaScript using <code>data-diagnostic-event="name"</code> on a button, link or form.</p>
    ${table(
      [{ label: "Event" }, { label: "Meaning" }, { label: "Last 14 days", num: 1 }, { label: "Status" }],
      [...declared, ...undeclared],
      "No custom events declared or received yet."
    )}`;
  view.querySelectorAll("[data-copy]").forEach((b) =>
    b.addEventListener("click", async () => {
      if (await copyText(b.dataset.copy)) toast("Copied");
    })
  );
}

// ---------------------------------------------------------------- Behaviour

let digestDays = Number(store("diagnostic_days")) || 14;

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

    <h2>Errors</h2>
    ${table(
      [{ label: "Kind" }, { label: "Message" }, { label: "Page" }, { label: "Source" }, { label: "Count", num: 1 }, { label: "Sessions", num: 1 }],
      d.errors.map((e) => [`<span class="chip">${esc(e.kind)}</span>`, esc(e.message), esc(e.path), e.source ? `<code>${esc(e.source)}</code>` : "", num(e.count), num(e.sessions)]),
      "No errors: no exceptions, console errors, failed requests or missing files."
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
      [{ label: "Name" }, { label: "Meaning" }, { label: "Count", num: 1 }, { label: "Sessions", num: 1 }],
      d.custom_events.map((x) => [esc(x.name), x.declared ? esc(x.description) : `<span class="hint">not declared in diagnostic.json</span>`, num(x.count), num(x.sessions)]),
      "No custom events. Send them with diagnostic.track(name, props)."
    )}`;
  view.querySelector("#days").addEventListener("change", (e) => {
    digestDays = Number(e.target.value);
    store("diagnostic_days", String(digestDays));
    renderBehaviour();
  });
}

// ---------------------------------------------------------------- Setup

async function renderSetup() {
  const runs = await api("/runs");
  const src = `${location.origin}/d.js`;
  const key = project.write_key;
  const stub = `<script>window.diagnostic=window.diagnostic||{q:[]};["track","identify","page","init"].forEach(function(m){diagnostic[m]=diagnostic[m]||function(){diagnostic.q.push([m,arguments])}})</script>`;
  const installs = {
    site: {
      label: "Website",
      hint: "Any site where links reload the page: server-rendered apps, CMSs, static sites. Paste into every page before </head>. Sessions continue across page loads, and back/forward restores count as pageviews.",
      code: `<script src="${src}" data-key="${key}" defer></script>`,
    },
    spa: {
      label: "Single-page app",
      hint: "React, Vue, Svelte, Angular and others. Route changes through the history API, the Navigation API or hash routes (#/settings) are picked up automatically. Use data-routes to group dynamic URLs. Numeric and UUID segments become :id on their own. The stub lets your code call diagnostic.track() before the script has loaded.",
      code: `${stub}\n<script src="${src}" data-key="${key}" data-routes="/products/:slug, /docs/*" defer></script>`,
    },
    next: {
      label: "Next.js",
      hint: "In app/layout.tsx (App Router) or pages/_app.tsx. App Router navigations are tracked automatically.",
      code: `import Script from "next/script";\n\n<Script src="${src}" data-key="${key}" data-routes="/products/:slug" strategy="afterInteractive" />`,
    },
    bundle: {
      label: "Tag manager / module",
      hint: "When the script tag can't carry data-* attributes (Google Tag Manager, type=\"module\", dynamic loading), configure it in code instead. Calls made before the script loads are queued.",
      code: `${stub}\n<script>\n  diagnostic.init({ key: "${key}", routes: ["/products/:slug"] });\n</script>\n<script src="${src}" async></script>`,
    },
  };
  const mcpCommand = `claude mcp add --transport http diagnostic ${location.origin}/mcp \\\n    --header "Authorization: Bearer YOUR_API_TOKEN"`;
  const mcpJson = JSON.stringify({ mcpServers: { diagnostic: { url: `${location.origin}/mcp`, headers: { Authorization: "Bearer YOUR_API_TOKEN" } } } }, null, 2);
  let installTab = store("diagnostic_install") || "site";
  if (!installs[installTab]) installTab = "site";
  const custom = `<button data-diagnostic-event="add_to_cart">Add to cart</button>   <!-- no JavaScript needed -->

diagnostic.track("purchase", { plan: "pro" });\ndiagnostic.identify(currentUser.id);      // optional: tie sessions to your user id\ndiagnostic.page("/checkout/:step");       // optional: name the current route yourself\n                                        // (set pageviews: "manual" to turn off automatic pageviews)`;
  view.innerHTML = `
    <h1>Setup</h1>
    <p class="sub">Install the collector, then let the analyst do its work.</p>
    <div class="card setup-status" id="setup-status">${setupStatus()}</div>


    <h2>1. Add the collector to your site</h2>
    <p class="hint">The collector records pageviews, clicks, rage and dead clicks, errors, scroll depth and form starts/submits. It never records what users type. Add <code>data-diagnostic-mask</code> to any element whose text is private. It does its work when the browser is idle, so it doesn't slow your pages down.</p>
    <div class="tabs" role="tablist">
      ${Object.entries(installs)
        .map(([id, i]) => `<button role="tab" data-install="${id}" class="${id === installTab ? "active" : ""}" aria-selected="${id === installTab}">${i.label}</button>`)
        .join("")}
    </div>
    <p class="hint">${esc(installs[installTab].hint)}</p>
    <div class="copy-row"><pre class="snippet">${esc(installs[installTab].code)}</pre><button data-copy="${esc(installs[installTab].code)}">Copy</button></div>

    <h2>2. Define KPIs and events in code</h2>
    <p class="hint">Add a <code>diagnostic.json</code> to your repository with the product name, the custom events your app sends and the KPIs you care about. Validate it with <code>diagnostic check</code> and publish it with <code>diagnostic push --server ${esc(location.origin)}</code>, ideally from CI on every merge. The <a href="#/kpis">KPIs</a> page shows the results and whether each event is arriving.</p>

    <h2>3. Track the moments that matter</h2>
    <div class="copy-row"><pre class="snippet">${esc(custom)}</pre><button data-copy="${esc(custom)}">Copy</button></div>

    <h2>4. Connect your coding agent (MCP)</h2>
    <p class="hint">Let Claude Code, Cursor or any MCP client read your KPIs and ideas, fetch an idea as a task, and mark it released once it ships. Agents only see aggregated patterns, like this dashboard. Create a personal API token under <a href="#/account">Account</a> (read-only is enough if the agent only needs to look), then:</p>
    <div class="copy-row"><pre class="snippet">${esc(mcpCommand)}</pre><button data-copy="${esc(mcpCommand)}">Copy</button></div>
    <p class="hint">Other MCP clients:</p>
    <div class="copy-row"><pre class="snippet">${esc(mcpJson)}</pre><button data-copy="${esc(mcpJson)}">Copy</button></div>

    <h2>5. Analyst</h2>
    <p class="hint">${
      project.claude
        ? `The analyst uses Claude (<code>${esc(project.claude.model)}</code>, effort <code>${esc(project.claude.effort)}</code>). If Claude is unavailable, it falls back to the built-in heuristics.`
        : `No <code>ANTHROPIC_API_KEY</code> is set, so the analyst uses built-in heuristics: rage clicks, dead clicks, errors and form drop-off. Set the key and restart for richer, KPI-aware ideas.`
    }</p>
    <p class="hint">From the command line: <code>diagnostic analyze</code>, <code>diagnostic evaluate</code>, <code>diagnostic prompt &lt;id&gt; | claude</code>.</p>
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
  view.querySelectorAll("[data-install]").forEach((b) =>
    b.addEventListener("click", () => {
      store("diagnostic_install", b.dataset.install);
      renderSetup();
    })
  );
  if (!project.collection.active) pollSetup();
}

function setupStatus() {
  const c = project.collection;
  if (c.active) {
    const sites = c.sources.map((s) => `<code>${esc(host(s.origin))}</code> (last event ${esc(ago(s.last_seen))})`).join(", ");
    return `<strong class="delta good">● Receiving data</strong>${sites ? ` from ${sites}` : ""}. You're set up: <a href="#/behaviour">see what's coming in</a> or <a href="#/ideas">go to ideas</a>. This page leaves the menu now; the status in the header brings you back here.`;
  }
  return c.last_event_at
    ? `<strong class="delta bad">● No data since ${esc(ago(c.last_event_at))}</strong>. Check that the snippet below is still on your pages.`
    : `<strong class="pulse">● Waiting for the first event…</strong> Add the snippet below to your site and open any page. This updates by itself.`;
}

// While Setup is open and nothing has arrived yet, check every few seconds so the
// confirmation appears the moment the first event lands.
let setupPoll = null;
function pollSetup() {
  clearInterval(setupPoll);
  setupPoll = setInterval(async () => {
    if (!location.hash.startsWith("#/setup")) return clearInterval(setupPoll);
    const was = project.collection.active;
    try {
      project = await api("/project");
    } catch {
      return;
    }
    showCollection();
    const box = document.getElementById("setup-status");
    if (box) box.innerHTML = setupStatus();
    if (!was && project.collection.active) toast("Data is coming in. Diagnostic is collecting.");
    if (project.collection.active) clearInterval(setupPoll);
  }, 3000);
}

// Keep the header status current on long-open dashboards.
setInterval(async () => {
  try {
    project = await api("/project");
    showCollection();
  } catch {}
}, 60000);

// ---------------------------------------------------------------- shell

// ---------------------------------------------------------------- accounts

let auth = null;
const RANK = { viewer: 0, member: 1, admin: 2, owner: 3 };
const can = (role) => Boolean(auth && auth.user && RANK[auth.user.role] >= RANK[role]);

function authScreen(html) {
  document.body.classList.add("auth");
  view.innerHTML = `<div class="auth-card card">${html}</div>`;
  const first = view.querySelector("input");
  if (first) first.focus();
}

function formError(form, message) {
  let el = form.querySelector(".form-error");
  if (!el) {
    el = document.createElement("p");
    el.className = "form-error";
    el.setAttribute("role", "alert");
    form.prepend(el);
  }
  el.textContent = message;
}

// Wires a form to an async action; shows errors inline and disables the button while busy.
function onSubmit(form, action) {
  form.addEventListener("submit", async (e) => {
    e.preventDefault();
    const btn = form.querySelector("button[type=submit]");
    if (btn) btn.disabled = true;
    try {
      await action(new FormData(form));
    } catch (err) {
      formError(form, err.message);
    } finally {
      if (btn) btn.disabled = false;
    }
  });
}

const BRAND = `<div class="auth-brand"><svg viewBox="0 0 32 32" aria-hidden="true"><rect width="32" height="32" rx="7" class="brand-tile"/><path d="M5 17h5l3-7 4 13 3-9 2 3h5" class="brand-mark"/></svg> Diagnostic</div>`;
const PATTERNS_NOT_PEOPLE = `<p class="hint privacy-note">Diagnostic looks at patterns, not people: no session recordings, nothing users type, and personal data is removed before it's stored.</p>`;

function renderLogin() {
  authScreen(`${BRAND}<h1>Sign in</h1>
    <form id="login" class="stack">
      <label class="field">Email <input name="email" type="email" autocomplete="username" required></label>
      <label class="field">Password <input name="password" type="password" autocomplete="current-password" required></label>
      <button class="primary" type="submit">Sign in</button>
    </form>
    <p class="hint">Forgot your password? Ask an admin to invite you again, or run <code>diagnostic user reset-password</code> on the server.</p>`);
  onSubmit(view.querySelector("#login"), async (fd) => {
    auth = await api("/auth/login", { method: "POST", body: JSON.stringify({ email: fd.get("email"), password: fd.get("password") }) });
    boot();
  });
}

function renderVerify() {
  authScreen(`${BRAND}<h1>Two-step verification</h1>
    <p>Enter the 6-digit code from your authenticator app.</p>
    <form id="verify" class="stack">
      <label class="field">Code <input name="code" autocomplete="one-time-code" inputmode="numeric" maxlength="12" required></label>
      <button class="primary" type="submit">Verify</button>
    </form>
    <p class="hint">Lost your phone? Enter one of your recovery codes instead. <a href="#" id="start-over">Sign in as someone else</a></p>`);
  onSubmit(view.querySelector("#verify"), async (fd) => {
    const r = await api("/auth/mfa", { method: "POST", body: JSON.stringify({ code: fd.get("code") }) });
    auth = r;
    if (r.recovery_codes_left != null)
      toast(`Recovery code used. ${r.recovery_codes_left} left: set up your authenticator again under Account.`, r.recovery_codes_left < 3);
    boot();
  });
  view.querySelector("#start-over").addEventListener("click", (e) => {
    e.preventDefault();
    signOut();
  });
}

// Set up an authenticator app inside `box`, then call `done`.
async function enrollmentFlow(box, done) {
  const e = await api("/account/mfa/start", { method: "POST" });
  box.innerHTML = `
    <ol class="steps">
      <li>Scan this with your authenticator app (1Password, Google Authenticator, Authy, ...).
        <div class="qr">${e.qr_svg}</div>
        <p class="hint">Can't scan? Enter this key instead: <code class="secret">${esc(e.secret.replace(/(.{4})/g, "$1 ").trim())}</code>
          · or <a href="${esc(e.uri)}">open it in an app on this device</a></p></li>
      <li>Enter the 6-digit code the app shows:
        <form id="confirm-mfa" class="row">
          <label class="field narrow">Code <input name="code" autocomplete="one-time-code" inputmode="numeric" maxlength="7" required></label>
          <button class="primary" type="submit">Turn on</button>
        </form></li>
    </ol>`;
  box.querySelector("input").focus();
  onSubmit(box.querySelector("#confirm-mfa"), async (fd) => {
    const r = await api("/account/mfa/confirm", { method: "POST", body: JSON.stringify({ code: fd.get("code") }) });
    showRecoveryCodes(box, r.recovery_codes, done);
  });
}

function showRecoveryCodes(box, codes, done) {
  const text = `Diagnostic recovery codes for ${auth.user.email}\nEach works once, instead of an authenticator code.\n\n${codes.join("\n")}\n`;
  box.innerHTML = `
    <p><strong>Save your recovery codes.</strong> Each one works once if you lose your phone. They won't be shown again.</p>
    <div class="codes">${codes.map((c) => `<code>${esc(c)}</code>`).join("")}</div>
    <div class="actions">
      <button data-copy-codes>Copy</button>
      <button data-download-codes>Download .txt</button>
      <span class="spacer"></span>
      <button class="primary" data-done>I've saved them</button>
    </div>`;
  box.querySelector("[data-copy-codes]").addEventListener("click", async () => (await copyText(text)) && toast("Copied"));
  box.querySelector("[data-download-codes]").addEventListener("click", () => {
    const a = document.createElement("a");
    a.href = URL.createObjectURL(new Blob([text], { type: "text/plain" }));
    a.download = "diagnostic-recovery-codes.txt";
    a.click();
    URL.revokeObjectURL(a.href);
  });
  box.querySelector("[data-done]").addEventListener("click", done);
}

function renderEnroll() {
  authScreen(`${BRAND}<h1>Set up two-step verification</h1>
    <p>Your workspace requires it. You'll need an authenticator app on your phone.</p>
    <div id="enroll"></div>
    <p class="hint"><a href="#" id="start-over">Sign out</a></p>`);
  view.querySelector("#start-over").addEventListener("click", (e) => {
    e.preventDefault();
    signOut();
  });
  enrollmentFlow(view.querySelector("#enroll"), boot).catch((err) => toast(err.message, true));
}

function renderWelcome(code) {
  if (!code) {
    authScreen(`${BRAND}<h1>Welcome to Diagnostic</h1>
      <p>This workspace has no owner yet. To create the owner account, open the setup link that <code>diagnostic serve</code> printed when it started.</p>
      <p class="hint">Or create it on the server: <code>diagnostic user create --email you@example.com</code></p>`);
    return;
  }
  authScreen(`${BRAND}<h1>Create your account</h1>
    <p>You'll be the owner of this workspace. You can invite your team later, or keep it just for you.</p>
    <form id="welcome" class="stack">
      <label class="field">Name <input name="name" autocomplete="name"></label>
      <label class="field">Email <input name="email" type="email" autocomplete="username" required></label>
      <label class="field">Password <input name="password" type="password" autocomplete="new-password" minlength="10" required>
        <span class="hint">At least 10 characters.</span></label>
      <button class="primary" type="submit">Create account</button>
    </form>${PATTERNS_NOT_PEOPLE}`);
  onSubmit(view.querySelector("#welcome"), async (fd) => {
    auth = await api("/auth/setup", {
      method: "POST",
      body: JSON.stringify({ code, name: fd.get("name"), email: fd.get("email"), password: fd.get("password") }),
    });
    history.replaceState(null, "", "#/setup");
    boot();
  });
}

async function renderInvite(token) {
  let invite;
  try {
    invite = await api(`/auth/invite/${encodeURIComponent(token)}`);
  } catch (e) {
    authScreen(`${BRAND}<h1>Invite not valid</h1><p>${esc(e.message)}</p><p><a href="#/">Sign in</a></p>`);
    return;
  }
  authScreen(`${BRAND}<h1>Join ${esc(invite.workspace)}</h1>
    <p>You've been invited as <strong>${esc(invite.role)}</strong> with <strong>${esc(invite.email)}</strong>.${invite.require_mfa ? " This workspace requires two-step verification, which you'll set up next." : ""}</p>
    <form id="accept" class="stack">
      <input type="email" name="username" value="${esc(invite.email)}" autocomplete="username" hidden>
      <label class="field">Name <input name="name" autocomplete="name"></label>
      <label class="field">Choose a password <input name="password" type="password" autocomplete="new-password" minlength="10" required>
        <span class="hint">At least 10 characters.</span></label>
      <button class="primary" type="submit">Join</button>
    </form>${PATTERNS_NOT_PEOPLE}`);
  onSubmit(view.querySelector("#accept"), async (fd) => {
    auth = await api(`/auth/invite/${encodeURIComponent(token)}`, {
      method: "POST",
      body: JSON.stringify({ name: fd.get("name"), password: fd.get("password") }),
    });
    history.replaceState(null, "", "#/ideas");
    boot();
  });
}

async function signOut() {
  try {
    await api("/auth/logout", { method: "POST" });
  } catch {}
  auth = null;
  project = null;
  history.replaceState(null, "", "#/");
  boot();
}

function applyRole() {
  document.querySelector('#nav a[data-view="team"]').hidden = !can("admin");
  document.getElementById("run-analysis").hidden = !can("member");
  const me = document.getElementById("me");
  me.hidden = false;
  me.textContent = auth.user.name || auth.user.email;
  me.title = `${auth.user.email} (${auth.user.role})`;
}

async function renderAccount() {
  const [a, tokens] = await Promise.all([api("/account"), api("/account/tokens")]);
  const u = a.user;
  view.innerHTML = `
    <h1>Account</h1>
    <p class="sub">${esc(u.email)} · ${esc(u.role)}</p>
    <h2>Two-step verification</h2>
    <div class="card" id="mfa-box">${
      u.totp_enabled
        ? `<p><strong class="delta good">● On.</strong> Signing in needs a code from your authenticator app. ${a.recovery_codes_left} recovery code${a.recovery_codes_left === 1 ? "" : "s"} left.</p>
           <div class="actions"><button data-new-codes>New recovery codes</button>${
             a.require_mfa ? `<span class="hint">Your workspace requires two-step verification.</span>` : `<button class="quiet" data-mfa-off>Turn off</button>`
           }</div>`
        : `<p><strong class="delta bad">● Off.</strong> Add a second step to signing in: a code from an authenticator app on your phone.</p>
           <button class="primary" data-mfa-on>Set up</button>`
    }</div>
    <h2>Password</h2>
    <form class="card stack narrow-form" id="password">
      <input type="email" name="username" value="${esc(u.email)}" autocomplete="username" hidden>
      <label class="field">Current password <input name="current" type="password" autocomplete="current-password" required></label>
      <label class="field">New password <input name="new" type="password" autocomplete="new-password" minlength="10" required></label>
      <div><button type="submit">Change password</button> <span class="hint">Signs you out on other devices.</span></div>
    </form>
    <h2>Your API tokens</h2>
    <p class="hint">For your coding agent over MCP and for CI (<code>diagnostic push --token</code>). A token can do what you can (you're ${esc(u.role)}), or only read if you tick read-only. Use one token per tool, so you can revoke them separately.</p>
    ${tokens.length ? table(
      [{ label: "Token" }, { label: "Access" }, { label: "Created" }, { label: "Last used" }, { label: "" }],
      tokens.map((k) => [`${esc(k.name)} <code>${esc(k.prefix)}…</code>`, k.read_only ? "read-only" : esc(u.role), new Date(k.created_at).toLocaleDateString(), esc(ago(k.last_used_at)), `<button class="quiet" data-revoke-mine="${k.id}">Revoke</button>`])
    ) : ""}
    <form class="card row" id="new-token">
      <label class="field">Name <input name="name" placeholder="Claude Code" required></label>
      <label class="check"><input type="checkbox" name="read_only"> <span>Read-only</span></label>
      <button type="submit">Create token</button>
    </form>
    <div id="token-result"></div>

    <h2>Session</h2>
    <button data-sign-out>Sign out</button>`;
  const box = view.querySelector("#mfa-box");
  view.querySelector("[data-mfa-on]")?.addEventListener("click", () =>
    enrollmentFlow(box, async () => {
      auth = await api("/auth/state");
      renderAccount();
    }).catch((e) => toast(e.message, true))
  );
  view.querySelector("[data-mfa-off]")?.addEventListener("click", async () => {
    const password = prompt("Enter your password to turn off two-step verification:");
    if (!password) return;
    await api("/account/mfa/disable", { method: "POST", body: JSON.stringify({ password }) });
    toast("Two-step verification is off");
    renderAccount();
  });
  view.querySelector("[data-new-codes]")?.addEventListener("click", async () => {
    const password = prompt("Enter your password to replace your recovery codes. The old ones stop working.");
    if (!password) return;
    const r = await api("/account/recovery-codes", { method: "POST", body: JSON.stringify({ password }) });
    showRecoveryCodes(box, r.recovery_codes, renderAccount);
  });
  onSubmit(view.querySelector("#password"), async (fd) => {
    await api("/account/password", { method: "POST", body: JSON.stringify({ current: fd.get("current"), new: fd.get("new") }) });
    view.querySelector("#password").reset();
    toast("Password changed");
  });
  view.querySelector("[data-sign-out]").addEventListener("click", signOut);
  onSubmit(view.querySelector("#new-token"), async (fd) => {
    const r = await api("/account/tokens", { method: "POST", body: JSON.stringify({ name: fd.get("name"), read_only: fd.get("read_only") === "on" }) });
    await renderAccount();
    view.querySelector("#token-result").innerHTML = `<div class="card"><p><strong>Copy this token now.</strong> It won't be shown again.</p>
      <div class="copy-row"><pre class="snippet">${esc(r.secret)}</pre><button data-copy="${esc(r.secret)}">Copy</button></div></div>`;
    view.querySelector("#token-result [data-copy]").addEventListener("click", async (e) => (await copyText(e.target.dataset.copy)) && toast("Copied"));
  });
  view.querySelectorAll("[data-revoke-mine]").forEach((b) =>
    b.addEventListener("click", async () => {
      if (!confirm("Revoke this token? Anything using it stops working.")) return;
      await api(`/account/tokens/${b.dataset.revokeMine}`, { method: "DELETE" });
      renderAccount();
    })
  );
}

async function renderTeam() {
  if (!can("admin")) {
    view.innerHTML = `<div class="empty">Only admins can manage the team.</div>`;
    return;
  }
  const [t, tokens, log] = await Promise.all([api("/team"), api("/team/tokens"), api("/audit")]);
  const roleSelect = (u) =>
    u.role === "owner" || u.id === auth.user.id
      ? esc(u.role)
      : `<select data-role="${u.id}" aria-label="Role for ${esc(u.email)}">${["admin", "member", "viewer"]
          .map((r) => `<option ${r === u.role ? "selected" : ""}>${r}</option>`)
          .join("")}</select>`;
  view.innerHTML = `
    <h1>Team</h1>
    <p class="sub">Who can use this workspace, and how they sign in.</p>

    <h2>Security</h2>
    <div class="card">
      <label class="check"><input type="checkbox" id="require-mfa" ${t.require_mfa ? "checked" : ""}>
        <span><strong>Require two-step verification for everyone</strong><br>
        <span class="hint">Members without it are asked to set up an authenticator app the next time they sign in.${auth.user.totp_enabled ? "" : " Turn it on for your own account first, under Account."}</span></span></label>
    </div>

    <h2>Members</h2>
    ${table(
      [{ label: "Person" }, { label: "Role" }, { label: "Two-step" }, { label: "Last sign-in" }, { label: "" }],
      t.users.map((u) => [
        `${esc(u.name || u.email)}<br><span class="hint">${esc(u.email)}</span>`,
        roleSelect(u),
        u.totp_enabled ? `<span class="delta good">● on</span>` : `<span class="delta ${t.require_mfa ? "bad" : ""}">● off</span>`,
        esc(ago(u.last_login_at)),
        u.role === "owner" || u.id === auth.user.id
          ? ""
          : `${u.totp_enabled ? `<button class="quiet" data-reset-mfa="${u.id}">Reset two-step</button>` : ""}<button class="quiet" data-remove="${u.id}" data-email="${esc(u.email)}">Remove</button>`,
      ])
    )}

    <h2>Invite someone</h2>
    <form class="card row" id="invite">
      <label class="field">Email <input name="email" type="email" required></label>
      <label class="field narrow">Role <select name="role"><option>member</option><option>viewer</option><option>admin</option></select></label>
      <button class="primary" type="submit">Create invite link</button>
    </form>
    <p class="hint">Members work with ideas and run analysis. Viewers can look but not change anything. Admins also manage the team, security and API tokens. Diagnostic doesn't send email: share the link yourself. It works once and expires in 7 days.</p>
    <div id="invite-result"></div>
    ${
      t.invites.length
        ? table(
            [{ label: "Pending invite" }, { label: "Role" }, { label: "Expires" }, { label: "" }],
            t.invites.map((i) => [esc(i.email), esc(i.role), new Date(i.expires_at).toLocaleDateString(), `<button class="quiet" data-revoke-invite="${i.id}">Revoke</button>`])
          )
        : ""
    }

    <h2>API tokens</h2>
    <p class="hint">Everyone creates their own tokens under Account. A token acts with its owner's current role (or read-only), and stops working when they leave the team. You can revoke any of them here.</p>
    ${tokens.length ? table(
      [{ label: "Token" }, { label: "Owner" }, { label: "Access" }, { label: "Last used" }, { label: "" }],
      tokens.map((k) => [`${esc(k.name)} <code>${esc(k.prefix)}…</code>`, esc(k.owner_email), k.read_only ? "read-only" : "owner's role", esc(ago(k.last_used_at)), `<button class="quiet" data-revoke-token="${k.id}">Revoke</button>`])
    ) : `<p class="hint">No tokens yet.</p>`}

    <h2>Activity</h2>
    ${table(
      [{ label: "When" }, { label: "Who" }, { label: "What" }],
      log.slice(0, 25).map((e) => [esc(ago(e.at)), esc(e.actor), `${esc(e.action)}${e.detail ? ` <span class="hint">${esc(e.detail)}</span>` : ""}`]),
      "Nothing yet."
    )}`;

  view.querySelector("#require-mfa").addEventListener("change", async (e) => {
    try {
      await api("/team/settings", { method: "POST", body: JSON.stringify({ require_mfa: e.target.checked }) });
      toast(e.target.checked ? "Two-step verification is now required" : "Two-step verification is optional");
    } catch (err) {
      e.target.checked = !e.target.checked;
      toast(err.message, true);
    }
    renderTeam();
  });
  view.querySelectorAll("[data-role]").forEach((sel) =>
    sel.addEventListener("change", async () => {
      await api(`/team/users/${sel.dataset.role}/role`, { method: "POST", body: JSON.stringify({ role: sel.value }) });
      toast("Role changed");
      renderTeam();
    })
  );
  view.querySelectorAll("[data-remove]").forEach((b) =>
    b.addEventListener("click", async () => {
      if (!confirm(`Remove ${b.dataset.email}? They're signed out immediately.`)) return;
      await api(`/team/users/${b.dataset.remove}`, { method: "DELETE" });
      renderTeam();
    })
  );
  view.querySelectorAll("[data-reset-mfa]").forEach((b) =>
    b.addEventListener("click", async () => {
      if (!confirm("Turn off their two-step verification and sign them out? Use this when someone lost their phone.")) return;
      await api(`/team/users/${b.dataset.resetMfa}/reset-mfa`, { method: "POST" });
      toast("Reset. They'll set it up again when they sign in.");
      renderTeam();
    })
  );
  onSubmit(view.querySelector("#invite"), async (fd) => {
    const r = await api("/team/invites", { method: "POST", body: JSON.stringify({ email: fd.get("email"), role: fd.get("role") }) });
    const link = `${location.origin}/#/invite/${r.token}`;
    await renderTeam();
    view.querySelector("#invite-result").innerHTML = `<div class="card"><p>Send this link to <strong>${esc(r.invite.email)}</strong>. It works once and expires in 7 days.</p>
      <div class="copy-row"><pre class="snippet">${esc(link)}</pre><button data-copy="${esc(link)}">Copy</button></div></div>`;
    view.querySelector("#invite-result [data-copy]").addEventListener("click", async (e) => (await copyText(e.target.dataset.copy)) && toast("Copied"));
  });
  view.querySelectorAll("[data-revoke-invite]").forEach((b) =>
    b.addEventListener("click", async () => {
      await api(`/team/invites/${b.dataset.revokeInvite}`, { method: "DELETE" });
      renderTeam();
    })
  );
  view.querySelectorAll("[data-revoke-token]").forEach((b) =>
    b.addEventListener("click", async () => {
      if (!confirm("Revoke this token? Anything using it stops working.")) return;
      await api(`/team/tokens/${b.dataset.revokeToken}`, { method: "DELETE" });
      renderTeam();
    })
  );
}

// ---------------------------------------------------------------- privacy

const RULE_KINDS = {
  mask_selector: {
    title: "Never read text inside",
    help: "A CSS selector, e.g. .account-summary or [data-customer]. Text inside matching elements is never recorded. Applies in the browser.",
    placeholder: ".account-summary",
  },
  redact_pattern: {
    title: "Redact a pattern",
    help: "A regular expression, e.g. ORD-\\d+ for order numbers. Matches are replaced in everything recorded, in the browser and again on the server. Add (?i) at the start to ignore case.",
    placeholder: "ORD-\\d+",
  },
  ignore_path: {
    title: "Ignore pages",
    help: "A page path; end it with * to cover everything below it, e.g. /admin/* or /account/billing. Nothing at all is recorded on these pages.",
    placeholder: "/admin/*",
  },
  drop_prop: {
    title: "Drop a property",
    help: "A custom-event property name that's never stored, e.g. customer_ref. Matching ignores case.",
    placeholder: "customer_ref",
  },
};

// Same check the collector does, so a rule that would be skipped in browsers is caught here.
function checkRuleInBrowser(kind, pattern) {
  try {
    if (kind === "redact_pattern") new RegExp(pattern.replace(/^\(\?i\)/, ""), "g");
    if (kind === "mask_selector") document.querySelector(pattern);
    return null;
  } catch {
    return kind === "redact_pattern" ? "Browsers can't use this regular expression." : "That isn't a valid CSS selector.";
  }
}

async function renderPrivacy() {
  const p = await api("/privacy");
  const admin = can("admin");
  const byKind = (kind) => p.rules.filter((r) => r.kind === kind);
  const ruleTable = (kind) => {
    const rules = byKind(kind);
    if (!rules.length) return "";
    return table(
      [{ label: kind === "redact_pattern" ? "Pattern → replacement" : "Rule" }, { label: "Note" }, { label: "Added" }, { label: "" }],
      rules.map((r) => [
        `<code>${esc(r.pattern)}</code>${r.replacement ? ` → <code>${esc(r.replacement)}</code>` : ""}${r.enabled ? "" : ` <span class="chip">off</span>`}`,
        esc(r.label),
        `${esc(ago(r.created_at))} <span class="hint">by ${esc(r.created_by)}</span>`,
        admin
          ? `<button class="quiet" data-toggle="${r.id}" data-on="${r.enabled ? 0 : 1}">${r.enabled ? "Turn off" : "Turn on"}</button><button class="quiet" data-delete-rule="${r.id}">Remove</button>`
          : "",
      ])
    );
  };
  view.innerHTML = `
    <h1>Privacy</h1>
    <p class="sub">Diagnostic studies patterns, not people. These protections always apply. Add your own rules for anything specific to your site.</p>

    <h2>Always on</h2>
    <div class="card"><ul class="checklist">${p.built_in.map((b) => `<li>${esc(b)}</li>`).join("")}</ul></div>

    <h2>Your rules</h2>
    <p class="hint">Changes apply on the server straight away and reach visitors' browsers within an hour, as soon as their cached copy of the collector is refreshed.${admin ? "" : " Only admins can change rules."}</p>
    ${Object.entries(RULE_KINDS)
      .map(([kind, k]) => {
        const t = ruleTable(kind);
        return t ? `<h3>${esc(k.title)}</h3>${t}` : "";
      })
      .join("") || `<div class="empty">No rules of your own yet.${admin ? " Add one below." : ""}</div>`}

    ${
      admin
        ? `<h2>Add a rule</h2>
    <form class="card stack" id="add-rule">
      <div class="row">
        <label class="field narrow">Kind
          <select name="kind">${Object.entries(RULE_KINDS).map(([kind, k]) => `<option value="${kind}">${esc(k.title)}</option>`).join("")}</select></label>
        <label class="field">Pattern <input name="pattern" required></label>
        <label class="field narrow" id="replacement-field">Replace with <input name="replacement" placeholder="[redacted]"></label>
      </div>
      <p class="hint" id="rule-help"></p>
      <label class="field">Note (optional) <input name="label" placeholder="Why this rule exists, e.g. order numbers identify customers"></label>
      <div><button class="primary" type="submit">Add rule</button></div>
    </form>`
        : ""
    }

    <h2>Try it</h2>
    <form class="card stack" id="try-rules">
      <label class="field">Sample text, as it might appear on a button, a link or in an error message
        <textarea name="text" rows="2">Order ORD-20931 for jane.doe@example.com: pay €1,234.56 with 4242 4242 4242 4242</textarea></label>
      <label class="field">Page path <input name="path" value="/account/jane.doe@example.com/orders/ORD-20931"></label>
      <div><button type="submit">Show what would be stored</button></div>
      <div id="try-result"></div>
    </form>`;

  view.querySelectorAll("[data-toggle]").forEach((b) =>
    b.addEventListener("click", async () => {
      await api(`/privacy/rules/${b.dataset.toggle}`, { method: "POST", body: JSON.stringify({ enabled: b.dataset.on === "1" }) });
      renderPrivacy();
    })
  );
  view.querySelectorAll("[data-delete-rule]").forEach((b) =>
    b.addEventListener("click", async () => {
      if (!confirm("Remove this rule?")) return;
      await api(`/privacy/rules/${b.dataset.deleteRule}`, { method: "DELETE" });
      renderPrivacy();
    })
  );
  const form = view.querySelector("#add-rule");
  if (form) {
    const sync = () => {
      const k = RULE_KINDS[form.kind.value];
      view.querySelector("#rule-help").textContent = k.help;
      form.pattern.placeholder = k.placeholder;
      view.querySelector("#replacement-field").hidden = form.kind.value !== "redact_pattern";
    };
    form.kind.addEventListener("change", sync);
    sync();
    onSubmit(form, async (fd) => {
      const kind = fd.get("kind");
      const problem = checkRuleInBrowser(kind, String(fd.get("pattern")).trim());
      if (problem) throw new Error(problem);
      await api("/privacy/rules", {
        method: "POST",
        body: JSON.stringify({ kind, pattern: fd.get("pattern"), replacement: fd.get("replacement") || null, label: fd.get("label") }),
      });
      toast("Rule added");
      renderPrivacy();
    });
  }
  onSubmit(view.querySelector("#try-rules"), async (fd) => {
    const r = await api("/privacy/test", { method: "POST", body: JSON.stringify({ text: fd.get("text"), path: fd.get("path") }) });
    view.querySelector("#try-result").innerHTML = `
      <div class="table-wrap"><table><tbody>
        <tr><th>On a button or link</th><td><code>${esc(r.page_text)}</code></td></tr>
        <tr><th>In an error message</th><td><code>${esc(r.error_text)}</code></td></tr>
        ${r.path != null ? `<tr><th>Page</th><td>${r.path_ignored ? `<strong>ignored</strong>: nothing is recorded on this page` : `<code>${esc(r.path)}</code>`}</td></tr>` : ""}
      </tbody></table></div>`;
  });
}

// Decides between the sign-in screens and the app.
async function boot() {
  try {
    auth = await api("/auth/state");
  } catch (e) {
    view.innerHTML = `<div class="empty">Couldn't reach the Diagnostic server: ${esc(e.message)}</div>`;
    return;
  }
  const hash = location.hash;
  if (auth.needs_setup) return renderWelcome(hash.startsWith("#/welcome/") ? hash.slice("#/welcome/".length) : "");
  if (!auth.user) return hash.startsWith("#/invite/") ? renderInvite(hash.slice("#/invite/".length)) : renderLogin();
  if (auth.step === "verify") return renderVerify();
  if (auth.step === "enroll") return renderEnroll();
  document.body.classList.remove("auth");
  if (/^#\/(welcome|invite)\//.test(hash)) history.replaceState(null, "", "#/");
  applyRole();
  route();
}

const routes = { ideas: renderIdeas, kpis: renderKpis, behaviour: renderBehaviour, privacy: renderPrivacy, setup: renderSetup, account: renderAccount, team: renderTeam };

async function route() {
  if (!auth || !auth.user || auth.step) return boot();
  if (!project) {
    try {
      await loadProject();
    } catch (e) {
      view.innerHTML = `<div class="empty">Couldn't load this page: ${esc(e.message)}</div>`;
      return;
    }
  }
  // Land on Setup until data is arriving, on Ideas after.
  const name = location.hash.replace(/^#\//, "") || (project.collection.active ? "ideas" : "setup");
  if (!location.hash) history.replaceState(null, "", `#/${name}`);
  const render = routes[name] || renderIdeas;
  document.querySelectorAll("#nav a").forEach((a) => a.classList.toggle("active", a.dataset.view === name));
  try {
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
boot();
