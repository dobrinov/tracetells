// Trace admin: the server as a whole, for its operators. Separate from the
// dashboard on purpose: no website menu, nothing about one account's analytics.
// Seeing those means viewing as one of the account's people, which is
// read-only, ends after an hour, and is recorded in that account's activity log.

const view = document.getElementById("view");
const toastEl = document.getElementById("toast");
let toastTimer;

const esc = (s) =>
  String(s ?? "").replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]);
const num = (v) => (v == null ? "–" : Number(v).toLocaleString());
const date = (ts) => (ts ? new Date(ts).toLocaleDateString() : "–");

function toast(message, isError = false) {
  toastEl.textContent = message;
  toastEl.className = "toast" + (isError ? " error" : "");
  toastEl.hidden = false;
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => (toastEl.hidden = true), isError ? 8000 : 4000);
}

function ago(ts) {
  if (!ts) return "never";
  const m = Math.round((Date.now() - ts) / 60000);
  if (m < 1) return "just now";
  if (m < 60) return `${m} min ago`;
  const h = Math.round(m / 60);
  return h < 48 ? `${h} h ago` : `${Math.round(h / 24)} days ago`;
}

async function api(path, opts = {}) {
  const res = await fetch(`/api${path}`, { ...opts, headers: { "Content-Type": "application/json", ...(opts.headers || {}) } });
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
    return false;
  }
}

function table(headers, rows, empty = "Nothing here yet.") {
  if (!rows.length) return `<p class="hint">${empty}</p>`;
  return `<div class="table-wrap"><table><thead><tr>${headers
    .map((h) => `<th class="${h.num ? "num" : ""}">${h.label}</th>`)
    .join("")}</tr></thead><tbody>${rows
    .map((r) => `<tr>${r.map((c, i) => `<td class="${headers[i].num ? "num" : ""}">${c}</td>`).join("")}</tr>`)
    .join("")}</tbody></table></div>`;
}

function onSubmit(form, action) {
  form.addEventListener("submit", async (e) => {
    e.preventDefault();
    const btn = form.querySelector("button[type=submit]");
    if (btn) btn.disabled = true;
    try {
      await action(new FormData(form));
    } catch (err) {
      toast(err.message, true);
    } finally {
      if (btn) btn.disabled = false;
    }
  });
}

const LIMIT_KEYS = ["retention_days", "max_sites", "monthly_events", "max_members"];
const limitInput = (name, value, placeholder = "no limit") =>
  `<input name="${name}" type="number" min="0" step="1" value="${value == null ? "" : value}" placeholder="${placeholder}">`;

function stat(value, label, note = "") {
  return `<div class="card stat"><div class="value">${value}</div><div class="label">${label}</div>${note ? `<div class="hint">${note}</div>` : ""}</div>`;
}

// ---------------------------------------------------------------- Overview

async function renderOverview() {
  const [s, o] = await Promise.all([api("/operator/stats"), api("/operator/overview")]);
  const max = Math.max(1, ...s.sessions_by_day.map((d) => d.sessions));
  const busiest = [...o.accounts].sort((a, b) => b.usage.events - a.usage.events).slice(0, 5);
  view.innerHTML = `
    <h1>Overview</h1>
    <p class="sub">The whole server. Numbers are counts across accounts; nothing here shows one account's analytics.</p>
    ${o.database === "sqlite" ? `<div class="card"><strong>This server keeps its data in SQLite.</strong> <span class="hint">Fine for trying Trace; for production, use Postgres (<code>trace init --database postgres</code>).</span></div>` : ""}
    <div class="stats">
      ${stat(num(s.accounts), "Accounts", `${num(s.accounts_new_7d)} new in 7 days`)}
      ${stat(num(s.people), "People", `${num(s.people_active_7d)} signed in, ${num(s.people_new_7d)} new in 7 days`)}
      ${stat(num(s.websites), "Websites", `${num(s.websites_collecting)} collecting now`)}
      ${stat(num(s.sessions_7d), "Sessions", "last 7 days")}
      ${stat(num(s.events_month), "Events", `${esc(s.month)}${s.events_dropped_month ? `, ${num(s.events_dropped_month)} over limits` : ""}`)}
    </div>
    <h2>Sessions per day</h2>
    <div class="card day-bars">${
      s.sessions_by_day.length
        ? s.sessions_by_day
            .map((d) => `<div class="day-bar" title="${esc(d.day)}: ${num(d.sessions)} sessions"><span style="height:${Math.max(2, (d.sessions / max) * 100).toFixed(1)}%"></span><small>${esc(d.day.slice(5))}</small></div>`)
            .join("")
        : `<p class="hint">No sessions in the last 14 days.</p>`
    }</div>
    <h2>Busiest accounts this month</h2>
    ${table(
      [{ label: "Account" }, { label: "Plan" }, { label: "Events", num: 1 }, { label: "Websites", num: 1 }, { label: "Members", num: 1 }],
      busiest.map((a) => [`<a href="#/accounts/${a.id}">${esc(a.name)}</a>`, esc(a.plan), num(a.usage.events), num(a.sites), num(a.members)])
    )}`;
}

// ---------------------------------------------------------------- Accounts

async function renderAccounts(id) {
  const o = await api("/operator/overview");
  const usageCell = (a) => {
    const ev = a.limits.monthly_events;
    return `${num(a.usage.events)}${ev == null ? "" : ` / ${num(ev)}`}${a.usage.events_dropped ? ` <span class="delta bad">${num(a.usage.events_dropped)} dropped</span>` : ""}`;
  };
  view.innerHTML = `
    <h1>Accounts</h1>
    <p class="sub">Plans, limits and usage. Customers never see plan names.</p>
    ${table(
      [{ label: "Account" }, { label: "Plan" }, { label: "Websites", num: 1 }, { label: "Members", num: 1 }, { label: "Events this month" }, { label: "Created" }],
      o.accounts.map((a) => [
        `<a href="#/accounts/${a.id}">${esc(a.name)}</a>`,
        `${esc(a.plan)}${Object.keys(a.overrides).length ? ` <span class="chip">+${Object.keys(a.overrides).length} override${Object.keys(a.overrides).length > 1 ? "s" : ""}</span>` : ""}`,
        num(a.sites),
        num(a.members),
        usageCell(a),
        date(a.created_at),
      ])
    )}
    <div id="account-detail"></div>
    <h2>Create an account</h2>
    <form class="card row" id="new-account">
      <label class="field">Name <input name="name" required placeholder="Bolt Bikes"></label>
      <label class="field">Owner's email <input name="owner_email" type="email" required></label>
      <label class="field narrow">Plan <select name="plan">${o.plans.map((p) => `<option ${p.is_default ? "selected" : ""}>${esc(p.key)}</option>`).join("")}</select></label>
      <button class="primary" type="submit">Create</button>
    </form>
    <div id="new-account-result"></div>`;
  onSubmit(view.querySelector("#new-account"), async (fd) => {
    const r = await post("/operator/accounts", { name: fd.get("name"), owner_email: fd.get("owner_email"), plan: fd.get("plan") });
    const link = `${location.origin}/#/invite/${r.invite_token}`;
    await renderAccounts();
    view.querySelector("#new-account-result").innerHTML = `<div class="card"><p>Send this link to <strong>${esc(r.owner_email)}</strong>: it makes them the account's owner. It works once and expires in 7 days.${r.has_login ? " They already have a login, so they'll sign in with it to accept." : ""}</p>
      <div class="copy-row"><pre class="snippet">${esc(link)}</pre><button data-copy="${esc(link)}">Copy</button></div></div>`;
    view.querySelector("#new-account-result [data-copy]").addEventListener("click", async (e) => (await copyText(e.target.dataset.copy)) && toast("Copied"));
  });
  if (id) await showAccount(Number(id), o);
}

async function showAccount(id, o) {
  const d = await api(`/operator/accounts/${id}`);
  const a = d.account;
  const plan = o.plans.find((p) => p.key === a.plan);
  const labels = Object.fromEntries(o.limits);
  const box = view.querySelector("#account-detail");
  box.innerHTML = `
    <div class="card">
      <h2>${esc(a.name)}</h2>
      <h3>People</h3>
      ${table(
        [{ label: "Person" }, { label: "Role" }, { label: "Two-step" }, { label: "Last sign-in" }, { label: "" }],
        d.people.map((u) => [
          `${esc(u.name || u.email)}<br><span class="hint">${esc(u.email)}</span>${u.is_operator ? ` <span class="chip">operator</span>` : ""}`,
          esc(u.role),
          u.totp_enabled ? "on" : "off",
          esc(ago(u.last_login_at)),
          u.is_operator || u.id == null ? "" : `<button class="quiet" data-view-as="${u.id}" data-account="${a.id}" data-email="${esc(u.email)}">View as</button>`,
        ]),
        "Nobody yet."
      )}
      ${d.invites.length ? `<p class="hint">Pending invites: ${d.invites.map((i) => `${esc(i.email)} (${esc(i.role)})`).join(", ")}</p>` : ""}
      <h3>Websites</h3>
      ${table(
        [{ label: "Website" }, { label: "Collecting from" }, { label: "Status" }],
        d.sites.map((s) => [`${esc(s.name)} <code>${esc(s.slug)}</code>`, esc(s.origins.join(", ") || "–"), s.collecting ? `<span class="delta good">● collecting</span>` : `<span class="hint">last event ${esc(ago(s.last_event_at))}</span>`]),
        "No websites."
      )}
      <h3>Plan and limits</h3>
      <form id="account-plan" class="stack">
        <label class="field narrow">Plan <select name="plan">${o.plans.map((p) => `<option ${p.key === a.plan ? "selected" : ""}>${esc(p.key)}</option>`).join("")}</select></label>
        <div class="table-wrap"><table><thead><tr><th>Limit</th><th>From the plan</th><th>For this account</th><th>Used this month</th></tr></thead><tbody>
        ${LIMIT_KEYS.map((k) => {
          const has = Object.prototype.hasOwnProperty.call(a.overrides, k);
          const mode = !has ? "inherit" : a.overrides[k] == null ? "none" : "custom";
          const used = { monthly_events: a.usage.events, max_sites: a.sites, max_members: a.members }[k];
          return `<tr data-limit="${k}"><td>${esc(labels[k])}</td><td>${plan.limits[k] == null ? "no limit" : num(plan.limits[k])}</td>
            <td><select name="mode" aria-label="${esc(labels[k])}"><option value="inherit" ${mode === "inherit" ? "selected" : ""}>Same as plan</option><option value="none" ${mode === "none" ? "selected" : ""}>No limit</option><option value="custom" ${mode === "custom" ? "selected" : ""}>Custom</option></select>
              ${limitInput("value", has ? a.overrides[k] : null, "")}</td><td>${used == null ? "–" : num(used)}</td></tr>`;
        }).join("")}
        </tbody></table></div>
        <div><button type="submit">Save plan and limits</button></div>
      </form>
    </div>`;
  bindViewAs(box);
  box.scrollIntoView({ behavior: "smooth", block: "start" });
  onSubmit(box.querySelector("#account-plan"), async (fd) => {
    if (fd.get("plan") !== a.plan) await post(`/operator/accounts/${id}/plan`, { plan: fd.get("plan") });
    const overrides = {};
    box.querySelectorAll("[data-limit]").forEach((row) => {
      const mode = row.querySelector("[name=mode]").value;
      const value = row.querySelector("[name=value]").value;
      if (mode === "none") overrides[row.dataset.limit] = null;
      if (mode === "custom" && value !== "") overrides[row.dataset.limit] = Number(value);
    });
    await post(`/operator/accounts/${id}/overrides`, { overrides });
    toast("Saved");
    showAccount(id, await api("/operator/overview"));
  });
}

// ---------------------------------------------------------------- People

async function renderPeople() {
  const people = await api("/operator/people");
  view.innerHTML = `
    <h1>People</h1>
    <p class="sub">Everyone on the server and the accounts they belong to. <strong>View as</strong> opens the dashboard as that person, read-only, for up to an hour; it's recorded in your log and in theirs.</p>
    <label class="field narrow"><input id="filter" type="search" placeholder="Filter by email, name or account" aria-label="Filter"></label>
    <div id="people-table"></div>`;
  const draw = (q) => {
    const rows = people.filter((p) => !q || `${p.email} ${p.name} ${p.accounts.map((a) => a.account).join(" ")}`.toLowerCase().includes(q));
    view.querySelector("#people-table").innerHTML = table(
      [{ label: "Person" }, { label: "Accounts" }, { label: "Two-step" }, { label: "Joined" }, { label: "Last sign-in" }, { label: "" }],
      rows.map((p) => [
        `${esc(p.name || p.email)}<br><span class="hint">${esc(p.email)}</span>${p.is_operator ? ` <span class="chip">operator</span>` : ""}`,
        p.accounts.map((a) => `<a href="#/accounts/${a.account_id}">${esc(a.account)}</a> <span class="hint">${esc(a.role)}</span>`).join("<br>"),
        p.totp_enabled ? "on" : "off",
        date(p.created_at),
        esc(ago(p.last_login_at)),
        p.is_operator ? "" : `<button class="quiet" data-view-as="${p.id}" data-email="${esc(p.email)}">View as</button>`,
      ]),
      "Nobody matches."
    );
    bindViewAs(view);
  };
  draw("");
  view.querySelector("#filter").addEventListener("input", (e) => draw(e.target.value.trim().toLowerCase()));
}

function bindViewAs(root) {
  root.querySelectorAll("[data-view-as]").forEach((b) =>
    b.addEventListener("click", async () => {
      if (!confirm(`View the dashboard as ${b.dataset.email}? It's read-only, ends after an hour, and is recorded in their account's activity log.`)) return;
      try {
        await post("/operator/impersonate", { user_id: Number(b.dataset.viewAs), account_id: b.dataset.account ? Number(b.dataset.account) : null });
        location.href = "/#/home";
      } catch (e) {
        toast(e.message, true);
      }
    })
  );
}

// ---------------------------------------------------------------- Websites

async function renderWebsites() {
  const sites = await api("/operator/sites");
  view.innerHTML = `
    <h1>Websites</h1>
    <p class="sub">Every website on the server, its account, and how much it's used.</p>
    ${table(
      [{ label: "Website" }, { label: "Account" }, { label: "Status" }, { label: "Sessions, 7 days", num: 1 }, { label: "Events, 30 days", num: 1 }, { label: "Added" }],
      sites.map((s) => [
        `${esc(s.name)} <code>${esc(s.slug || "")}</code>`,
        `<a href="#/accounts/${s.account_id}">${esc(s.account)}</a>`,
        s.collecting ? `<span class="delta good">● collecting</span>` : `<span class="hint">last event ${esc(ago(s.last_event_at))}</span>`,
        num(s.sessions_7d),
        num(s.events_30d),
        date(s.created_at),
      ])
    )}`;
}

// ---------------------------------------------------------------- Plans

async function renderPlans() {
  const o = await api("/operator/overview");
  const labels = Object.fromEntries(o.limits);
  view.innerHTML = `
    <h1>Plans</h1>
    <p class="sub">Changing a template changes every account on it, except limits an account overrides. Empty means no limit.</p>
    <div class="table-wrap"><table><thead><tr><th>Plan</th>${LIMIT_KEYS.map((k) => `<th>${esc(labels[k])}</th>`).join("")}<th>Default</th><th></th></tr></thead><tbody>
      ${o.plans
        .map(
          (p) => `<tr data-plan="${esc(p.key)}"><td><input name="name" value="${esc(p.name)}" aria-label="Name"><br><code>${esc(p.key)}</code></td>
            ${LIMIT_KEYS.map((k) => `<td>${limitInput(k, p.limits[k])}</td>`).join("")}
            <td><input type="radio" name="default-plan" value="${esc(p.key)}" ${p.is_default ? "checked" : ""} aria-label="Default plan"></td>
            <td><button data-save-plan="${esc(p.key)}">Save</button></td></tr>`
        )
        .join("")}
    </tbody></table></div>
    <p class="hint">New accounts (sign-ups and accounts created without a plan) get the default plan.</p>`;
  view.querySelectorAll("[data-save-plan]").forEach((b) =>
    b.addEventListener("click", async () => {
      const row = b.closest("tr");
      const limits = Object.fromEntries(LIMIT_KEYS.map((k) => [k, row.querySelector(`[name=${k}]`).value === "" ? null : Number(row.querySelector(`[name=${k}]`).value)]));
      await api(`/operator/plans/${b.dataset.savePlan}`, { method: "PUT", body: JSON.stringify({ name: row.querySelector("[name=name]").value, limits }) });
      toast(`Saved ${b.dataset.savePlan}`);
    })
  );
  view.querySelectorAll("[name=default-plan]").forEach((r) =>
    r.addEventListener("change", async () => {
      await post("/operator/plans/default", { key: r.value });
      toast(`New accounts now get ${r.value}`);
    })
  );
}

// ---------------------------------------------------------------- Routing

const routes = { overview: renderOverview, accounts: renderAccounts, people: renderPeople, websites: renderWebsites, plans: renderPlans };

async function route() {
  const [name, arg] = location.hash.replace(/^#\//, "").split("/");
  const page = routes[name] ? name : "overview";
  if (!routes[name]) history.replaceState(null, "", "#/overview");
  document.querySelectorAll("#nav a").forEach((a) => a.classList.toggle("active", a.dataset.view === page));
  try {
    await routes[page](arg);
  } catch (e) {
    view.innerHTML = `<div class="empty"><p>${esc(e.message)}</p></div>`;
  }
}

async function boot() {
  let auth;
  try {
    auth = await api("/auth/state");
  } catch (e) {
    view.innerHTML = `<div class="empty">Couldn't reach the Trace server: ${esc(e.message)}</div>`;
    return;
  }
  const stop = (html) => (view.innerHTML = `<div class="empty">${html}</div>`);
  if (!auth.user || auth.step) return stop(`<h1>Sign in first</h1><p><a href="/">Sign in to Trace</a>, then come back to <code>/admin</code>.</p>`);
  if (auth.impersonating) return stop(`<h1>You're viewing as ${esc(auth.user.email)}</h1><p><a href="/">Stop viewing as them</a> in the dashboard first.</p>`);
  if (!auth.user.is_operator) return stop(`<h1>This is for the server's operators</h1><p>Operator rights are granted on the server with <code>trace operator grant</code>.</p>`);
  if (!auth.user.totp_enabled) return stop(`<h1>Turn on two-step verification</h1><p>The admin area needs it. <a href="/#/account">Set it up under Account</a>.</p>`);
  document.getElementById("nav").hidden = false;
  window.addEventListener("hashchange", route);
  route();
}

boot();
