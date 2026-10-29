"use strict";

// Setup › Install and tracking, and Get started.
// The dashboard is plain scripts loaded in order (see index.html); they share one scope.

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
  if (!pub || !pub.public) return `<p class="hint">Off. ${what}</p>${can("admin") ? askAgent("/trace:manage publish the public link", "To share it") : ""}`;
  return `<p class="hint">On since ${esc(new Date(pub.since).toLocaleDateString())}. ${what}</p>
    <div class="copy-row"><pre class="snippet">${esc(pub.url)}</pre><button data-copy="${esc(pub.url)}">Copy</button></div>
    ${can("admin") ? `${askAgent("/trace:manage replace the public link", "A new link")}${askAgent("/trace:manage withdraw the public link", "Make it private")}` : ""}`;
}

// The things people ask their agent for, in one place: the dashboard shows,
// the agent does.
const AGENT_JOBS = [
  ["Set Trace up in this codebase: the tag, trace.json, masks", "/trace:setup"],
  ["Find what to fix", "/trace:analyse"],
  ["Look at one part of the product", "/trace:analyse checkout"],
  ["Implement an insight", "/trace:fix 12"],
  ["Mark it released once it's live", "/trace:manage release insight 12"],
  ["Dismiss one, with why", "/trace:manage dismiss insight 4: we're redesigning that page"],
  ["Keep something out of what's collected", "/trace:manage mask .account-summary"],
  ["Confirm you've reviewed privacy", "/trace:manage confirm the privacy review"],
  ["Share a read-only dashboard", "/trace:manage publish the public link"],
  ["Add another website", "/trace:manage add a website called docs"],
  ["Apply an edited trace.json", "/trace:manage push trace.json"],
  ["Check that data and events arrive", "/trace:manage is Trace receiving data?"],
];

async function renderSetup() {
  const pub = await api("/publish").catch(() => null);
  const { installs, mcpCommand, mcpJson, pluginInstall, pluginServerNote, custom, analysisHook } = setupSnippets();
  let installTab = store("trace_install") || "site";
  if (!installs[installTab]) installTab = "site";
  view.innerHTML = `
    <h1>Install and tracking</h1>
    <p class="sub">This dashboard is for looking. Setting Trace up, analysing, fixing and every change happen in your coding agent, over MCP: Claude Code, Cursor or any MCP client, with the model you choose. New here? <a href="#/setup/start">Follow the setup steps</a>.</p>
    <div class="card setup-status" id="setup-status">${setupStatus()}</div>

    <h2>1. Connect your coding agent</h2>
    <p class="hint">In Claude Code, install the Trace plugin${pluginServerNote}:</p>
    <div class="copy-row"><pre class="snippet">${esc(pluginInstall)}</pre><button data-copy="${esc(pluginInstall)}">Copy</button></div>
    <p class="hint">Then run <code>/mcp</code>, choose <strong>plugin:trace:trace</strong> and <strong>Authenticate</strong>. Your browser opens Trace, where you approve it: no token to copy. Without the plugin, add the server alone:</p>
    <div class="copy-row"><pre class="snippet">${esc(mcpCommand)}</pre><button data-copy="${esc(mcpCommand)}">Copy</button></div>
    <p class="hint">Other MCP clients sign in the same way with:</p>
    <div class="copy-row"><pre class="snippet">${esc(mcpJson)}</pre><button data-copy="${esc(mcpJson)}">Copy</button></div>
    <p class="hint">Connected apps appear under <a href="#/account">Account</a>, where you can disconnect them. Your agent sees only aggregated patterns, like this dashboard.</p>

    <h2>2. What to ask it</h2>
    <p class="hint">With the plugin, these are commands; any MCP client understands the same requests in plain words.</p>
    ${table(
      [{ label: "To" }, { label: "Ask your agent" }, { label: "" }],
      AGENT_JOBS.map(([what, command]) => [esc(what), `<code>${esc(command)}</code>`, html`<button type="button" class="quiet" data-ask="${command}">Copy</button>`])
    )}

    <h2>3. The tag</h2>
    <p class="hint"><code>/trace:setup</code> adds it for you. By hand: it records pageviews, clicks, rage and dead clicks, errors, scroll depth and form starts and submits, never what people type. Add <code>data-trace-mask</code> to any element whose text is private.</p>
    <div class="tabs" role="tablist">
      ${Object.entries(installs)
        .map(([id, i]) => `<button role="tab" data-install="${id}" class="${id === installTab ? "active" : ""}" aria-selected="${id === installTab}">${i.label}</button>`)
        .join("")}
    </div>
    <p class="hint">${esc(installs[installTab].hint)}</p>
    ${consentToggle()}
    <div class="copy-row"><pre class="snippet">${esc(installs[installTab].code)}</pre><button data-copy="${esc(installs[installTab].code)}">Copy</button></div>

    <h2>4. Goals, events, funnels and areas</h2>
    <p class="hint">They live in <code>trace.json</code> in your repository. <code>/trace:setup</code> writes it from your code; after editing, <code>/trace:manage push trace.json</code> applies it, or <code>trace push</code> from CI. Custom events are sent like this:</p>
    <div class="copy-row"><pre class="snippet">${esc(custom)}</pre><button data-copy="${esc(custom)}">Copy</button></div>

    <h2>5. Hear when it's time to analyse</h2>
    <p class="hint">Trace says a website is worth analysing once it has 100 new sessions since the last analysis (30 after a week); Home shows it too. To hear it from Claude Code each time you start it, add this to <code>~/.claude/settings.json</code> and put a token from Account in the <code>TRACE_TOKEN</code> environment variable. It prints nothing until there's something to analyse.</p>
    <div class="copy-row"><pre class="snippet">${esc(analysisHook)}</pre><button data-copy="${esc(analysisHook)}">Copy</button></div>

    <h2>Public view</h2>
    <div id="public-card">${publicCard(pub)}</div>

    ${
      can("admin")
        ? `<h2>Remove this website</h2>
    <p class="hint">Deletes ${esc(project.name)} and everything collected and configured for it. This can't be undone.</p>
    <button class="quiet danger" id="remove-site">Remove ${esc(project.name)}…</button>`
        : ""
    }`;
  view.querySelectorAll("[data-copy]").forEach((b) =>
    b.addEventListener("click", async () => {
      if (await copyText(b.dataset.copy)) toast("Copied");
    })
  );

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
  data: "See your first visit",
  goals: "Tell Trace what success means",
  insight: "Get your first insight",
  agent: "Connect your coding agent",
  privacy: "Check what's protected",
  team: "Invite your team",
};

let checklist = null;

async function loadChecklist() {
  try {
    checklist = await api("/onboarding");
  } catch {
    checklist = null;
  }
  const progress = document.getElementById("start-progress");
  progress.hidden = !checklist || checklist.complete;
  if (checklist) progress.textContent = `${checklist.done}/${checklist.total}`;
}

const tick = `<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="3" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M5 12l5 5L20 7"/></svg>`;

async function renderStart() {
  await loadChecklist();
  if (!checklist) throw new Error("couldn't load the checklist");
  const steps = checklist.steps;
  const asked = routeArg;
  const firstOpen = steps.find((x) => !x.done && !x.optional) || steps.find((x) => !x.done) || steps[steps.length - 1];
  const current = steps.find((x) => x.key === asked) || firstOpen;
  const index = steps.indexOf(current);
  const next = steps[index + 1];
  const { installs, pluginInstall, pluginServerNote, setupPrompt } = setupSnippets();
  let installTab = store("trace_install") || "site";
  if (!installs[installTab]) installTab = "site";

  const content = {
    data: () => `
      <h1>See your first visit</h1>
      <p class="sub">Add one line to the page's &lt;head&gt;, on every page, then open your site. This step is done when the first visit arrives, not when the tag is saved. If the script ever fails, your site carries on as if it weren't there.</p>
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
        <div class="card"><strong>What it never records</strong><p class="hint">Anything typed, page titles, query strings, the content of your pages. <a href="#/setup/start/privacy">Review what's protected</a></p></div>
      </div>`,
    privacy: () => `
      <h1>Check what's protected</h1>
      <p class="sub">Trace looks for patterns, never people. These protections are always on. If your pages show anything else that identifies someone, such as order or member numbers, add a rule for it.</p>
      <div class="card"><ul class="checklist" id="built-in"><li>Loading…</li></ul></div>
      <div class="actions">
        ${
          can("admin")
            ? `${askAgent("/trace:manage confirm the privacy review", "Reviewed it?")}${askAgent("/trace:manage mask .account-summary", "Something to keep out?")}`
            : `<span class="hint">An admin of your account confirms this step, through their coding agent.</span>`
        }
      </div>`,
    goals: () => {
      const { push } = setupSnippets();
      return `
      <h1>Tell Trace what success means</h1>
      <p class="sub">Goals (KPIs), the events behind them, funnels and your product's areas live in <code>trace.json</code> in your repository, next to the code that sends the events. The analyst puts the goals first, and released insights are judged against them. This step is done when the file is applied.</p>
      <div class="pair">
        <div class="card"><strong>With your coding agent</strong><p class="hint">It reads your code, proposes events, goals, funnels and areas, writes trace.json and applies it.</p>${snippet("/trace:setup")}<p class="hint">Not connected yet? <a href="#/setup/start/agent">Connect your coding agent</a>.</p></div>
        <div class="card"><strong>By hand</strong><p class="hint">Start from this example and edit it in your repository.</p></div>
      </div>
      <pre class="snippet">${esc(CONFIG_EXAMPLE)}</pre>
      <div class="actions"><button class="primary" data-copy="${esc(CONFIG_EXAMPLE)}">Copy example</button></div>
      <p class="hint">Check it and publish it, ideally from CI on every merge, with a personal API token from <a href="#/account">Account</a>:</p>
      <div class="copy-row"><pre class="snippet">${esc(push)}</pre><button data-copy="${esc(push)}">Copy</button></div>`;
    },
    insight: () => `
      <h1>Get your first insight</h1>
      <p class="sub">Trace waits until there's enough behaviour to say something true, then it's time to analyse. This step is done after the first analysis, even one that finds nothing worth saving.</p>
      <div id="start-analysis"><p class="hint">Loading…</p></div>`,
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
            <span class="mark">${x.done ? tick : i + 1}</span><span>${esc(START_STEPS[x.key] || x.key)}${x.optional ? ` <span class="hint">(optional)</span>` : ""}</span></a>`
        )
        .join("")}
        <p class="hint">${checklist.complete ? "All set. This checklist stays here, under Setup." : `${checklist.done} of ${checklist.total} done. The dashboard works from the first visit: stop any time.`} <a href="#/home">Go to Home</a></p>
      </nav>
      <section class="start-body">
        <span class="hint">Step ${index + 1} of ${steps.length}${current.done ? " · done" : ""}</span>
        ${(content[current.key] || (() => ""))()}
        <div class="start-foot">
          ${index > 0 ? `<a href="#/start/${steps[index - 1].key}">Back</a>` : "<span></span>"}
          ${next ? `<a class="button" href="#/start/${next.key}">Next: ${esc(START_STEPS[next.key].toLowerCase())}</a>` : `<a class="button" href="#/home">Go to Home</a>`}
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
  if (current.key === "privacy") {
    const p = await api("/privacy");
    const list = view.querySelector("#built-in");
    if (list) list.innerHTML = p.built_in.map((b) => `<li>${esc(b)}</li>`).join("") + (p.rules.length ? `<li>${p.rules.length} rule${p.rules.length === 1 ? "" : "s"} of your own</li>` : "");
  }
  if (current.key === "data" && !project.collection.active) pollSetup();
  if (current.key === "insight") {
    const a = await api("/analysis").catch(() => null);
    const box = view.querySelector("#start-analysis");
    if (a && box) {
      box.innerHTML = analysisPanel(a);
      wireAnalysisPanel(a);
    }
  }
}
