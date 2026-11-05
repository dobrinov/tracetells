"use strict";

// Problems: everything that goes wrong for visitors, by page. A table of pages
// by dead clicks, rage clicks and browser errors, shaded by how much; the chosen
// page's problems in detail, each a task for the coding agent; server errors beside.
// Server errors keep their own pages (errors.js): #/problems/server and #/problems/<id>.
// The dashboard is plain scripts loaded in order (see index.html); they share one scope.

let problemsDays = [7, 14, 30].includes(Number(store("trace_problems_days"))) ? Number(store("trace_problems_days")) : 7;
let problemsPage = null;

const PROBLEM_KINDS = { dead_click: "Dead click", rage_click: "Rage click", error: "Browser error" };

// One of five shades by size against the column's largest; lightness, not hue, carries it.
function heat(v, max) {
  if (!v) return 0;
  return Math.min(4, 1 + Math.floor(Math.sqrt(v / Math.max(1, max)) * 4));
}

// A task for the coding agent about one problem on one page, to paste as it is.
function pageProblemTask(path, p, days) {
  const what =
    p.kind === "error"
      ? `browser error (${p.error_kind || "exception"}): "${p.text}"`
      : `${PROBLEM_KINDS[p.kind].toLowerCase()}s on ${p.text ? `"${p.text}" ` : ""}(${p.selector || "the page"})`;
  return `Trace measured a problem on ${project.name} (Trace website "${project.slug}") over the last ${days} days: ${what} on ${path}, ${p.events} times in ${p.sessions} session${p.sessions === 1 ? "" : "s"}.

Find the cause in this codebase and fix it. Use the trace MCP tools: get_page_story for ${path} shows what people do around it. Then save an insight for it (draft_insights, then save_insights) with the share of sessions hitting this problem as evidence to decrease, and mark it released when the fix ships, so Trace can tell whether it worked.`;
}

function problemTabs(active) {
  const tab = (id, label, href) => html`<a role="tab" class="${active === id ? "active" : ""}" aria-selected="${active === id}" href="${href}">${label}</a>`;
  return html`<div class="tabs" role="tablist" aria-label="Problems">${tab("pages", "By page", pageUrl("errors"))}${tab("server", "Server errors", `${pageUrl("errors")}/server`)}</div>`;
}

function serverPart(r) {
  if (!r) return html`<p class="hint">Couldn't load server errors.</p>`;
  if (!r.key && !r.groups.length)
    return html`<div class="card problems-server-none"><p><strong>Server errors aren't reported yet.</strong> Add the tracetells client to your server (Rust, Node.js or Ruby) and the errors behind failed requests show here, with how many visits each broke.</p>${askAgent("/trace:setup server errors", "Set it up with your agent")}</div>`;
  const open = r.groups.filter((g) => g.status === "open");
  if (!open.length) return html`<p class="hint">No open server errors in this window.</p>`;
  return html`${table(
    [{ label: "Error" }, { label: "Where" }, { label: "Occurrences", num: 1 }, { label: "Visits broken", num: 1 }],
    open.slice(0, 5).map((g) => [
      html`<a href="${pageUrl("errors")}/${g.id}"><code>${g.class}</code></a><div class="hint">${g.message}</div>`,
      html`${g.routes.slice(0, 1).map((x) => html`<code>${routeLabel(x)}</code>`)}`,
      num(g.occurrences),
      num(g.impact.visits),
    ])
  )}${open.length > 5 ? html`<p><a href="${pageUrl("errors")}/server">All ${open.length} open server errors →</a></p>` : ""}`;
}

async function problemDetail(path, days) {
  const box = view.querySelector("#problem-detail");
  if (!box) return;
  box.innerHTML = html`<p class="hint">Loading ${path}…</p>`;
  let s;
  try {
    s = await api(`/pages/story?path=${encodeURIComponent(path)}&days=${days}`);
  } catch (e) {
    box.innerHTML = html`<p class="hint">Couldn't load this page's problems: ${e.message}</p>`;
    return;
  }
  if (problemsPage !== path) return;
  box.innerHTML = html`
    <div class="problem-detail-head">
      <h2><code>${path}</code></h2>
      <a href="${storyUrl(path)}">How people use this page →</a>
    </div>
    <ul class="problem-list">${s.problems.map(
      (p) => html`<li class="card">
        <span class="badge ${p.kind === "error" ? "bad" : "warn"}">${PROBLEM_KINDS[p.kind] || p.kind}</span>
        <div class="problem-text">${p.kind === "error" ? html`<strong>${p.text}</strong><span class="hint">${p.error_kind || "exception"}</span>` : html`<strong>${p.text || p.selector}</strong>${p.text ? html`<code>${p.selector}</code>` : ""}`}</div>
        <span class="problem-count">${num(p.events)} <span class="hint">in ${num(p.sessions)} session${p.sessions === 1 ? "" : "s"}</span></span>
        <button type="button" data-task="1" data-copy="${pageProblemTask(path, p, days)}" title="Copies a task for your coding agent">Make it a task</button>
      </li>`
    )}</ul>
    <h3>Insights about this page</h3>
    ${s.insights.length
      ? html`<ul class="story-insights">${s.insights.map((i) => html`<li><a href="${i.status === "released" ? pageUrl("releases") : pageUrl("insights")}">${i.title}</a> <span class="hint">${i.status.replace("_", " ")}</span></li>`)}</ul>`
      : html`<p class="hint">None names this page yet. Make one of these a task and your agent saves it as an insight.</p>`}`;
  wireCopy();
}

async function renderProblems() {
  if (PUBLIC || routeArg === "server" || /^\d+$/.test(routeArg || "")) return renderErrors();
  const days = problemsDays;
  const [r, server] = await Promise.all([api(`/problems?days=${days}`), api(`/errors?days=${days}`).catch(() => null)]);
  if (days !== problemsDays) return;
  const max = { dead: 0, rage: 0, errors: 0, sessions: 0 };
  for (const p of r.pages) {
    max.dead = Math.max(max.dead, p.dead_clicks);
    max.rage = Math.max(max.rage, p.rage_clicks);
    max.errors = Math.max(max.errors, p.errors);
    max.sessions = Math.max(max.sessions, p.sessions);
  }
  if (!r.pages.some((p) => p.path === problemsPage)) problemsPage = r.pages.length ? r.pages[0].path : null;
  const cell = (v, m) => html`<span class="heat heat-${heat(v, m)}">${v ? num(v) : "·"}</span>`;
  view.innerHTML = html`
    <div class="pulse-head">
      <div><h1>Problems</h1><p class="sub">Everything that went wrong for visitors, by page: clicks the page ignored, angry repeated clicks, and errors.</p></div>
      <div class="segmented" role="group" aria-label="Period">${[7, 14, 30].map((n) => html`<button type="button" data-days="${n}" aria-pressed="${n === days ? "true" : "false"}">${n} days</button>`)}</div>
    </div>
    ${problemTabs("pages")}
    ${r.pages.length
      ? html`
      <p class="hint">${num(r.pages.length)} page${r.pages.length === 1 ? "" : "s"} with problems, ${num(r.sessions)} session${r.sessions === 1 ? "" : "s"} hit. A stronger colour is more. Pick a page to see what happened.</p>
      <div class="heat-table">
        <div class="heat-row heat-headings" aria-hidden="true">
          <span>Page</span><span>Dead clicks</span><span>Rage clicks</span><span>Browser errors</span><span>Sessions hit</span>
        </div>
        ${r.pages.map(
          (p) => html`<button type="button" class="heat-row${p.path === problemsPage ? " on" : ""}" data-path="${p.path}" aria-pressed="${p.path === problemsPage ? "true" : "false"}"
            aria-label="${p.path}: ${p.dead_clicks} dead clicks, ${p.rage_clicks} rage clicks, ${p.errors} browser errors, ${p.sessions} sessions hit">
            <span class="heat-path"><code>${p.path}</code></span>
            ${cell(p.dead_clicks, max.dead)}${cell(p.rage_clicks, max.rage)}${cell(p.errors, max.errors)}${cell(p.sessions, max.sessions)}
          </button>`
        )}
      </div>
      <section class="card problem-detail" id="problem-detail" aria-live="polite"></section>`
      : html`<div class="card"><p><strong>Nothing went wrong in the last ${days} days.</strong> No dead clicks, rage clicks or browser errors.</p></div>`}
    <h2>Server errors</h2>
    ${serverPart(server)}`;
  view.querySelectorAll("[data-days]").forEach((b) =>
    b.addEventListener("click", () => {
      problemsDays = Number(b.dataset.days);
      store("trace_problems_days", String(problemsDays));
      renderProblems();
    })
  );
  view.querySelectorAll(".heat-row[data-path]").forEach((b) =>
    b.addEventListener("click", () => {
      problemsPage = b.dataset.path;
      view.querySelectorAll(".heat-row[data-path]").forEach((x) => {
        const on = x.dataset.path === problemsPage;
        x.classList.toggle("on", on);
        x.setAttribute("aria-pressed", on ? "true" : "false");
      });
      problemDetail(problemsPage, days);
    })
  );
  if (problemsPage) problemDetail(problemsPage, days);
  wireCopy();
}
