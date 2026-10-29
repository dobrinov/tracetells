"use strict";

// Privacy rules, and plan and usage.
// The dashboard is plain scripts loaded in order (see index.html); they share one scope.

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
