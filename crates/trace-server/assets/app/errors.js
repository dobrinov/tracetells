"use strict";

// Errors: what breaks in visitors' browsers (from the behaviour summary) and on
// the website's servers (sent by the tracetells clients). #/understand/errors
// is the browser, /server the server groups, /<id> one server group.

let serverEnv = store("trace_error_env") || "";

const SERVER_STATUS = { open: ["Open", "warn"], resolved: ["Resolved", "good"], ignored: ["Ignored", ""] };
const VERDICT = { stopped: ["Stopped", "good"], dropped: ["Dropped", "good"], still_happening: ["Still happening", "warn"], waiting: ["Waiting", ""] };

function errorTabs(active) {
  if (PUBLIC) return "";
  const tab = (id, label, href) => html`<a role="tab" class="${active === id ? "active" : ""}" aria-selected="${active === id}" href="${href}">${label}</a>`;
  return html`<div class="tabs" role="tablist" aria-label="Where errors happen">${tab("browser", "Browser", pageUrl("errors"))}${tab("server", "Server", `${pageUrl("errors")}/server`)}</div>`;
}

function errorsToolbar(sub, extra = "") {
  return html`
    <div class="toolbar">
      <div><h1>Errors</h1><p class="sub">${sub}</p></div>
      <div class="toolbar-fields">${raw(extra)}
        <label class="field narrow">Window
          <select id="days">${[1, 7, 14, 30, 90].map((n) => html`<option value="${n}" ${raw(n === digestDays ? "selected" : "")}>Last ${n} day${n > 1 ? "s" : ""}</option>`)}</select>
        </label>
      </div>
    </div>`;
}

function onDaysChange(rerender) {
  view.querySelector("#days").addEventListener("change", (e) => {
    digestDays = Number(e.target.value);
    store("trace_days", String(digestDays));
    rerender();
  });
}

async function renderErrors() {
  if (!PUBLIC && routeArg === "server") return renderServerErrors();
  if (!PUBLIC && /^\d+$/.test(routeArg || "")) return renderServerError(Number(routeArg));
  const d = await api(`/digest?days=${digestDays}`);
  const total = d.errors.reduce((n, e) => n + e.sessions, 0);
  view.innerHTML = html`
    ${errorsToolbar("Exceptions, console errors, failed requests and missing files that visitors hit, with the page and how many sessions each one reached.")}
    ${errorTabs("browser")}
    ${d.errors.length ? html`<div class="stats">${raw(stat(num(d.errors.length), "Different errors"))}${raw(stat(num(total), "Sessions hit", `of ${num(d.totals.sessions)}`))}</div>` : ""}
    ${table(
      [{ label: "Kind" }, { label: "Message" }, { label: "Page" }, { label: "Source" }, { label: "Count", num: 1 }, { label: "Sessions", num: 1 }],
      d.errors.map((e) => [html`<span class="chip">${e.kind}</span>`, html`${e.message}`, html`${e.path}`, e.source ? html`<code>${e.source}</code>` : "", num(e.count), num(e.sessions)]),
      "No errors: no exceptions, console errors, failed requests or missing files."
    )}`;
  onDaysChange(renderErrors);
}

function serverBadge(g) {
  const [label, tone] = SERVER_STATUS[g.status] || [g.status, ""];
  const badges = [html`<span class="badge ${tone}">${label}</span>`];
  if (g.regressed_in_release && g.status === "open") badges.push(html` <span class="badge warn">Back in ${g.regressed_in_release}</span>`);
  else if (g.new && g.status === "open") badges.push(html` <span class="badge">New${g.first_release ? ` in ${g.first_release}` : ""}</span>`);
  if (g.verdict) {
    const [v, vt] = VERDICT[g.verdict.outcome] || [g.verdict.outcome, ""];
    badges.push(html` <span class="badge ${vt}" title="${g.verdict.summary}">${v}</span>`);
  }
  return badges;
}

const routeLabel = (r) => `${r.method || ""} ${r.route || "(no route)"}${r.status ? ` → ${r.status}` : ""}`.trim();

function envPicker(r) {
  if (r.environments.length < 2) return "";
  return html`<label class="field narrow">Environment
    <select id="env">${r.environments.map((e) => html`<option value="${e}" ${raw(e === r.environment ? "selected" : "")}>${e || "(none sent)"}</option>`)}</select>
  </label>`;
}

async function renderServerErrors() {
  const r = await api(`/errors?days=${digestDays}${serverEnv ? `&environment=${encodeURIComponent(serverEnv)}` : ""}`);
  const sub = "Exceptions on the website's servers, grouped and counted, never individual requests. Ranked by how many visits saw the same request fail in the browser, then by how often.";
  if (!r.key && !r.groups.length) {
    view.innerHTML = html`
      ${errorsToolbar(sub)}
      ${errorTabs("server")}
      <div class="empty">
        <h2>See the errors behind failed requests</h2>
        <p>Add the tracetells client to your server (Rust, Node.js or Ruby) and Trace shows what fails there: the exception, where in your code, on which routes, since which release, and how many visits it broke. It never sends request bodies, parameters, headers, cookies or who was signed in.</p>
        ${askAgent("/trace:setup server errors", "Set it up with your agent")}
      </div>`;
    onDaysChange(renderServerErrors);
    return;
  }
  const occurrences = r.groups.reduce((n, g) => n + g.occurrences, 0);
  const broken = r.groups.reduce((n, g) => n + g.impact.visits, 0);
  view.innerHTML = html`
    ${errorsToolbar(sub, envPicker(r))}
    ${errorTabs("server")}
    ${r.key ? "" : html`<p class="hint">The server key was revoked, so new errors aren't arriving. ${askAgent("/trace:manage create a server key", "New key")}</p>`}
    <div class="stats">${raw(stat(num(r.groups.length), "Error groups"))}${raw(stat(num(occurrences), "Occurrences"))}${raw(stat(num(broken), "Visits broken", "the same request failed in the browser"))}</div>
    ${table(
      [{ label: "Error" }, { label: "Where" }, { label: "Occurrences", num: 1 }, { label: "Visits broken", num: 1 }, { label: "Releases" }, { label: "Status" }],
      r.groups.map((g) => [
        html`<a href="${pageUrl("errors")}/${g.id}"><code>${g.class}</code></a><div class="hint">${g.message}</div>`,
        html`${g.routes.slice(0, 2).map((x) => html`<div><code>${routeLabel(x)}</code></div>`)}`,
        html`${num(g.occurrences)}<div class="hint">before: ${num(g.previous)}</div>`,
        num(g.impact.visits),
        html`${g.first_release || "–"}${g.last_release && g.last_release !== g.first_release ? html` → ${g.last_release}` : ""}`,
        html`${serverBadge(g)}`,
      ]),
      "No server errors in this window."
    )}
    ${r.groups.length ? html`<p class="hint">Ignored groups keep counting but leave this list. Once a fix is live, tell your agent which release it shipped in:</p>${askAgent(`/trace:manage resolve server error ${r.groups[0].id} in <release>`, "e.g.")}` : ""}`;
  onDaysChange(renderServerErrors);
  const env = view.querySelector("#env");
  if (env)
    env.addEventListener("change", (e) => {
      serverEnv = e.target.value;
      store("trace_error_env", serverEnv);
      renderServerErrors();
    });
}

async function renderServerError(id) {
  let d;
  try {
    d = await api(`/errors/${id}?days=${digestDays}${serverEnv ? `&environment=${encodeURIComponent(serverEnv)}` : ""}`);
  } catch {
    view.innerHTML = html`<div class="empty"><h1>No server error ${id}</h1><p><a href="${pageUrl("errors")}/server">Back to server errors</a></p></div>`;
    return;
  }
  const peak = Math.max(1, ...d.daily.map(([, n]) => n));
  view.innerHTML = html`
    <p><a href="${pageUrl("errors")}/server">← Server errors</a></p>
    <div class="toolbar"><div><h1><code>${d.class}</code></h1><p class="sub">${d.message}</p></div><div>${serverBadge(d)}</div></div>
    <div class="stats">
      ${raw(stat(num(d.occurrences), `Occurrences, last ${d.daily.length} days`, `before: ${num(d.previous)}`))}
      ${raw(stat(num(d.impact.visits), "Visits broken", d.impact.pages.length ? `on ${d.impact.pages.map(([p]) => esc(p)).join(", ")}` : "no matching failures in browsers"))}
      ${raw(stat(esc(d.first_release || "–"), "First seen in", esc(new Date(d.first_seen).toLocaleDateString())))}
      ${raw(stat(esc(ago(d.last_seen)), "Last seen", esc(d.last_release || "")))}
    </div>
    ${d.verdict ? html`<div class="card"><strong>Since it was resolved${d.resolved_in_release ? ` in ${d.resolved_in_release}` : ""}:</strong> ${d.verdict.summary}</div>` : ""}
    ${d.impact.goals.length ? html`<p class="hint">The pages where it breaks visits feed: ${d.impact.goals.join(", ")}.</p>` : ""}
    <h2>Per day</h2>
    <div class="daybars" role="img" aria-label="Occurrences per day">${d.daily.map(([day, n]) => html`<span title="${day}: ${num(n)}" style="height:${Math.max(2, Math.round((n / peak) * 48))}px"></span>`)}</div>
    <h2>Where</h2>
    ${table(
      [{ label: "Route" }, { label: "Occurrences", num: 1 }],
      d.routes.map((x) => [html`<code>${routeLabel(x)}</code>`, num(x.count)])
    )}
    ${d.releases.length ? html`<h2>By release</h2>${table([{ label: "Release" }, { label: "Occurrences", num: 1 }], d.releases.map(([r, n]) => [html`<code>${r}</code>`, num(n)]))}` : ""}
    <h2>Backtrace</h2>
    <p class="hint">The latest occurrence${d.sample.runtime ? `, on ${d.sample.runtime}` : ""}${d.sample.environment ? ` in ${d.sample.environment}` : ""}. Dependencies are dimmed.</p>
    <pre class="backtrace">${d.sample.frames.map((f) => html`<span class="${f.in_app ? "" : "muted"}">${f.file}${f.line ? `:${f.line}` : ""} in ${f.function}</span>\n`)}</pre>
    ${askAgent(`/trace:fix server error ${id}`, "Fix it with your agent")}`;
}
