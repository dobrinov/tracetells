"use strict";

// Starting up, the public view, connecting MCP clients, and routing. Loads last.
// The dashboard is plain scripts loaded in order (see index.html); they share one scope.

// ---------------------------------------------------------------- operator area

// Decides between the sign-in screens and the app.
async function bootPublic() {
  document.body.classList.add("public");
  document.querySelectorAll("#nav a").forEach((a) => (a.hidden = !["understand", "improve"].includes(a.dataset.section)));
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
  audience: renderAudience,
  features: renderFeatures,
  funnels: renderFunnels,
  pages: renderBehaviour,
  errors: renderErrors,
  goals: renderKpis,
  insights: () => ((insightsMode = "insights"), renderInsights()),
  releases: () => ((insightsMode = "releases"), renderInsights()),
  install: renderInstall,
  privacy: renderPrivacy,
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
  if (PUBLIC && !project) return;
  let { page, arg } = resolve(location.hash);
  page = page || (PUBLIC ? "insights" : "home");
  if (PUBLIC && !PUBLIC_PAGES.includes(page)) page = "insights";
  if (!PUBLIC && !sites.length && !ACCOUNT_PAGES.includes(page)) return renderNoSites();
  if (!project && !ACCOUNT_PAGES.includes(page)) {
    try {
      await loadProject();
    } catch (e) {
      view.innerHTML = `<div class="empty">Couldn't load this page: ${esc(e.message)}</div>`;
      return;
    }
  }
  // Every website opens on its home. Old and bare links settle on the page's own address.
  const canonical = `${pageUrl(page)}${arg ? `/${arg}` : ""}`;
  if (!ACCOUNT_PAGES.includes(page) && location.hash.split("?")[0] !== canonical) history.replaceState(null, "", canonical + (location.hash.includes("?") ? location.hash.slice(location.hash.indexOf("?")) : ""));
  routeArg = arg;
  measurePage(ACCOUNT_PAGES.includes(page) || !sectionOf(page) ? `/${page}` : sectionOf(page) === "home" ? "/home" : `/${sectionOf(page)}/${page}${page === "features" && arg ? "/area" : ""}`);
  const render = routes[page] || renderHome;
  if (!PUBLIC && project && !ACCOUNT_PAGES.includes(page)) await loadChecklist();
  renderSubnav(ACCOUNT_PAGES.includes(page) && page !== "team" && page !== "plan" ? null : page);
  clearInterval(analysisWatch);
  if (!PUBLIC && project && !ACCOUNT_PAGES.includes(page) && page !== "home" && page !== "insights") api("/analysis").then(analysisBadge, () => {});
  try {
    await render();
  } catch (e) {
    view.innerHTML = `<div class="empty">Couldn't load this page: ${esc(e.message)}</div>`;
  }
}

// The header's "Analyse · N new": analysis runs in your agent, so this copies the command.
document.getElementById("run-analysis").addEventListener("click", async () => {
  if (await copyText(ANALYSE_COMMAND)) {
    toast(`Copied ${ANALYSE_COMMAND}. Paste it into Claude Code; the insights it saves show up here.`);
    measure("copied", { what: "analyse" });
  }
});

window.addEventListener("hashchange", route);
window.addEventListener("unhandledrejection", (e) => toast(e.reason?.message || String(e.reason), true));
boot();
