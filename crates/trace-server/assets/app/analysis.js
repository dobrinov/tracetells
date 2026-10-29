"use strict";

// The analysis panel: when the last analysis ran, what's new since and how to run the next.
// The dashboard is plain scripts loaded in order (see index.html); they share one scope.

// ---------------------------------------------------------------- Analysis

// When an analysis last ran, what it read, how much is new since, and what to
// do about it: run /trace:analyse in the coding agent.
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
          <span class="hint">It shows you what it found; the insights you keep are saved here and this panel resets.</span>
        </div>`
      : `<div class="analysis-how">
          <span>Analysis runs in your coding agent, with the model you choose, over MCP. It reads your code too.</span>
          <a class="button primary" href="#/setup/install">Connect your coding agent</a>
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

// Wires a rendered panel: copy, and noticing when the agent saves.
function wireAnalysisPanel(a) {
  view.querySelectorAll(".analysis-panel [data-copy]").forEach((b) =>
    b.addEventListener("click", async () => {
      if (await copyText(b.dataset.copy)) {
        toast("Copied. Paste it into Claude Code.");
        measure("copied", { what: "analyse" });
      }
    })
  );
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

// The header button appears when an analysis is worth running, with the count.
function analysisBadge(a) {
  const btn = document.getElementById("run-analysis");
  if (!btn || btn.disabled) return;
  btn.hidden = a.next !== "analyse" || !can("user") || Boolean(auth?.impersonating);
  btn.classList.toggle("ready", a.next === "analyse");
  btn.textContent = `Analyse · ${num(a.new_sessions)} new`;
  btn.title = a.message;
}
