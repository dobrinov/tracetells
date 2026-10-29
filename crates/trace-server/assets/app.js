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

// "system" follows the OS; the choice is per browser.
const themeChoice = () => store("trace_theme") || "system";
function setTheme(choice) {
  store("trace_theme", choice === "system" ? null : choice);
  if (choice === "system") delete document.documentElement.dataset.theme;
  else document.documentElement.dataset.theme = choice;
}

let toastTimer;
function toast(message, isError = false) {
  toastEl.textContent = message;
  toastEl.className = "toast" + (isError ? " error" : "");
  toastEl.hidden = false;
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => (toastEl.hidden = true), isError ? 8000 : 4000);
}

// A website's public, read-only view lives at /p/{token}: no session, only
// the aggregated pages, read from /v1/public/{token}/....
const PUBLIC = (location.pathname.match(/^\/p\/([A-Za-z0-9_-]{20,})\/?$/) || [])[1] || null;
const PUBLIC_PAGES = ["insights", "kpis", "behaviour"];

// Every request is JSON: the server rejects cookie-authenticated writes that
// aren't, which is what stops other sites from forging requests (CSRF).
async function api(path, opts = {}) {
  // The selected website goes with every call; account-level endpoints ignore it.
  const siteHeader = currentSite ? { "X-Trace-Site": currentSite } : {};
  const url = PUBLIC ? `/v1/public/${PUBLIC}${path}` : `/api${path}`;
  const res = await fetch(url, { ...opts, headers: { "Content-Type": "application/json", ...siteHeader, ...(opts.headers || {}) } });
  const type = res.headers.get("content-type") || "";
  const body = type.includes("json") ? await res.json() : await res.text();
  if (res.status === 401 && !PUBLIC && !path.startsWith("/auth/")) {
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
// The account's websites and the one being looked at (its slug).
let sites = [];
let currentSite = store("trace_site");
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
  document.getElementById("site-dot").classList.toggle("on", c.active);
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

const siteButton = document.getElementById("site-button");
const siteList = document.getElementById("site-list");

async function loadSites() {
  sites = await api("/sites");
  if (!sites.some((s) => s.slug === currentSite)) currentSite = sites.length ? sites[0].slug : null;
  store("trace_site", currentSite);
  document.getElementById("site-menu").hidden = !sites.length;
  const current = sites.find((s) => s.slug === currentSite);
  document.getElementById("site-name").textContent = current ? current.name : "";
  siteButton.setAttribute("aria-label", current ? `Website: ${current.name}` : "Website");
  siteList.innerHTML =
    `<div class="account-name">${esc(auth.account ? auth.account.name : "Websites")}</div>` +
    sites
      .map(
        (s) => `<button type="button" role="option" data-site="${esc(s.slug)}" aria-selected="${s.slug === currentSite}">
          <span class="dot ${s.collection && s.collection.active ? "on" : ""}"></span>
          <span><span>${esc(s.name)}</span><span class="site-host">${esc(s.collection && s.collection.sources.length ? host(s.collection.sources[0].origin) : s.slug)}</span></span>
        </button>`
      )
      .join("") +
    (can("admin")
      ? `<hr><button type="button" class="add" data-add-site><svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" aria-hidden="true"><path d="M12 5v14M5 12h14"/></svg>Add website</button>`
      : "") +
    accountsMenu();
}

// One login can belong to several accounts. The menu lists the others, and
// a new account when this server lets people create them.
function accountsMenu() {
  const others = (auth.accounts || []).filter((a) => !auth.account || a.account_id !== auth.account.id);
  if (!others.length && !auth.can_create_account) return "";
  return (
    `<hr><div class="account-name">Your other accounts</div>` +
    others
      .map((a) => `<button type="button" data-account="${a.account_id}"><span><span>${esc(a.name)}</span><span class="site-host">${esc(a.role)}</span></span></button>`)
      .join("") +
    (auth.can_create_account
      ? `<button type="button" class="add" data-new-account><svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" aria-hidden="true"><path d="M12 5v14M5 12h14"/></svg>New account</button>`
      : "")
  );
}

// After the session moved to another account: its websites, its role, its first page.
async function enterAccount(state) {
  auth = state;
  currentSite = null;
  store("trace_site", null);
  project = null;
  kpiCache = [];
  checklist = null;
  history.replaceState(null, "", "#/");
  if (auth.step) return boot();
  applyRole();
  await loadSites();
  route();
}

async function switchAccount(id) {
  await enterAccount(await post("/account/switch", { account_id: Number(id) }));
  toast(`Switched to ${auth.account.name}`);
}

async function newAccount() {
  const name = prompt("Name of the new account, e.g. your company or project. You'll own it, with the same login:");
  if (!name) return;
  await enterAccount(await post("/account/new", { name }));
  toast(`Created ${auth.account.name}. Add its first website's snippet on Setup.`);
}

function toggleSiteList(open = siteList.hidden) {
  siteList.hidden = !open;
  siteButton.setAttribute("aria-expanded", String(open));
  if (open) (siteList.querySelector('[aria-selected="true"]') || siteList.querySelector("button"))?.focus();
}

async function addWebsite() {
  const name = prompt("Name of the new website, e.g. \"Docs\" or \"Shop (staging)\":");
  if (!name) return false;
  const s = await post("/sites", { name });
  toast(`Added ${s.name}. Install its snippet on Setup.`);
  currentSite = s.slug;
  store("trace_site", currentSite);
  await loadSites();
  return true;
}

// Another website opens where a fresh visit to it would: Get started until data
// arrives, Insights after. The page open for the previous website (its onboarding,
// say) may mean nothing for this one.
function switchSite(slug) {
  currentSite = slug;
  store("trace_site", slug);
  project = null;
  kpiCache = [];
  checklist = null;
  history.replaceState(null, "", "#/");
  route();
}

siteButton.addEventListener("click", () => toggleSiteList());
siteList.addEventListener("click", async (e) => {
  const pick = e.target.closest("[data-site]");
  if (pick) {
    toggleSiteList(false);
    if (pick.dataset.site !== currentSite) {
      switchSite(pick.dataset.site);
      await loadSites();
    }
    return;
  }
  const account = e.target.closest("[data-account]");
  if (account || e.target.closest("[data-new-account]")) {
    toggleSiteList(false);
    try {
      await (account ? switchAccount(account.dataset.account) : newAccount());
    } catch (err) {
      toast(err.message, true);
    }
    return;
  }
  if (e.target.closest("[data-add-site]")) {
    toggleSiteList(false);
    try {
      if (await addWebsite()) switchSite(currentSite);
    } catch (err) {
      toast(err.message, true);
    }
  }
});
siteList.addEventListener("keydown", (e) => {
  const items = [...siteList.querySelectorAll("button")];
  const i = items.indexOf(document.activeElement);
  if (e.key === "ArrowDown" || e.key === "ArrowUp") {
    e.preventDefault();
    items[(i + (e.key === "ArrowDown" ? 1 : -1) + items.length) % items.length].focus();
  }
});
document.addEventListener("keydown", (e) => {
  if (e.key === "Escape" && !siteList.hidden) {
    toggleSiteList(false);
    siteButton.focus();
  }
});
document.addEventListener("click", (e) => {
  if (!siteList.hidden && !e.target.closest("#site-menu")) toggleSiteList(false);
});

async function loadProject() {
  project = await api("/project");
  showCollection();
  if (PUBLIC) return;
  loadChecklist();
  // Events dropped because of the monthly limit: say so on every page.
  const notice = document.getElementById("notice");
  try {
    const p = await api("/plan");
    const events = p.limits.find((l) => l.key === "monthly_events");
    notice.hidden = !p.events_dropped;
    if (p.events_dropped)
      notice.innerHTML = `<div><span><strong>${num(p.events_dropped)} events weren't recorded this month.</strong> This account reached its limit of ${num(events.limit)} events. Your website is not affected.</span> <a href="#/plan">Plan & usage</a></div>`;
  } catch {
    notice.hidden = true;
  }
}

// ---------------------------------------------------------------- Insights

let insightTab = store("trace_tab") || "new";

function kpiValue(k, v) {
  if (v == null) return "–";
  return k.kind === "per_session" ? Number(v).toFixed(2) : pct(v);
}

function insightActions(insight) {
  const b = (status, label, cls = "") => `<button class="${cls}" data-status="${status}">${label}</button>`;
  switch (insight.status) {
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

const VERDICTS = { measuring: "Measuring", improved: "Improved after release", regressed: "Worse after release", no_change: "No change", insufficient_data: "Not enough data" };

// The KPI's verdict, and each measured evidence item's: did the problem itself shrink?
function verdictBlock(insight) {
  if (insight.status !== "released") return "";
  const measured = insight.evidence.some((e) => e.metric);
  let kpi;
  if (!insight.kpi_id) {
    kpi = measured ? "" : `<div class="verdict"><span class="badge">Not measured</span><span>Link a KPI to measure the impact of this release.</span></div>`;
  } else if (!insight.evaluation) {
    kpi = `<div class="verdict"><span class="badge">Pending</span><span>Not evaluated yet. Trace checks released insights every hour.</span></div>`;
  } else {
    const e = insight.evaluation;
    kpi = `<div class="verdict ${esc(e.verdict)}"><span class="badge">${esc(VERDICTS[e.verdict] || e.verdict)}</span><span>${esc(e.summary)}</span></div>`;
  }
  const items = (insight.item_verdicts || []).map(
    (v) => `<div class="verdict ${esc(v.verdict)} item"><span class="badge">${esc(v.verdict === "regressed" ? "Worse" : v.verdict === "improved" ? "Better" : VERDICTS[v.verdict] || v.verdict)}</span><span>${esc(v.summary)}</span></div>`
  );
  if (!items.length && measured && insight.released_at) {
    const due = shortDate(insight.released_at + 14 * 86400000);
    items.push(`<div class="verdict measuring item"><span class="badge">Measuring</span><span>The evidence is compared over the 14 days before and after the release; the result comes on ${esc(due)}.</span></div>`);
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
  const kpiOptions =
    `<option value="">No KPI</option>` +
    kpiCache.map((k) => `<option value="${k.id}" ${k.id === insight.kpi_id ? "selected" : ""}>${esc(k.name)}</option>`).join("");
  return `
  <article class="card ${focused ? "focused" : ""}" data-id="${insight.id}">
    <div class="meta">
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
      ${
        can("user")
          ? `${insight.evidence.some((e) => e.metric) && insight.status !== "dismissed" ? `<button class="quiet" data-refresh title="Measure the evidence again with fresh data">Refresh</button>` : ""}<select data-kpi aria-label="KPI this insight should move">${kpiOptions}</select>${insightActions(insight)}`
          : ""
      }
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
    const label = btn.textContent;
    btn.textContent = "Copied ✓";
    setTimeout(() => (btn.textContent = label), 1600);
  } else {
    toast("Couldn't copy. Select the prompt in the panel and copy it from there.", true);
  }
}

async function renderInsights() {
  const [insights, kpis, analysis] = await Promise.all([api("/insights"), api("/kpis"), api("/analysis").catch(() => null)]);
  kpiCache = kpis;
  const counts = Object.fromEntries(STATUSES.map(([s]) => [s, insights.filter((i) => i.status === s).length]));
  const shown = insights.filter((i) => i.status === insightTab);
  if (!shown.some((i) => i.id === focusedInsight)) focusedInsight = shown.length ? shown[0].id : null;
  view.innerHTML = `
    <h1>Insights</h1>
    <p class="sub">${
      PUBLIC
        ? `What a coding agent (or Trace's built-in checks) found in how people use ${esc(project.name)}, with the change it suggests, and for released ones whether it moved its KPI. Each one is also a ready prompt for a coding agent.`
        : "What your coding agent (or Trace's built-in checks) found in real behaviour, with the change it suggests. Copy one as a prompt, move it along as you work, and once it's released Trace measures whether it moved its KPI."
    }</p>
    ${analysis ? analysisPanel(analysis) : ""}
    <div class="chips" role="tablist" aria-label="Status">
      ${STATUSES.filter(([s]) => !(PUBLIC && s === "dismissed")).map(
        ([s, label]) =>
          `<button role="tab" data-tab="${s}" class="${s === insightTab ? "active" : ""}" aria-selected="${s === insightTab}">${label}<span class="count">${counts[s]}</span></button>`
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
              : insightTab === "new"
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
    card.querySelector("[data-kpi]")?.addEventListener("change", async (e) => {
      await post(`/insights/${id}/kpi`, { kpi_id: e.target.value ? Number(e.target.value) : null });
      toast("KPI updated");
      renderInsights();
    });
    card.querySelector("[data-refresh]")?.addEventListener("click", async (e) => {
      e.currentTarget.disabled = true;
      try {
        await post(`/insights/${id}/refresh`);
        toast("Measured again with fresh data");
        renderInsights();
      } catch (err) {
        toast(err.message, true);
      }
    });
    card.querySelectorAll("[data-status]").forEach((btn) =>
      btn.addEventListener("click", async () => {
        const status = btn.dataset.status;
        let reason = null;
        let released_at = null;
        if (status === "dismissed") {
          reason = prompt("Why dismiss this insight? The analyst uses your reason to avoid similar suggestions.", "");
          if (reason === null) return;
        }
        if (status === "released") {
          // The before/after split is when the change went live, which may be earlier than now.
          const pad = (n) => String(n).padStart(2, "0");
          const d = new Date();
          const nowText = `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}`;
          const when = prompt("When did this change go live? Keep now, or enter the deploy's time (your local time).", nowText);
          if (when === null) return;
          if (when.trim() !== nowText) {
            const at = new Date(when.trim().replace(" ", "T"));
            if (isNaN(at)) return toast("Use a time like 2026-10-28 14:05");
            released_at = at.toISOString();
          }
        }
        await post(`/insights/${id}/status`, { status, reason, released_at });
        if (status === "released") {
          await post("/evaluate");
          toast("Released. Trace compares the KPI over the 14 days before and after, and gives its verdict once those 14 days have passed.");
        }
        renderInsights();
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

// Where the value and target sit on a bar: rates use 0–100%, per-session counts
// scale to whichever of value and target is larger.
function kpiBar(k) {
  if (k.current == null) return "";
  const max = k.kind === "per_session" ? Math.max(k.current, k.target ?? 0) * 1.25 || 1 : 1;
  const at = (v) => Math.max(0, Math.min(100, (v / max) * 100));
  const tone = k.target == null || k.target_met ? "" : k.direction === "decrease" ? "bad" : "";
  return `<div class="bar ${tone}" role="img" aria-label="${esc(k.name)}: ${kpiValue(k, k.current)}${k.target != null ? `, target ${kpiValue(k, k.target)}` : ""}">
    <span style="width:${at(k.current).toFixed(1)}%"></span>
    ${k.target != null ? `<i class="target" style="left:${at(k.target).toFixed(1)}%"></i>` : ""}
  </div>`;
}

function kpiCard(k) {
  const target = k.target != null ? `Target ${k.direction === "increase" ? "≥" : "≤"} ${kpiValue(k, k.target)}${k.target_met ? " ✓" : ""}` : "No target";
  return `
    <div class="card kpi-card">
      <div class="head"><span class="name">${esc(k.name)}</span><span class="key" title="Key in trace.json">${esc(k.key || "")}</span></div>
      <div class="value-row"><span class="value">${kpiValue(k, k.current)}</span>${deltaHtml(k)}</div>
      ${kpiBar(k)}
      <span class="hint">${target} · ${num(k.sessions)} sessions</span>
      <div class="def">${esc(k.description || k.summary)}</div>
    </div>`;
}

// ---------------------------------------------------------------- Analysis

// When an analysis last ran, what it read, how much is new since, and what to
// do about it: run /trace:analyse in the coding agent, or analyse here.
const ANALYSE_COMMAND = "/trace:analyse";

function fullDate(ts) {
  return new Date(ts).toLocaleString(undefined, { day: "numeric", month: "short", hour: "2-digit", minute: "2-digit" });
}

function analysisPanel(a) {
  const last = a.last_analysed_at
    ? `<span><strong>${esc(ago(a.last_analysed_at))}</strong>${a.analysed_by ? ` by ${esc(a.analysed_by)}` : ""}</span><span class="hint">It read behaviour up to ${esc(fullDate(a.last_analysed_at))}.</span>`
    : `<strong>Never</strong><span class="hint">Nothing has been analysed yet.</span>`;
  const progress = Math.min(100, a.needed ? (a.new_sessions / a.needed) * 100 : 0);
  const title = {
    analyse: a.last_analysed_at ? "Run an analysis now" : "Run the first analysis",
    wait: `Worth analysing at ${num(a.needed)} new sessions`,
    nothing_new: "Analysis is up to date",
  }[a.next];
  const why = {
    analyse: `There's enough new behaviour for findings worth having. ${a.last_analysed_at ? "The last analysis didn't see any of it." : ""}`,
    wait: `${num(a.needed - a.new_sessions)} more to go. You can analyse earlier, but findings from little data are often noise.`,
    nothing_new: "No sessions since the last analysis. Trace will say here when there's something new to read.",
  }[a.next];
  const how = PUBLIC || auth?.impersonating
    ? ""
    : a.agent_connected
      ? `<div class="analysis-how">
          <span>In Claude Code, run</span>
          <code class="command">${ANALYSE_COMMAND}</code>
          <button type="button" data-copy="${ANALYSE_COMMAND}">Copy</button>
          <button type="button" class="link" data-analyse-here>or analyse here</button>
          <span class="hint">It shows you what it found; the insights you keep are saved here and this panel resets.</span>
        </div>`
      : `<div class="analysis-how">
          <span>Analysis runs in your coding agent, which reads your code too.</span>
          <a class="button primary" href="#/setup">Connect your coding agent</a>
          <button type="button" data-analyse-here>Analyse here instead</button>
        </div>`;
  return `<section class="analysis-panel next-${esc(a.next)}" aria-label="Analysis">
    <div class="analysis-head">
      <div><h2>${esc(title)}</h2><p class="hint">${esc(why)}</p></div>
    </div>
    <div class="analysis-facts">
      <div><span class="label">Last analysis</span>${last}</div>
      <div><span class="label">${a.last_analysed_at ? "New since then" : "Waiting to be analysed"}</span><strong>${num(a.new_sessions)} sessions</strong><span class="hint">${num(a.new_events)} events</span></div>
      <div><span class="label">Worth analysing at</span><strong>${num(a.needed)} new sessions</strong>
        <div class="analysis-bar" role="progressbar" aria-label="New sessions towards the next analysis" aria-valuemin="0" aria-valuemax="${a.needed}" aria-valuenow="${Math.min(a.new_sessions, a.needed)}"><span style="width:${progress.toFixed(1)}%"></span></div>
      </div>
    </div>
    ${a.next === "nothing_new" ? "" : how}
  </section>`;
}

// Wires a rendered panel: copy, analyse here, and noticing when the agent saves.
function wireAnalysisPanel(a) {
  view.querySelectorAll(".analysis-panel [data-copy]").forEach((b) =>
    b.addEventListener("click", async () => {
      if (await copyText(b.dataset.copy)) toast("Copied. Paste it into Claude Code.");
    })
  );
  view.querySelectorAll("[data-analyse-here]").forEach((b) => b.addEventListener("click", () => document.getElementById("run-analysis").click()));
  watchAnalysis(a);
}

// While a panel asks for an analysis, check every 20 s whether one arrived, so
// running /trace:analyse in the terminal shows up here without a reload.
let analysisWatch = null;
function watchAnalysis(a) {
  clearInterval(analysisWatch);
  if (PUBLIC || a.next !== "analyse") return;
  const page = location.hash;
  analysisWatch = setInterval(async () => {
    if (location.hash !== page || document.hidden) return;
    const now = await api("/analysis").catch(() => null);
    if (!now || now.last_analysed_at === a.last_analysed_at) return;
    clearInterval(analysisWatch);
    toast(`Analysed just now${now.analysed_by ? ` by ${now.analysed_by}` : ""}.`);
    route();
  }, 20000);
}

// The header button says when there's something to analyse.
function analysisBadge(a) {
  const btn = document.getElementById("run-analysis");
  if (!btn || btn.disabled) return;
  btn.classList.toggle("ready", a.next === "analyse");
  btn.textContent = a.next === "analyse" ? `Run analysis · ${num(a.new_sessions)} new` : "Run analysis";
  btn.title = a.message;
}

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
    ${cell(num(a.visitors.visitors), "Visitors", change(a.visitors.visitors, a.visitors.previous_visitors))}
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
  const [kpis, cfg, funnels] = await Promise.all([api("/kpis"), api("/config"), api("/funnels").catch(() => [])]);
  kpiCache = kpis;
  const push = `trace push trace.json --server ${location.origin} --token $TRACE_TOKEN   # create one under Account`;
  const source = cfg.synced_at
    ? `<span class="source" title="To change a KPI, edit trace.json in your repository and push it."><code>${esc(cfg.source || "trace.json")}</code> · synced ${esc(ago(cfg.synced_at))}</span>`
    : "";
  const cards = kpis.length
    ? `<div class="kpi-grid">${kpis.map(kpiCard).join("")}</div>`
    : `<div class="card">
        <p><strong>No KPIs yet.</strong> KPIs live in your application's code, next to the events that feed them. Add a <code>trace.json</code> to your repository:</p>
        <div class="copy-row"><pre class="snippet">${esc(CONFIG_EXAMPLE)}</pre><button data-copy="${esc(CONFIG_EXAMPLE)}">Copy</button></div>
        <p class="hint">Check it with <code>trace check</code>, then publish it, for example from CI:</p>
        <div class="copy-row"><pre class="snippet">${esc(push)}</pre><button data-copy="${esc(push)}">Copy</button></div>
      </div>`;

  const declared = cfg.events.map((e) => [
    `<code>${esc(e.name)}</code>`,
    esc(e.description),
    num(e.count),
    eventStatus(e, cfg.collecting_since),
  ]);
  const undeclared = cfg.undeclared_events.map((e) => [
    `<code>${esc(e.name)}</code>`,
    `<span class="hint">Sent by your app but not declared in trace.json</span>`,
    num(e.count),
    `<span class="status-dot">● undeclared</span>`,
  ]);

  const steps = `trace check\ntrace push --server ${location.origin} --site ${project.slug}`;
  view.innerHTML = `
    <div class="page-head">
      <div><h1>KPIs</h1><p class="sub">The last 14 days against the 14 before. Your team defines these in code; this page only shows how they're doing, and released insights are judged against them.</p></div>
      ${source}
    </div>
    ${cards}
    ${funnelsSection(funnels)}
    <h2>Tracking health</h2>
    <div class="side-by-side">
      <div>
        ${table(
          [{ label: "Event" }, { label: "What it means" }, { label: "Last 14 days", num: 1 }, { label: "Status" }],
          [...declared, ...undeclared],
          "No custom events declared or received yet."
        )}
        <p class="hint">Send events with <code>trace.track("name")</code>, or without JavaScript using <code>data-trace-event="name"</code> on a button, link or form.</p>
      </div>
      <div class="dark-panel" ${PUBLIC ? "hidden" : ""}>
        <strong>Change a KPI in your repo</strong>
        <span>Edit <code>trace.json</code> next to the code that sends the events, then:</span>
        <pre>${esc(steps)}</pre>
        <span class="hint">Run them in CI with a personal API token (<code>TRACE_TOKEN</code>), so the dashboard always matches your main branch.</span>
      </div>
    </div>`;
  view.querySelectorAll("[data-copy]").forEach((b) =>
    b.addEventListener("click", async () => {
      if (await copyText(b.dataset.copy)) toast("Copied");
    })
  );
}

// ---------------------------------------------------------------- Behaviour

let digestDays = Number(store("trace_days")) || 14;

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
        ["Visitors", t.visitors, "Signed-in people your site names with trace.identify() count once. Everyone else counts once a day: nothing is stored on their device to recognise them by."],
        ["Pageviews", t.pageviews],
        ["Events", t.events],
      ]
        .map(([l, v, hint]) => `<div class="card stat"${hint ? ` title="${esc(hint)}"` : ""}><div class="value">${num(v)}</div><div class="label">${l}</div></div>`)
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
      d.custom_events.map((x) => [esc(x.name), x.declared ? esc(x.description) : `<span class="hint">not declared in trace.json</span>`, num(x.count), num(x.sessions)]),
      "No custom events. Send them with trace.track(name, props)."
    )}`;
  view.querySelector("#days").addEventListener("change", (e) => {
    digestDays = Number(e.target.value);
    store("trace_days", String(digestDays));
    renderBehaviour();
  });
}

// ---------------------------------------------------------------- Setup

// Snippets for the current website, shared by Setup and Get started.
// Whether install snippets wait for the site's own consent tool. Off by default:
// the collector is cookieless, so there is nothing to ask consent for.
const askConsent = () => store("trace_wait_consent") === "yes";

/// The "wait for consent" checkbox shown above install snippets.
function consentToggle() {
  return `<p class="hint">Cookieless by default: Trace stores nothing on visitors' devices and shows no banner. It tells visits apart with a hash that changes every day and can't be traced back.</p>
    <label class="check-row"><input type="checkbox" data-ask-consent ${askConsent() ? "checked" : ""}>
    <span><strong>Wait for my consent tool</strong> <span class="hint">For sites that already ask for consent. Nothing is sent until your tool calls <code>trace.consent(true)</code>; after a yes, Trace keeps an id so it can follow visits across days.</span></span></label>`;
}

function bindConsentToggle(rerender) {
  view.querySelector("[data-ask-consent]")?.addEventListener("change", (e) => {
    store("trace_wait_consent", e.target.checked ? "yes" : "no");
    rerender();
  });
}

function setupSnippets() {
  // ?k= serves this website's privacy rules built into the collector.
  const src = `${location.origin}/d.js?k=${project.write_key}`;
  const key = project.write_key;
  const consent = askConsent() ? ` data-consent="required"` : "";
  const consentInit = askConsent() ? `, consent: "required"` : "";
  const stub = `<script>window.trace=window.trace||{q:[]};["track","identify","page","init","consent"].forEach(function(m){trace[m]=trace[m]||function(){trace.q.push([m,arguments])}})</script>`;
  const installs = {
    site: {
      label: "Website",
      hint: "Any site where links reload the page: server-rendered apps, CMSs, static sites. Paste into every page before </head>. Sessions continue across page loads, and back/forward restores count as pageviews.",
      code: `<script src="${src}" data-key="${key}"${consent} defer></script>`,
    },
    spa: {
      label: "Single-page app",
      hint: "React, Vue, Svelte, Angular and others. Route changes through the history API, the Navigation API or hash routes (#/settings) are picked up automatically. Use data-routes to group dynamic URLs. Numeric and UUID segments become :id on their own. The stub lets your code call trace.track() before the script has loaded.",
      code: `${stub}\n<script src="${src}" data-key="${key}"${consent} data-routes="/products/:slug, /docs/*" defer></script>`,
    },
    next: {
      label: "Next.js",
      hint: "In app/layout.tsx (App Router) or pages/_app.tsx. App Router navigations are tracked automatically.",
      code: `import Script from "next/script";\n\n<Script src="${src}" data-key="${key}"${consent} data-routes="/products/:slug" strategy="afterInteractive" />`,
    },
    bundle: {
      label: "Tag manager / module",
      hint: "When the script tag can't carry data-* attributes (Google Tag Manager, type=\"module\", dynamic loading), configure it in code instead. Calls made before the script loads are queued.",
      code: `${stub}\n<script>\n  trace.init({ key: "${key}", routes: ["/products/:slug"]${consentInit} });\n</script>\n<script src="${src}" async></script>`,
    },
  };
  // No token to paste: the client opens Trace in the browser to sign in and approve.
  const mcpCommand = `claude mcp add --transport http trace ${location.origin}/mcp`;
  // The Claude Code plugin: the MCP connection plus skills for setup, analysis and fixes.
  const pluginInstall = `/plugin marketplace add dobrinov/tracetells\n/plugin install trace@tracetells`;
  const hosted = location.hostname === "app.tracetells.com";
  const pluginServerNote = hosted ? "" : ` and, when it asks for the <strong>Trace MCP server</strong>, enter <code>${esc(location.origin)}/mcp</code>`;
  const mcpJson = JSON.stringify({ mcpServers: { trace: { url: `${location.origin}/mcp` } } }, null, 2);
  const setupPrompt =
    `Set up Trace (${location.origin}) for this project with the trace MCP tools: use the website "${project.slug}", ` +
    `add its install snippet to every page, write trace.json with KPIs for what matters in this product and push it, ` +
    `and mark screens that show customer data with data-trace-mask. Show me every change before you make it.`;
  const custom = `<button data-trace-event="add_to_cart">Add to cart</button>   <!-- no JavaScript needed -->

trace.track("purchase", { plan: "pro" });\ntrace.identify(currentUser.id);      // optional: tie sessions to your user id\ntrace.page("/checkout/:step");       // optional: name the current route yourself\n                                        // (set pageviews: "manual" to turn off automatic pageviews)`;
  const push = `trace check\ntrace push --server ${location.origin} --site ${project.slug} --token $TRACE_TOKEN`;
  // A Claude Code hook that says, when a session starts, whether it's worth analysing.
  const analysisHook = JSON.stringify(
    {
      hooks: {
        SessionStart: [
          {
            matcher: "startup",
            hooks: [
              {
                type: "command",
                command: `curl -fsS --max-time 3 -H "Authorization: Bearer $TRACE_TOKEN" "${location.origin}/api/sites/${project.slug}/analysis?format=claude-hook" || true`,
              },
            ],
          },
        ],
      },
    },
    null,
    2
  );
  return { installs, mcpCommand, mcpJson, pluginInstall, pluginServerNote, setupPrompt, custom, push, analysisHook };
}

// A task a coding agent can follow to install the collector.
function installPrompt(code) {
  return `Add the Trace analytics collector to this website.

Put this in the <head> of every page, in the shared layout or template so it loads on every route:

${code}

It loads after the page and never records what people type.${
    askConsent()
      ? `

It waits for the site's own consent tool (data-consent="required"): call trace.consent(true) when the visitor agrees and trace.consent(false) when they decline or withdraw. Check that before agreeing no requests go to ${location.origin}/v1/collect, and after it they return 204.`
      : ` It is cookieless: it stores nothing on visitors' devices and needs no consent banner. Then open a page and check that requests to ${location.origin}/v1/collect return 204.`
  }`;
}

// The public view card: off, or on with its link.
function publicCard(pub) {
  const what = `Anyone with the link sees ${esc(project.name)}'s KPIs, behaviour, errors and insights, read-only. Never settings, people, tokens, the write key or privacy rules, and rows from fewer than 5 sessions are left out.`;
  if (!pub || !pub.public)
    return `<p class="hint">Off. ${what}</p>${can("admin") ? `<button id="public-on">Make public</button>` : ""}`;
  return `<p class="hint">On since ${esc(new Date(pub.since).toLocaleDateString())}. ${what}</p>
    <div class="copy-row"><pre class="snippet">${esc(pub.url)}</pre><button data-copy="${esc(pub.url)}">Copy</button></div>
    ${can("admin") ? `<div class="actions"><button class="quiet" id="public-rotate">Replace link</button><button class="quiet danger" id="public-off">Make private</button></div>` : ""}`;
}

async function renderSetup() {
  const [runs, pub] = await Promise.all([api("/runs"), api("/publish").catch(() => null)]);
  const { installs, mcpCommand, mcpJson, pluginInstall, pluginServerNote, setupPrompt, custom, analysisHook } = setupSnippets();
  let installTab = store("trace_install") || "site";
  if (!installs[installTab]) installTab = "site";
  view.innerHTML = `
    <h1>Setup</h1>
    <p class="sub">Install the collector, then let the analyst do its work. New here? <a href="#/start">Follow the setup checklist</a>.</p>
    <div class="card setup-status" id="setup-status">${setupStatus()}</div>


    <h2>1. Add the collector to your site</h2>
    <p class="hint">The collector records pageviews, clicks, rage and dead clicks, errors, scroll depth and form starts/submits. It never records what users type. Add <code>data-trace-mask</code> to any element whose text is private. It does its work when the browser is idle, so it doesn't slow your pages down.</p>
    <div class="tabs" role="tablist">
      ${Object.entries(installs)
        .map(([id, i]) => `<button role="tab" data-install="${id}" class="${id === installTab ? "active" : ""}" aria-selected="${id === installTab}">${i.label}</button>`)
        .join("")}
    </div>
    <p class="hint">${esc(installs[installTab].hint)}</p>
    ${consentToggle()}
    <div class="copy-row"><pre class="snippet">${esc(installs[installTab].code)}</pre><button data-copy="${esc(installs[installTab].code)}">Copy</button></div>

    <h2>2. Define KPIs and events in code</h2>
    <p class="hint">Add a <code>trace.json</code> to your repository with the product name, the custom events your app sends and the KPIs you care about. Validate it with <code>trace check</code> and publish it with <code>trace push --server ${esc(location.origin)}</code>, ideally from CI on every merge. The <a href="#/kpis">KPIs</a> page shows the results and whether each event is arriving.</p>

    <h2>3. Track the moments that matter</h2>
    <div class="copy-row"><pre class="snippet">${esc(custom)}</pre><button data-copy="${esc(custom)}">Copy</button></div>

    <h2>4. Connect your coding agent (MCP)</h2>
    <p class="hint">Your coding agent can do everything on this page from your codebase: add the snippet, write and push trace.json, add privacy rules, then analyse the behaviour, save insights and mark them released when they ship. It sees only aggregated patterns, like this dashboard. In Claude Code, install the Trace plugin${pluginServerNote}:</p>
    <div class="copy-row"><pre class="snippet">${esc(pluginInstall)}</pre><button data-copy="${esc(pluginInstall)}">Copy</button></div>
    <p class="hint">Then run <code>/mcp</code>, choose <strong>plugin:trace:trace</strong> and <strong>Authenticate</strong>. Your browser opens Trace, where you approve it: no token to copy. The plugin adds <code>/trace:setup</code>, <code>/trace:analyse</code> and <code>/trace:fix</code>. Without the plugin, add the server alone:</p>
    <div class="copy-row"><pre class="snippet">${esc(mcpCommand)}</pre><button data-copy="${esc(mcpCommand)}">Copy</button></div>
    <p class="hint">Other MCP clients (Cursor and others) sign in the same way with:</p>
    <div class="copy-row"><pre class="snippet">${esc(mcpJson)}</pre><button data-copy="${esc(mcpJson)}">Copy</button></div>
    <p class="hint">Ask it to set things up:</p>
    <div class="copy-row"><pre class="snippet wrap">${esc(setupPrompt)}</pre><button data-copy="${esc(setupPrompt)}">Copy</button></div>
    <p class="hint">And to analyse: run <code>/trace:analyse</code> with the plugin, <code>/mcp__trace__analyse_website</code> without it, or pick the <strong>analyse_website</strong> prompt in other clients. Connected apps appear under <a href="#/account">Account</a>, where you can disconnect them. CI and scripts use a personal API token from there instead.</p>
    <h3>Get told when it's time to analyse</h3>
    <p class="hint">Trace says a website is worth analysing once it has 100 new sessions since the last analysis (30 after a week); the Insights page shows it too. To hear it from Claude Code each time you start it, add this to <code>~/.claude/settings.json</code> and put your token in the <code>TRACE_TOKEN</code> environment variable (in your shell profile, not in the file). It prints nothing until there's something to analyse.</p>
    <div class="copy-row"><pre class="snippet">${esc(analysisHook)}</pre><button data-copy="${esc(analysisHook)}">Copy</button></div>

    <h2>Public view</h2>
    <div id="public-card">${publicCard(pub)}</div>

    ${
      can("admin")
        ? `<h2>Remove this website</h2>
    <p class="hint">Deletes ${esc(project.name)} and everything collected and configured for it. This can't be undone.</p>
    <button class="quiet danger" id="remove-site">Remove ${esc(project.name)}…</button>`
        : ""
    }

    <h2>5. Analysis runs</h2>
    <p class="hint">${
      project.claude
        ? `Besides your agent, this server analyses with Claude (<code>${esc(project.claude.model)}</code>, effort <code>${esc(project.claude.effort)}</code>), falling back to the built-in checks if Claude is unavailable.`
        : `Your coding agent does the analysis (above). Trace's own runs are the built-in checks: rage clicks, dead clicks, errors and form drop-off.`
    }</p>
    <p class="hint">From the command line: <code>trace analyze</code>, <code>trace evaluate</code>, <code>trace prompt &lt;id&gt; | claude</code>.</p>
    ${table(
      [{ label: "Run" }, { label: "When" }, { label: "Engine" }, { label: "Insights", num: 1 }, { label: "Notes" }],
      runs.map((r) => [
        `#${r.id}`,
        new Date(r.started_at).toLocaleString(),
        esc(r.engine + (r.model ? ` · ${r.model}` : "")),
        num(r.insights_created),
        esc(r.error || ""),
      ]),
      "No analysis runs yet."
    )}`;
  view.querySelectorAll("[data-copy]").forEach((b) =>
    b.addEventListener("click", async () => {
      if (await copyText(b.dataset.copy)) toast("Copied");
    })
  );
  // `fresh`: the card was re-rendered, so its Copy button needs binding too.
  const bindPublic = (fresh) => {
    const card = view.querySelector("#public-card");
    const act = async (method, path, done) => {
      try {
        const r = await api(path, { method });
        card.innerHTML = publicCard(method === "DELETE" ? null : r);
        bindPublic(true);
        toast(done);
      } catch (e) {
        toast(e.message, true);
      }
    };
    card.querySelector("#public-on")?.addEventListener("click", () => act("POST", "/publish", "Public link created"));
    card.querySelector("#public-rotate")?.addEventListener("click", () => {
      if (confirm("Replace the link? The current one stops working at once.")) act("POST", "/publish/rotate", "New link created; the old one no longer works");
    });
    card.querySelector("#public-off")?.addEventListener("click", () => {
      if (confirm(`Make ${project.name} private? The link stops working at once.`)) act("DELETE", "/publish", "Public view switched off");
    });
    if (fresh) card.querySelectorAll("[data-copy]").forEach((b) => b.addEventListener("click", async () => (await copyText(b.dataset.copy)) && toast("Copied")));
  };
  bindPublic(false);

  view.querySelector("#remove-site")?.addEventListener("click", async () => {
    const typed = prompt(`Type the website's name, "${project.name}", to delete it and all its data:`);
    if (typed === null) return;
    try {
      await api(`/sites/${encodeURIComponent(project.slug)}`, { method: "DELETE", body: JSON.stringify({ confirm: typed }) });
      toast(`Removed ${project.name}`);
      project = null;
      await loadSites();
      route();
    } catch (e) {
      toast(e.message, true);
    }
  });
  view.querySelectorAll("[data-install]").forEach((b) =>
    b.addEventListener("click", () => {
      store("trace_install", b.dataset.install);
      renderSetup();
    })
  );
  bindConsentToggle(renderSetup);
  if (!project.collection.active) pollSetup();
}

function setupStatus() {
  const c = project.collection;
  if (c.active) {
    const sites = c.sources.map((s) => `<code>${esc(host(s.origin))}</code> (last event ${esc(ago(s.last_seen))})`).join(", ");
    return `<strong class="delta good">● Receiving data</strong>${sites ? ` from ${sites}` : ""}. You're set up: <a href="#/behaviour">see what's coming in</a> or <a href="#/insights">go to insights</a>. This page leaves the menu now; the status in the header brings you back here.`;
  }
  return c.last_event_at
    ? `<strong class="delta bad">● No data since ${esc(ago(c.last_event_at))}</strong>. Check that the snippet below is still on your pages.`
    : `<strong class="pulse">● Waiting for the first event…</strong> Add the snippet to your site and open any page. This updates by itself.`;
}

// While Setup is open and nothing has arrived yet, check every few seconds so the
// confirmation appears the moment the first event lands.
let setupPoll = null;
function pollSetup() {
  clearInterval(setupPoll);
  setupPoll = setInterval(async () => {
    if (!/^#\/(setup|start)/.test(location.hash)) return clearInterval(setupPoll);
    const was = project.collection.active;
    try {
      project = await api("/project");
    } catch {
      return;
    }
    showCollection();
    const box = document.getElementById("setup-status");
    if (box) box.innerHTML = setupStatus();
    if (!was && project.collection.active) {
      toast("Data is coming in. Trace is collecting.");
      if (location.hash.startsWith("#/start")) renderStart();
    }
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

// ---------------------------------------------------------------- get started

const START_STEPS = {
  website: "Your website",
  install: "Install the collector",
  privacy: "Check what's protected",
  kpis: "Define KPIs in code",
  agent: "Connect your coding agent",
  team: "Invite your team",
};

let checklist = null;

async function loadChecklist() {
  try {
    checklist = await api("/onboarding");
  } catch {
    checklist = null;
  }
  const link = document.querySelector('#nav a[data-view="start"]');
  link.hidden = !checklist || checklist.complete;
  if (checklist) document.getElementById("start-progress").textContent = `${checklist.done}/${checklist.total}`;
}

const tick = `<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="3" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M5 12l5 5L20 7"/></svg>`;

async function renderStart() {
  await loadChecklist();
  if (!checklist) throw new Error("couldn't load the checklist");
  const steps = checklist.steps;
  const asked = location.hash.split("/")[2];
  const firstOpen = steps.find((x) => !x.done && !x.optional) || steps.find((x) => !x.done) || steps[steps.length - 1];
  const current = steps.find((x) => x.key === asked) || firstOpen;
  const index = steps.indexOf(current);
  const next = steps[index + 1];
  const { installs, pluginInstall, pluginServerNote, setupPrompt } = setupSnippets();
  let installTab = store("trace_install") || "site";
  if (!installs[installTab]) installTab = "site";

  const content = {
    website: () => `
      <h1>${esc(project.name)}</h1>
      <p class="sub">Each website has its own snippet, KPIs, privacy rules and insights. Its short name is <code>${esc(project.slug)}</code>, used in commands like <code>trace insights --site ${esc(project.slug)}</code>.</p>
      ${can("admin") ? `<p class="hint">Monitoring more than one site? Add it from the website menu next to the logo.</p>` : ""}`,
    install: () => `
      <h1>Add one line to your site</h1>
      <p class="sub">Paste it into the page's &lt;head&gt;, on every page. It loads after your page, and if it ever fails your site carries on as if it weren't there.</p>
      <div class="card">
        <div class="tabs" role="tablist" aria-label="Kind of site">${Object.entries(installs)
          .map(([id, i]) => `<button role="tab" data-install="${id}" class="${id === installTab ? "active" : ""}" aria-selected="${id === installTab}">${i.label}</button>`)
          .join("")}</div>
        <p class="hint">${esc(installs[installTab].hint)}</p>
        ${consentToggle()}
        <pre class="snippet">${esc(installs[installTab].code)}</pre>
        <div class="actions">
          <button class="primary" data-copy="${esc(installs[installTab].code)}">Copy snippet</button>
          <button data-copy="${esc(installPrompt(installs[installTab].code))}">Copy as a prompt for my agent</button>
        </div>
      </div>
      <div class="card" id="setup-status" style="margin-top: 16px">${setupStatus()}</div>
      <div class="pair">
        <div class="card"><strong>What it records</strong><p class="hint">Pages, clicks, scroll, form starts and submits, errors. Counted per route, never per person.</p></div>
        <div class="card"><strong>What it never records</strong><p class="hint">Anything typed, page titles, query strings, the content of your pages.</p></div>
      </div>`,
    privacy: () => `
      <h1>Check what's protected</h1>
      <p class="sub">Trace looks for patterns, never people. These protections are always on. If your pages show anything else that identifies someone, such as order or member numbers, add a rule for it.</p>
      <div class="card"><ul class="checklist" id="built-in"><li>Loading…</li></ul></div>
      <div class="actions">
        ${
          can("admin")
            ? `<button class="primary" data-reviewed>I've reviewed this</button><a class="button" href="#/privacy">Add a rule</a>`
            : `<span class="hint">An admin of your account confirms this step.</span><a class="button" href="#/privacy">See the privacy page</a>`
        }
      </div>`,
    kpis: () => {
      const { push } = setupSnippets();
      return `
      <h1>Define KPIs in code</h1>
      <p class="sub">KPIs live in <code>trace.json</code> in your repository, next to the code that sends the events. The analyst puts them first, and released insights are judged against them.</p>
      <pre class="snippet">${esc(CONFIG_EXAMPLE)}</pre>
      <div class="actions"><button class="primary" data-copy="${esc(CONFIG_EXAMPLE)}">Copy example</button></div>
      <p class="hint">Check it and publish it, ideally from CI on every merge, with a personal API token from <a href="#/account">Account</a>:</p>
      <div class="copy-row"><pre class="snippet">${esc(push)}</pre><button data-copy="${esc(push)}">Copy</button></div>`;
    },
    agent: () => `
      <h1>Let your coding agent set it up</h1>
      <p class="sub">Connect Claude Code, Cursor or any MCP client once, and it does the rest from your codebase: it adds the snippet, writes your KPIs, keeps customer data out, and later analyses the behaviour and fixes what it finds. It sees only aggregated patterns, like this dashboard.</p>
      <ol class="steps">
        <li>In Claude Code, install the Trace plugin${pluginServerNote}:<div class="copy-row" style="margin-top: 8px"><pre class="snippet">${esc(pluginInstall)}</pre><button class="primary" data-copy="${esc(pluginInstall)}">Copy</button></div></li>
        <li>Run <code>/mcp</code>, choose <strong>plugin:trace:trace</strong>, then <strong>Authenticate</strong>. Your browser opens Trace: approve it there. No token to copy.</li>
        <li>Ask it to set things up:<div class="copy-row" style="margin-top: 8px"><pre class="snippet wrap">${esc(setupPrompt)}</pre><button data-copy="${esc(setupPrompt)}">Copy</button></div></li>
      </ol>
      <p class="hint">This step ticks off once your agent connects. Or run <code>/trace:setup</code> instead of the prompt. Other MCP clients sign in the same way with <code>${esc(location.origin)}/mcp</code> (see Setup). Rather do it by hand? The next steps show how.</p>`,
    team: () => `
      <h1>Invite your team</h1>
      <p class="sub">Optional. Users work with insights and analysis; admins also manage the team, websites, security and privacy rules. You can require two-step verification for everyone.</p>
      <div class="actions">${can("admin") ? `<a class="button primary" href="#/team">Invite someone</a>` : `<span class="hint">Ask an admin of your account to invite people.</span>`}</div>`,
  };

  view.innerHTML = `
    <div class="start">
      <nav class="start-steps" aria-label="Setup steps">${steps
        .map(
          (x, i) => `<a href="#/start/${x.key}" class="${x === current ? "current" : ""} ${x.done ? "done" : ""}" ${x === current ? 'aria-current="step"' : ""}>
            <span class="mark">${x.done ? tick : i + 1}</span><span>${esc(START_STEPS[x.key])}${x.optional ? ` <span class="hint">(optional)</span>` : ""}</span></a>`
        )
        .join("")}
        <p class="hint">${checklist.complete ? "All set. This checklist stays here, under Setup." : `${checklist.done} of ${checklist.total} done.`} <a href="#/insights">Skip to the dashboard</a></p>
      </nav>
      <section class="start-body">
        <span class="hint">Step ${index + 1} of ${steps.length}${current.done ? " · done" : ""}</span>
        ${content[current.key]()}
        <div class="start-foot">
          ${index > 0 ? `<a href="#/start/${steps[index - 1].key}">Back</a>` : "<span></span>"}
          ${next ? `<a class="button" href="#/start/${next.key}">Next: ${esc(START_STEPS[next.key].toLowerCase())}</a>` : `<a class="button" href="#/insights">Go to insights</a>`}
        </div>
      </section>
    </div>`;

  view.querySelectorAll("[data-copy]").forEach((b) =>
    b.addEventListener("click", async () => {
      if (await copyText(b.dataset.copy)) toast("Copied");
    })
  );
  view.querySelectorAll("[data-install]").forEach((b) =>
    b.addEventListener("click", () => {
      store("trace_install", b.dataset.install);
      renderStart();
    })
  );
  bindConsentToggle(renderStart);
  view.querySelector("[data-reviewed]")?.addEventListener("click", async () => {
    await post("/onboarding/privacy-reviewed");
    toast("Thanks. Privacy reviewed.");
    location.hash = next ? `#/start/${next.key}` : "#/insights";
  });
  if (current.key === "privacy") {
    const p = await api("/privacy");
    const list = view.querySelector("#built-in");
    if (list) list.innerHTML = p.built_in.map((b) => `<li>${esc(b)}</li>`).join("") + (p.rules.length ? `<li>${p.rules.length} rule${p.rules.length === 1 ? "" : "s"} of your own</li>` : "");
  }
  if (current.key === "install" && !project.collection.active) pollSetup();
}

// ---------------------------------------------------------------- shell

// ---------------------------------------------------------------- accounts

let auth = null;
const RANK = { user: 1, admin: 2, owner: 3 };
const can = (role) => Boolean(auth && auth.user && RANK[auth.user.role] >= RANK[role]);

function authScreen(html, { wide = false } = {}) {
  document.body.classList.add("auth");
  view.innerHTML = `<div class="auth-card card${wide ? " wide" : ""}">${html}</div>`;
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

const BRAND = `<div class="auth-brand"><svg class="logo" viewBox="2 15.5 60 33" aria-hidden="true"><rect x="3.5" y="25" width="14" height="14" rx="4.5" fill="#45c75b"/><rect x="13.5" y="21" width="22" height="22" rx="6" fill="#2a9ba3" class="ring"/><rect x="30.5" y="17" width="30" height="30" rx="8" fill="#4a6cff" class="ring"/></svg> Trace</div>`;
const PATTERNS_NOT_PEOPLE = `<p class="hint privacy-note">Trace looks at patterns, not people: no session recordings, nothing users type, and personal data is removed before it's stored.</p>`;

function renderLogin() {
  authScreen(`${BRAND}<h1>Sign in</h1>
    <form id="login" class="stack">
      <label class="field">Email <input name="email" type="email" autocomplete="username" required></label>
      <label class="field">Password <input name="password" type="password" autocomplete="current-password" required></label>
      <button class="primary" type="submit">Sign in</button>
    </form>
    <p class="hint">Forgot your password? Ask an admin to invite you again, or run <code>trace user reset-password</code> on the server.</p>
    ${auth && auth.allow_signup ? `<p>New here? <a href="#/signup" id="to-signup">Create an account</a></p>` : ""}`);
  view.querySelector("#to-signup")?.addEventListener("click", (e) => {
    e.preventDefault();
    renderSignup();
  });
  onSubmit(view.querySelector("#login"), async (fd) => {
    auth = await api("/auth/login", { method: "POST", body: JSON.stringify({ email: fd.get("email"), password: fd.get("password") }) });
    boot();
  });
}

function renderSignup() {
  authScreen(`${BRAND}<h1>Create an account</h1>
    <p>Free to try. You'll own the account and can add your team later.</p>
    <form id="signup" class="stack">
      <label class="field">Company or project <input name="account" autocomplete="organization" placeholder="Acme"></label>
      <label class="field">Name <input name="name" autocomplete="name"></label>
      <label class="field">Email <input name="email" type="email" autocomplete="username" required></label>
      <label class="field">Password <input name="password" type="password" autocomplete="new-password" minlength="10" required>
        <span class="hint">At least 10 characters.</span></label>
      ${legalLine()}
      <button class="primary" type="submit">Create account</button>
    </form>
    <p class="hint">Already have an account? <a href="#/" id="to-login">Sign in</a></p>${PATTERNS_NOT_PEOPLE}`);
  view.querySelector("#to-login").addEventListener("click", (e) => {
    e.preventDefault();
    renderLogin();
  });
  onSubmit(view.querySelector("#signup"), async (fd) => {
    auth = await api("/auth/signup", {
      method: "POST",
      body: JSON.stringify({ account: fd.get("account"), name: fd.get("name"), email: fd.get("email"), password: fd.get("password") }),
    });
    history.replaceState(null, "", "#/start");
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
  const text = `Trace recovery codes for ${auth.user.email}\nEach works once, instead of an authenticator code.\n\n${codes.join("\n")}\n`;
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
    a.download = "trace-recovery-codes.txt";
    a.click();
    URL.revokeObjectURL(a.href);
  });
  box.querySelector("[data-done]").addEventListener("click", done);
}

function renderEnroll() {
  authScreen(`${BRAND}<h1>Set up two-step verification</h1>
    <p>Your workspace requires it. You'll need an authenticator app on your phone.</p>
    <div id="enroll"></div>
    <p class="hint">${(auth.accounts || []).length > 1 ? `${auth.account ? `${esc(auth.account.name)} requires it. ` : ""}<a href="#" id="other-account">Use another account</a> · ` : ""}<a href="#" id="start-over">Sign out</a></p>`);
  view.querySelector("#start-over").addEventListener("click", (e) => {
    e.preventDefault();
    signOut();
  });
  view.querySelector("#other-account")?.addEventListener("click", async (e) => {
    e.preventDefault();
    const other = auth.accounts.find((a) => !auth.account || a.account_id !== auth.account.id);
    try {
      auth = await post("/account/switch", { account_id: other.account_id });
      document.body.classList.remove("auth");
      await enterAccount(auth);
    } catch (err) {
      toast(err.message, true);
    }
  });
  enrollmentFlow(view.querySelector("#enroll"), boot).catch((err) => toast(err.message, true));
}

// Where `trace serve` writes its log, for finding the first-account link.
const SETUP_LOGS = [
  { id: "docker", label: "Docker", how: "On the server, run:", code: `docker logs trace-web 2>&1 | grep "first account"`,
    note: "trace-web is the container's name in deploy/compose.yaml; use yours if it differs." },
  { id: "systemd", label: "systemd", how: "On the server, run:", code: `journalctl -u trace | grep "first account"`,
    note: "Use your service's name in place of trace." },
  { id: "terminal", label: "Terminal", how: "Look in the terminal where trace serve is running, for the line:",
    code: "No accounts yet. Create the first account here (this link works once): …", note: "" },
];
const CREATE_BY_HAND = {
  docker: `docker exec -it trace-web trace user create --email you@example.com --name "Your name"`,
  systemd: `trace user create --email you@example.com --name "Your name"`,
  terminal: `trace user create --email you@example.com --name "Your name"`,
};

/// "By creating an account you agree to…", when the operator publishes terms.
function legalLine() {
  if (!auth?.terms_url) return "";
  const link = (url, text) => `<a href="${esc(url)}" target="_blank" rel="noopener">${text}</a>`;
  const privacy = auth.privacy_url ? ` and that your data is handled as the ${link(auth.privacy_url, "privacy policy")} says` : "";
  return `<p class="hint">By creating an account you agree to the ${link(auth.terms_url, "terms of service")}${privacy}.</p>`;
}

function renderWelcome(code) {
  if (!code) {
    renderWelcomeSteps("docker");
    return;
  }
  authScreen(`${BRAND}<h1>Create your account</h1>
    <p>You'll own this account. You can add more websites and invite your team later, or keep it just for you.</p>
    <form id="welcome" class="stack">
      <label class="field">Company or project <input name="account" autocomplete="organization" placeholder="Acme"></label>
      <label class="field">Name <input name="name" autocomplete="name"></label>
      <label class="field">Email <input name="email" type="email" autocomplete="username" required></label>
      <label class="field">Password <input name="password" type="password" autocomplete="new-password" minlength="10" required>
        <span class="hint">At least 10 characters.</span></label>
      <button class="primary" type="submit">Create account</button>
    </form>${PATTERNS_NOT_PEOPLE}`);
  onSubmit(view.querySelector("#welcome"), async (fd) => {
    auth = await api("/auth/setup", {
      method: "POST",
      body: JSON.stringify({ code, account: fd.get("account"), name: fd.get("name"), email: fd.get("email"), password: fd.get("password") }),
    });
    history.replaceState(null, "", "#/start");
    boot();
  });
}

/// The first-run screen when the setup link's code isn't in the address:
/// how to find the link on the server, or paste it here.
function renderWelcomeSteps(method) {
  const m = SETUP_LOGS.find((x) => x.id === method) || SETUP_LOGS[0];
  authScreen(
    `${BRAND}<h1>Welcome to Trace</h1>
    <p>This server has no accounts yet. The first one is created with a one-time setup link, which only someone with access to the server can see. Three steps:</p>
    <ol class="welcome-steps">
      <li>
        <strong>Find the setup link in the server's log.</strong> How is Trace running?
        <div class="segmented" role="group" aria-label="How Trace runs">${SETUP_LOGS.map(
          (x) => `<button type="button" data-method="${x.id}" aria-pressed="${x.id === m.id}">${x.label}</button>`
        ).join("")}</div>
        <p>${esc(m.how)}</p>
        <div class="copy-row"><pre class="snippet">${esc(m.code)}</pre>${m.id === "terminal" ? "" : `<button data-copy="${esc(m.code)}">Copy</button>`}</div>
        ${m.note ? `<p class="hint">${esc(m.note)}</p>` : ""}
      </li>
      <li>
        <strong>Open the link in this browser</strong>, or paste it here:
        <form id="setup-code" class="paste-row">
          <label class="visually-hidden" for="setup-link">Setup link or code</label>
          <input id="setup-link" name="link" placeholder="${esc(location.origin)}/#/welcome/…" autocomplete="off" required>
          <button class="primary" type="submit">Continue</button>
        </form>
      </li>
      <li><strong>Create your account.</strong> You become its owner, and can add websites and invite your team afterwards.</li>
    </ol>
    <p class="hint">Nothing in the log? The link is printed each time the server starts while it has no accounts, and only the newest one works. Restart the server${m.id === "docker" ? " (<code>docker restart trace-web</code>)" : ""} and look again.</p>
    <details>
      <summary>Or create the account from the command line</summary>
      <div class="copy-row"><pre class="snippet">${esc(CREATE_BY_HAND[m.id])}</pre><button data-copy="${esc(CREATE_BY_HAND[m.id])}">Copy</button></div>
      <p class="hint">It prints a password once; sign in with it and change it under Account.</p>
    </details>`,
    { wide: true }
  );
  view.querySelectorAll("[data-method]").forEach((b) => b.addEventListener("click", () => renderWelcomeSteps(b.dataset.method)));
  view.querySelectorAll("[data-copy]").forEach((b) =>
    b.addEventListener("click", async () => {
      if (await copyText(b.dataset.copy)) toast("Copied");
    })
  );
  view.querySelector("#setup-code").addEventListener("submit", (e) => {
    e.preventDefault();
    const raw = new FormData(e.currentTarget).get("link").trim();
    const code = raw.includes("/welcome/") ? raw.split("/welcome/").pop() : raw;
    if (code) location.hash = `#/welcome/${encodeURIComponent(code)}`;
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
  if (invite.has_login) {
    // They join with their own session; the link must never set a password.
    store("trace_join", token);
    authScreen(`${BRAND}<h1>Join ${esc(invite.workspace)}</h1>
      <p>You've been invited as <strong>${esc(invite.role)}</strong>. <strong>${esc(invite.email)}</strong> already has a Trace login, so sign in with it to join. Your other accounts stay as they are.</p>
      <button class="primary" id="to-login">Sign in to join</button>`);
    view.querySelector("#to-login").addEventListener("click", () => {
      history.replaceState(null, "", "#/");
      renderLogin();
    });
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
    history.replaceState(null, "", "#/insights");
    boot();
  });
}

// Someone signed in opening an invite: join with this login, if it's the invited email.
async function renderJoin(token) {
  store("trace_join", null);
  history.replaceState(null, "", "#/");
  let invite;
  try {
    invite = await api(`/auth/invite/${encodeURIComponent(token)}`);
  } catch (e) {
    authScreen(`${BRAND}<h1>Invite not valid</h1><p>${esc(e.message)}</p><p><a href="#/" id="go-on">Continue to Trace</a></p>`);
    view.querySelector("#go-on").addEventListener("click", (e) => (e.preventDefault(), boot()));
    return;
  }
  if (invite.email !== auth.user.email) {
    authScreen(`${BRAND}<h1>This invite is for someone else</h1>
      <p>It was sent to <strong>${esc(invite.email)}</strong>, and you're signed in as <strong>${esc(auth.user.email)}</strong>. Sign out and sign in as ${esc(invite.email)} to join ${esc(invite.workspace)}.</p>
      <div class="actions"><button class="primary" id="sign-out">Sign out</button><button class="quiet" id="go-on">Not now</button></div>`);
    view.querySelector("#sign-out").addEventListener("click", () => {
      store("trace_join", token);
      signOut();
    });
    view.querySelector("#go-on").addEventListener("click", () => boot());
    return;
  }
  authScreen(`${BRAND}<h1>Join ${esc(invite.workspace)}</h1>
    <p>You've been invited as <strong>${esc(invite.role)}</strong>. You'll be able to switch between ${esc(invite.workspace)} and your other accounts from the website menu.</p>
    <div class="actions"><button class="primary" id="join">Join ${esc(invite.workspace)}</button><button class="quiet" id="go-on">Not now</button></div>`);
  view.querySelector("#go-on").addEventListener("click", () => boot());
  view.querySelector("#join").addEventListener("click", async (e) => {
    e.target.disabled = true;
    try {
      document.body.classList.remove("auth");
      await enterAccount(await post(`/account/join/${encodeURIComponent(token)}`));
      toast(`You've joined ${auth.account.name}`);
    } catch (err) {
      e.target.disabled = false;
      toast(err.message, true);
    }
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

// While an operator views as someone: say so on every page, with a way out.
function impersonationBanner() {
  const notice = document.getElementById("viewing-as");
  notice.hidden = !(auth && auth.impersonating);
  if (notice.hidden) return;
  notice.innerHTML = `<div><span>Viewing as <strong>${esc(auth.user.email)}</strong>, read-only.</span> <button type="button" id="stop-viewing">Stop viewing as them</button></div>`;
  notice.querySelector("#stop-viewing").addEventListener("click", async () => {
    try {
      await post("/auth/stop-impersonating");
      location.href = "/admin#/people";
    } catch (e) {
      toast(e.message, true);
    }
  });
}

function applyRole() {
  impersonationBanner();
  document.querySelector('#nav a[data-view="team"]').hidden = !can("admin");
  document.querySelector('#nav a[data-view="admin"]').hidden = !auth.user.is_operator;
  // Viewing as someone is read-only: no analysis runs from inside it.
  document.getElementById("run-analysis").hidden = !can("user") || Boolean(auth.impersonating);
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
    <p class="sub">${esc(u.email)} · ${esc(u.role)}${auth.account ? ` in ${esc(auth.account.name)}` : ""}</p>
    <h2>Accounts</h2>
    <p class="hint">One login, every account you belong to. Your role can differ in each.</p>
    ${table(
      [{ label: "Account" }, { label: "Your role" }, { label: "" }],
      (auth.accounts || []).map((m) => {
        const here = auth.account && m.account_id === auth.account.id;
        const action = here
          ? m.role !== "owner" && auth.accounts.length > 1
            ? `<button class="quiet" data-leave>Leave</button>`
            : `<span class="hint">Current</span>`
          : `<button class="quiet" data-switch-account="${m.account_id}">Switch</button>`;
        return [esc(m.name), esc(m.role), action];
      })
    )}
    ${auth.can_create_account ? `<p><button data-create-account>New account</button> <span class="hint">You'll own it, with this same login.</span></p>` : ""}
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
    <p class="hint">For your coding agent over MCP and for CI (<code>trace push --token</code>). A token can do what you can (you're ${esc(u.role)}), or only read if you tick read-only. Use one token per tool, so you can revoke them separately.</p>
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

    <h2>Appearance</h2>
    <div class="segmented" role="group" aria-label="Theme">${["system", "light", "dark"]
      .map((t) => `<button type="button" data-theme-choice="${t}" aria-pressed="${themeChoice() === t}">${t[0].toUpperCase() + t.slice(1)}</button>`)
      .join("")}</div>

    <h2>Session</h2>
    <button data-sign-out>Sign out</button>`;
  view.querySelectorAll("[data-theme-choice]").forEach((b) =>
    b.addEventListener("click", () => {
      setTheme(b.dataset.themeChoice);
      view.querySelectorAll("[data-theme-choice]").forEach((x) => x.setAttribute("aria-pressed", String(x === b)));
    })
  );
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
  view.querySelectorAll("[data-switch-account]").forEach((b) => b.addEventListener("click", () => switchAccount(b.dataset.switchAccount).catch((e) => toast(e.message, true))));
  view.querySelector("[data-create-account]")?.addEventListener("click", () => newAccount().catch((e) => toast(e.message, true)));
  view.querySelector("[data-leave]")?.addEventListener("click", async () => {
    if (!confirm(`Leave ${auth.account.name}? Your tokens for it stop working, and only an invite brings you back.`)) return;
    const name = auth.account.name;
    await enterAccount(await post("/account/leave"));
    toast(`You've left ${name}`);
  });
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
      : `<select data-role="${u.id}" aria-label="Role for ${esc(u.email)}">${["admin", "user"]
          .map((r) => `<option ${r === u.role ? "selected" : ""}>${r}</option>`)
          .join("")}</select>`;
  view.innerHTML = `
    <h1>Team</h1>
    <p class="sub">Who can use this workspace, and how they sign in.</p>

    <h2>Security</h2>
    <div class="card">
      <label class="check"><input type="checkbox" id="require-mfa" ${t.require_mfa ? "checked" : ""}>
        <span><strong>Require two-step verification for everyone</strong><br>
        <span class="hint">Anyone without it is asked to set up an authenticator app the next time they sign in.${auth.user.totp_enabled ? "" : " Turn it on for your own account first, under Account."}</span></span></label>
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
      <label class="field narrow">Role <select name="role"><option>user</option><option>admin</option></select></label>
      <button class="primary" type="submit">Create invite link</button>
    </form>
    <p class="hint">Users see every website and work with insights and analysis. Admins also manage the team, websites, security, privacy rules and everyone's API tokens. Trace doesn't send email: share the link yourself. It works once and expires in 7 days.</p>
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
    <p class="sub">Trace studies patterns, not people. These protections always apply. Add your own rules for anything specific to your site.</p>

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

// ---------------------------------------------------------------- plan & usage

function limitText(l) {
  if (l.limit == null) return "No limit";
  return l.key === "retention_days" ? `${num(l.limit)} days` : num(l.limit);
}

async function renderPlan() {
  const p = await api("/plan");
  const retention = p.limits.find((l) => l.key === "retention_days");
  const effectiveDays = [retention.limit, p.server_retention_days].filter((d) => d != null);
  const meters = p.limits
    .filter((l) => l.key !== "retention_days")
    .map((l) => {
      const share = l.limit ? Math.min(100, Math.round((l.used / l.limit) * 100)) : null;
      const full = l.limit != null && l.used >= l.limit;
      const note =
        l.key === "monthly_events" && p.events_dropped
          ? `${num(p.events_dropped)} not recorded since the limit`
          : l.key === "monthly_ai_runs"
            ? "Heuristic analysis is never limited"
            : l.key === "max_members"
              ? "Pending invites count too"
              : full
                ? "Ask for more to add another"
                : "";
      return `<div class="card meter-card ${full ? "at-limit" : ""}">
        <span class="label">${esc(l.label)}</span>
        <span class="figure">${num(l.used)} <small>${l.limit == null ? "· no limit" : `/ ${num(l.limit)}`}</small></span>
        ${share == null ? "" : `<div class="bar ${full ? "warn" : ""}" role="img" aria-label="${share}% used"><span style="width:${share}%"></span></div>`}
        ${note ? `<span class="hint">${esc(note)}</span>` : ""}
      </div>`;
    })
    .join("");
  const days = effectiveDays.length ? `${Math.min(...effectiveDays)} days` : "No limit";
  view.innerHTML = `
    <h1>Plan & usage</h1>
    <p class="sub">What this account can use, and how much of it has gone this month (${esc(p.month)}).</p>
    <div class="meters">${meters}</div>
    <div class="callout">
      <div class="card" style="flex: 1 1 420px"><strong style="font-size: 26px">${esc(days)}</strong> <span class="hint">of raw events kept. Older events are deleted every hour; insights and their evaluations stay.</span></div>
      ${
        p.contact_url
          ? `<div class="card" style="flex: 1 1 360px; display: flex; gap: 16px; align-items: center; justify-content: space-between; flex-wrap: wrap"><span>Need longer history, more websites or more events? Limits are set by hand, per account.</span><a class="button primary" href="${esc(p.contact_url)}">Contact</a></div>`
          : `<p class="hint">Need more? Ask the people who run this server.</p>`
      }
    </div>`;
}

// ---------------------------------------------------------------- Audience

let audienceDays = Number(store("trace_audience_days")) || 30;

// A change against the window before: rates in points, counts in percent.
function change(current, previous, { rate = false, up = true } = {}) {
  if (current == null || previous == null || (!rate && !previous)) return "";
  const d = rate ? current - previous : (current - previous) / previous;
  if (Math.abs(d) < (rate ? 0.0005 : 0.005)) return `<span class="delta">no change</span>`;
  const text = rate ? `${(Math.abs(d) * 100).toFixed(1)} pts` : `${Math.abs(d * 100).toFixed(0)}%`;
  return `<span class="delta ${(d > 0) === up ? "good" : "bad"}">${d > 0 ? "▲" : "▼"} ${text}</span>`;
}

const AUDIENCE_EXAMPLE = `"audience": {
  "marketing": ["/", "/pricing", "/blog/*"],
  "engaged": [{ "event": "signup" }, { "event": "login" }]
}`;

const IDENTIFY_EXAMPLE = `<script>trace.identify("<%= hmac(current_user.id) %>")</script>`;

function snippet(text) {
  return `<div class="copy-row"><pre class="snippet">${esc(text)}</pre><button data-copy="${esc(text)}">Copy</button></div>`;
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
      ${stat(num(v.visitors), "Visitors", change(v.visitors, v.previous_visitors), visitorsHint)}
      ${stat(num(Math.round(v.daily_average * 10) / 10), "A day on average")}
      ${stat(num(v.sessions), "Sessions", change(v.sessions, v.previous_sessions))}
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

// ---------------------------------------------------------------- operator area

// Decides between the sign-in screens and the app.
async function bootPublic() {
  document.body.classList.add("public");
  document.querySelectorAll("#nav a").forEach((a) => (a.hidden = !PUBLIC_PAGES.includes(a.dataset.view)));
  for (const id of ["run-analysis", "me", "site-menu"]) document.getElementById(id).hidden = true;
  try {
    await loadProject();
  } catch {
    view.innerHTML = `<div class="empty"><h1>This public link doesn't work</h1><p>It may have been replaced or withdrawn. Ask whoever shared it for a new one.</p></div>`;
    return;
  }
  document.getElementById("collect-status").removeAttribute("href");
  const notice = document.getElementById("notice");
  notice.hidden = false;
  notice.innerHTML = `<div><span><strong>A public, read-only view of ${esc(project.name)}.</strong> Trace studies patterns, not people: everything here is aggregated, and rows from fewer than ${project.suppressed_below} sessions are left out.</span> <a href="https://tracetells.com/">What is Trace?</a></div>`;
  route();
}

async function boot() {
  if (PUBLIC) return bootPublic();
  try {
    auth = await api("/auth/state");
  } catch (e) {
    view.innerHTML = `<div class="empty">Couldn't reach the Trace server: ${esc(e.message)}</div>`;
    return;
  }
  const hash = location.hash;
  if (auth.needs_setup) return renderWelcome(hash.startsWith("#/welcome/") ? hash.slice("#/welcome/".length) : "");
  if (!auth.user) {
    if (hash.startsWith("#/invite/")) return renderInvite(hash.slice("#/invite/".length));
    return hash === "#/signup" && auth.allow_signup ? renderSignup() : renderLogin();
  }
  if (auth.step === "verify") return renderVerify();
  if (auth.step === "enroll") return renderEnroll();
  const joining = hash.startsWith("#/invite/") ? hash.slice("#/invite/".length) : store("trace_join");
  if (joining) return renderJoin(joining);
  document.body.classList.remove("auth");
  if (/^#\/(welcome|invite)\//.test(hash) || hash === "#/signup") history.replaceState(null, "", "#/");
  applyRole();
  try {
    await loadSites();
  } catch (e) {
    view.innerHTML = `<div class="empty">Couldn't load your websites: ${esc(e.message)}</div>`;
    return;
  }
  route();
}

// An MCP client (Claude Code, Cursor, ...) sent the person here to sign in and
// approve it; approving sends them back to the client with a one-time code.
async function renderConnect() {
  const req = Object.fromEntries(new URLSearchParams(location.hash.split("?")[1] || ""));
  let info;
  try {
    info = await api(`/account/connect?client_id=${encodeURIComponent(req.client_id || "")}&redirect_uri=${encodeURIComponent(req.redirect_uri || "")}`);
  } catch (e) {
    view.innerHTML = `<div class="empty"><h1>This connection request isn't valid</h1><p>${esc(e.message)}</p></div>`;
    return;
  }
  const name = esc(info.client_name);
  const changes = can("admin")
    ? "save the insights it finds, mark them released, and set up tracking, KPIs and privacy rules"
    : "save the insights it finds and mark them released";
  // Two plain choices, full access already picked: a lone "Read only" box read as the only option.
  view.innerHTML = `<div class="connect">
    <h1>Connect ${name} to Trace</h1>
    ${info.accounts && info.accounts.length > 1
      ? `<p>${name} is asking to work with Trace as you. It works in one account; you can connect it again for another.</p>
         <label class="field">Account <select id="connect-account">${info.accounts
           .map((a) => `<option value="${a.account_id}"${a.account_id === info.account_id ? " selected" : ""}>${esc(a.name)} (${esc(a.role)})</option>`)
           .join("")}</select></label>
         <p>How much can it do?</p>`
      : `<p>${name} is asking to work with Trace as you${info.account ? ` in <strong>${esc(info.account)}</strong>` : ""}. How much can it do?</p>`}
    <fieldset class="access-choice">
      <legend class="visually-hidden">Access</legend>
      <label class="choice"><input type="radio" name="access" value="full" checked>
        <span><strong>Full access</strong> <span class="hint">(recommended)</span><br>
        <span class="hint">Look at your websites' analytics, answer your questions, ${changes}. It acts with your role in that account.</span></span></label>
      <label class="choice"><input type="radio" name="access" value="read">
        <span><strong>Read only</strong><br>
        <span class="hint">Look at your websites' analytics and answer your questions, but never save or change anything.</span></span></label>
    </fieldset>
    <p class="hint">Either way it sees only aggregated patterns, never individual visitors, and it can't manage people or tokens. You can disconnect it any time under Account.</p>
    <p class="hint">Only allow it if you started this from ${name} yourself. Afterwards you go back to <code>${esc(info.back_to)}</code>.</p>
    <div class="actions"><button class="primary" id="connect-allow">Allow</button><button class="quiet" id="connect-cancel">Cancel</button></div>
  </div>`;
  const answer = async (approve) => {
    try {
      const chosen = view.querySelector("#connect-account");
      const r = await post("/account/connect", {
        ...req,
        read_only: view.querySelector('input[name="access"]:checked').value === "read",
        account_id: chosen ? Number(chosen.value) : null,
        approve,
      });
      view.innerHTML = `<div class="empty"><h1>${approve ? `${name} is connected` : "Cancelled"}</h1><p>${approve ? `You can go back to ${name}.` : "Nothing was connected."}</p></div>`;
      location.href = r.redirect;
    } catch (e) {
      toast(e.message, true);
    }
  };
  view.querySelector("#connect-allow").addEventListener("click", () => answer(true));
  view.querySelector("#connect-cancel").addEventListener("click", () => answer(false));
}

const routes = {
  connect: renderConnect,
  home: renderHome,
  start: renderStart,
  insights: renderInsights,
  kpis: renderKpis,
  audience: renderAudience,
  behaviour: renderBehaviour,
  privacy: renderPrivacy,
  setup: renderSetup,
  account: renderAccount,
  team: renderTeam,
  plan: renderPlan,
};

function renderNoSites() {
  view.innerHTML = `<div class="empty"><h1>No websites yet</h1>
    <p>${can("admin") ? "Add the first website this account will monitor." : "Ask an admin of your account to add a website."}</p>
    ${can("admin") ? `<button class="primary" id="first-site">Add a website</button>` : ""}</div>`;
  view.querySelector("#first-site")?.addEventListener("click", async () => {
    try {
      if (await addWebsite()) switchSite(currentSite);
    } catch (e) {
      toast(e.message, true);
    }
  });
}

// Pages about the account rather than one website.
const ACCOUNT_PAGES = ["account", "team", "plan", "connect"];

async function route() {
  if (!PUBLIC && (!auth || !auth.user || auth.step)) return boot();
  const page = location.hash.replace(/^#\//, "");
  if (PUBLIC && !project) return;
  if (PUBLIC && !PUBLIC_PAGES.includes(page.split(/[/?]/)[0])) {
    history.replaceState(null, "", "#/insights");
    return route();
  }
  if (!PUBLIC && !sites.length && !ACCOUNT_PAGES.includes(page)) return renderNoSites();
  if (!project && !ACCOUNT_PAGES.includes(page)) {
    try {
      await loadProject();
    } catch (e) {
      view.innerHTML = `<div class="empty">Couldn't load this page: ${esc(e.message)}</div>`;
      return;
    }
  }
  // Land on Setup until data is arriving, on Insights after.
  // Every website opens on its home: what needs attention now, setup first while it's unfinished.
  const name = page.split(/[/?]/)[0] || (PUBLIC ? "insights" : "home");
  if (!location.hash || location.hash === "#/") history.replaceState(null, "", `#/${name}`);
  const render = routes[name] || renderInsights;
  if (!PUBLIC && project && !ACCOUNT_PAGES.includes(name) && name !== "start") loadChecklist();
  document.querySelectorAll("#nav a").forEach((a) => a.classList.toggle("active", a.dataset.view === name));
  clearInterval(analysisWatch);
  if (!PUBLIC && project && !ACCOUNT_PAGES.includes(name) && name !== "home" && name !== "insights") api("/analysis").then(analysisBadge, () => {});
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
      `${n} new insight${n === 1 ? "" : "s"} via ${r.engine}${r.duplicates ? `, ${r.duplicates} already known` : ""}.` +
        (r.warning ? ` ${r.warning}` : ""),
      Boolean(r.warning && !n)
    );
    insightTab = "new";
    if (location.hash === "#/insights" || !location.hash) route();
    else location.hash = "#/insights";
  } catch (err) {
    toast(`Analysis failed: ${err.message}`, true);
  } finally {
    btn.disabled = false;
    btn.textContent = "Run analysis";
    btn.classList.remove("ready");
  }
});

window.addEventListener("hashchange", route);
window.addEventListener("unhandledrejection", (e) => toast(e.reason?.message || String(e.reason), true));
boot();
