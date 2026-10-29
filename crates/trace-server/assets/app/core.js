"use strict";

// Shared state, helpers, the API client, navigation between areas and the website menu.
// The dashboard is plain scripts loaded in order (see index.html); they share one scope.

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
// Markup that escapes by default: html`<p>${name}</p>` escapes `name`; only
// another html`` result, raw() or a list of them goes in as markup. A result
// works anywhere a string does, so older string templates can embed it.
class SafeHtml {
  constructor(s) {
    this.s = s;
  }
  toString() {
    return this.s;
  }
}
const raw = (s) => new SafeHtml(String(s ?? ""));
function markup(v) {
  if (v instanceof SafeHtml) return v.s;
  if (Array.isArray(v)) return v.map(markup).join("");
  if (v == null || v === false) return "";
  return esc(v);
}
function html(strings, ...values) {
  let out = strings[0];
  values.forEach((v, i) => (out += markup(v) + strings[i + 1]));
  return new SafeHtml(out);
}

// The dashboard shows; your coding agent changes things, over MCP. Wherever
// there's something to do, the page says exactly what to ask it.
function askAgent(command, label = "Ask your agent") {
  return html`<div class="ask-agent"><span class="ask-label">${label}</span><code>${command}</code><button type="button" data-ask="${command}">Copy</button></div>`;
}
document.addEventListener("click", async (e) => {
  const b = e.target.closest && e.target.closest("[data-ask]");
  if (!b) return;
  if (await copyText(b.dataset.ask)) {
    toast("Copied. Paste it into Claude Code, or ask any MCP client the same.");
    measure("copied", { what: "ask" });
  }
});

const pct = (v) => (v == null ? "–" : `${(v * 100).toFixed(1)}%`);
const num = (v) => (v == null ? "–" : Number(v).toLocaleString());

// When this server measures its own dashboard (TRACE_DASHBOARD_KEY), the page
// carries Trace's collector. Pages are named by area and page, never by an
// id, website or area key; the content area is masked in index.html.
function measure(event, props) {
  try {
    if (window.trace && typeof window.trace.track === "function") window.trace.track(event, props);
  } catch {}
}
function measurePage(name) {
  try {
    if (window.trace && typeof window.trace.page === "function") window.trace.page(name);
  } catch {}
}

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
// Four areas in the order of the job: see, understand, change, configure. Each
// page has one home; the URL carries the area (#/understand/funnels), and the
// old flat links (#/kpis, #/behaviour) still resolve.
const SECTIONS = {
  home: { label: "Home", pages: [["home", "Home"]] },
  understand: { label: "Understand", pages: [["audience", "Audience"], ["features", "Features"], ["funnels", "Funnels"], ["pages", "Pages"], ["errors", "Errors"]] },
  improve: { label: "Improve", pages: [["goals", "Goals"], ["insights", "Insights"], ["releases", "Releases"]] },
  setup: { label: "Setup", pages: [["start", "Get started"], ["install", "Install and tracking"], ["privacy", "Privacy"], ["team", "Team"], ["plan", "Plan"]] },
};
const ALIASES = { kpis: "goals", behaviour: "pages", setup: "install" };
const PUBLIC_PAGES = ["goals", "insights", "releases", "funnels", "pages", "errors"];
const sectionOf = (page) => Object.keys(SECTIONS).find((s) => SECTIONS[s].pages.some(([p]) => p === page));
const pageUrl = (page) => `#/${sectionOf(page) && sectionOf(page) !== "home" ? `${sectionOf(page)}/` : ""}${page}`;

// The page a hash names, and anything after it (an area's key, a setup step).
function resolve(hash) {
  const parts = hash.replace(/^#\//, "").split("?")[0].split("/").filter(Boolean);
  if (!parts.length) return { page: null, arg: null };
  if (SECTIONS[parts[0]] && parts[0] !== "home") {
    const pages = SECTIONS[parts[0]].pages.map(([p]) => p).filter((p) => !PUBLIC || PUBLIC_PAGES.includes(p));
    const page = pages.includes(parts[1]) ? parts[1] : parts[0] === "setup" && parts[1] === "start" ? "start" : pages.find((p) => p !== "start" || !checklist?.complete) || pages[0];
    return { page, arg: parts[1] === page ? parts[2] || null : null };
  }
  const page = ALIASES[parts[0]] || parts[0];
  return { page, arg: parts[1] || null };
}
let routeArg = null;

// The pages of the current area, under the header.
function renderSubnav(page) {
  const nav = document.getElementById("subnav");
  const section = sectionOf(page);
  document.querySelectorAll("#nav a[data-section]").forEach((a) => {
    const on = a.dataset.section === section;
    a.classList.toggle("active", on);
    if (on) a.setAttribute("aria-current", "page");
    else a.removeAttribute("aria-current");
  });
  if (!section || section === "home") return (nav.hidden = true);
  const pages = SECTIONS[section].pages.filter(([p]) => {
    if (PUBLIC) return PUBLIC_PAGES.includes(p);
    if (p === "team") return can("admin");
    if (p === "start") return !checklist || !checklist.complete;
    return true;
  });
  nav.hidden = false;
  nav.innerHTML = `<div>${pages
    .map(([p, label]) => `<a href="#/${section}/${p}" class="${p === page ? "active" : ""}"${p === page ? ' aria-current="page"' : ""}>${esc(label)}${p === "start" && checklist ? ` <span class="progress">${checklist.done}/${checklist.total}</span>` : ""}</a>`)
    .join("")}</div>`;
}

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

// Whether data is arriving right now, from the time of the last event: live,
// quiet, waiting for the first visit, or stopped. The pill links to the snippet.
function showCollection() {
  const c = project.collection;
  const pill = document.getElementById("collect-status");
  pill.hidden = false;
  const since = c.last_event_at ? Date.now() - c.last_event_at : null;
  const state = since == null ? "waiting" : since < 15 * 60000 ? "live" : since < 2 * 86400000 ? "quiet" : "stopped";
  document.getElementById("site-dot").classList.toggle("on", state === "live" || state === "quiet");
  const sites = c.sources.filter((s) => Date.now() - s.last_seen < 86400000).map((s) => host(s.origin));
  pill.className = `collect-status ${state}`;
  pill.textContent = {
    live: `Live · last event ${ago(c.last_event_at)}`,
    quiet: `Quiet · last event ${ago(c.last_event_at)}`,
    waiting: "Waiting for the first visit",
    stopped: `Stopped · nothing for ${ago(c.last_event_at).replace(" ago", "")}`,
  }[state];
  pill.title = sites.length ? `Collecting from ${sites.join(", ")}. Open install and tracking.` : "Open install and tracking";
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

// Websites are added by your coding agent (/trace:manage add a website called …);
// this stays only for an account that has none left.
async function addWebsite() {
  const name = prompt("Name of the new website, e.g. \"Docs\" or \"Shop (staging)\":");
  if (!name) return false;
  const s = await post("/sites", { name });
  measure("website_added");
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
    const command = "/trace:manage add a website called ";
    if (await copyText(command)) toast(`Copied "${command}…". Finish the name and paste it into your coding agent: it adds the website and its tag.`);
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
