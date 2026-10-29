"use strict";

// Pages and Errors: the behaviour summary.
// The dashboard is plain scripts loaded in order (see index.html); they share one scope.

// ---------------------------------------------------------------- Behaviour

let digestDays = Number(store("trace_days")) || 14;

// Cells and labels are markup: html`` results, or strings the caller escaped (older pages).
function table(headers, rows, empty = "No data in this window.") {
  const cell = (c) => (c instanceof SafeHtml ? c : raw(c));
  if (!rows.length) return html`<p class="hint">${cell(empty)}</p>`;
  return html`<div class="table-wrap"><table><thead><tr>${headers.map((h) => html`<th class="${h.num ? "num" : ""}">${cell(h.label)}</th>`)}</tr></thead><tbody>${rows.map(
    (r) => html`<tr>${r.map((c, i) => html`<td class="${headers[i].num ? "num" : ""}">${cell(c)}</td>`)}</tr>`
  )}</tbody></table></div>`;
}

async function renderBehaviour() {
  const d = await api(`/digest?days=${digestDays}`);
  const t = d.totals;
  const clickRows = (list) => list.map((c) => [esc(c.path), `<code>${esc(c.selector)}</code>`, esc(c.text || ""), num(c.count), num(c.sessions)]);
  const clickHeaders = [{ label: "Page" }, { label: "Element" }, { label: "Text" }, { label: "Count", num: 1 }, { label: "Sessions", num: 1 }];
  view.innerHTML = `
    <div class="toolbar">
      <div><h1>Pages</h1><p class="sub">How each page is used, aggregated over the window, never individual recordings. Errors have their own page.</p></div>
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

    <h2>Forms</h2>
    ${table(
      [{ label: "Page" }, { label: "Form" }, { label: "Started", num: 1 }, { label: "Submitted", num: 1 }, { label: "Completion", num: 1 }],
      d.forms.map((f) => [esc(f.path), `<code>${esc(f.selector)}</code>`, num(f.started), num(f.submitted), pct(f.completion_rate)])
    )}

    <h2>Top paths</h2>
    ${table(
      [{ label: "From" }, { label: "To" }, { label: "Count", num: 1 }],
      d.transitions.map((x) => [esc(x.from), esc(x.to), num(x.count)])
    )}`;
  view.querySelector("#days").addEventListener("change", (e) => {
    digestDays = Number(e.target.value);
    store("trace_days", String(digestDays));
    renderBehaviour();
  });
}

async function renderErrors() {
  const d = await api(`/digest?days=${digestDays}`);
  const total = d.errors.reduce((n, e) => n + e.sessions, 0);
  view.innerHTML = `
    <div class="toolbar">
      <div><h1>Errors</h1><p class="sub">Exceptions, console errors, failed requests and missing files that visitors hit, with the page and how many sessions each one reached.</p></div>
      <label class="field narrow">Window
        <select id="days">${[1, 7, 14, 30, 90].map((n) => `<option value="${n}" ${n === digestDays ? "selected" : ""}>Last ${n} day${n > 1 ? "s" : ""}</option>`).join("")}</select>
      </label>
    </div>
    ${d.errors.length ? `<div class="stats">${stat(num(d.errors.length), "Different errors")}${stat(num(total), "Sessions hit", `of ${num(d.totals.sessions)}`)}</div>` : ""}
    ${table(
      [{ label: "Kind" }, { label: "Message" }, { label: "Page" }, { label: "Source" }, { label: "Count", num: 1 }, { label: "Sessions", num: 1 }],
      d.errors.map((e) => [`<span class="chip">${esc(e.kind)}</span>`, esc(e.message), esc(e.path), e.source ? `<code>${esc(e.source)}</code>` : "", num(e.count), num(e.sessions)]),
      "No errors: no exceptions, console errors, failed requests or missing files."
    )}`;
  view.querySelector("#days").addEventListener("change", (e) => {
    digestDays = Number(e.target.value);
    store("trace_days", String(digestDays));
    renderErrors();
  });
}
