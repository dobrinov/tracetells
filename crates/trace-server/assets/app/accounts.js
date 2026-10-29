"use strict";

// Signing in, accounts, people and the team.
// The dashboard is plain scripts loaded in order (see index.html); they share one scope.

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
  measurePage("/sign-in");
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
  measurePage("/sign-up");
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
    measure("signed_up");
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
  try {
    if (auth.analytics_id) window.trace?.identify?.(auth.analytics_id);
  } catch {}
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
  try {
    window.trace?.identify?.(null);
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
  document.querySelector('#nav a[data-view="admin"]').hidden = !auth.user.is_operator;
  // Shown by analysisBadge when an analysis is due; never while viewing as someone.
  document.getElementById("run-analysis").hidden = true;
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
    measure("invite_sent");
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
