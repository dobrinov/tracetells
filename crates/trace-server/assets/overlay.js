/*! Trace page overlay | MIT License */
// Draws a page story over the website's own page, in the browser of a team
// member who opened an overlay link (#trace-overlay=<token>) from the
// dashboard. The collector loads this instead of collecting on that page load.
//
// It reads one thing from Trace (the page's story, with the token) and sends
// nothing about the page. Everything it draws lives in one shadow root, so the
// page's own markup and styles are untouched. Text from the story (labels,
// selectors) came from visitors' browsers: it is only ever set as textContent.
(function () {
  "use strict";
  if (window.__traceOverlay) return;
  window.__traceOverlay = true;

  var script = document.currentScript;
  var SERVER = script && script.src ? new URL(script.src).origin : "";
  var match = /trace-overlay=([\w-]+)/.exec(location.hash);
  var TOKEN = match ? match[1] : "";
  // The token leaves the address bar at once, so it isn't bookmarked or shared.
  try {
    history.replaceState(null, "", location.href.replace(/#?&?trace-overlay=[\w-]+/, ""));
  } catch (e) {}

  var api = window.traceAnalytics;
  function routeName() {
    try {
      if (api && api.route) return api.route();
    } catch (e) {}
    return location.pathname.replace(/\/+$/, "") || "/";
  }

  var STEP_MS = 1200;
  var state = { days: 14, view: "use", story: null, error: null, playing: null, step: -1, timer: null, route: routeName() };

  // ---- the layer -------------------------------------------------------------
  var host = document.createElement("div");
  host.setAttribute("data-trace-overlay", "");
  host.style.cssText = "position:fixed;inset:0;z-index:2147483647;pointer-events:none;";
  var root = host.attachShadow ? host.attachShadow({ mode: "open" }) : host;
  var style = document.createElement("style");
  style.textContent =
    ":host{all:initial}" +
    "*{box-sizing:border-box;font-family:system-ui,-apple-system,'Segoe UI',sans-serif}" +
    ".badge{position:fixed;pointer-events:none;padding:2px 7px;border-radius:999px;font:600 11px/1.5 ui-monospace,Menlo,monospace;color:#fff;background:#10141d;white-space:nowrap;transform:translate(-30%,-60%)}" +
    ".badge.dead{background:#c2410c}" +
    ".box{position:fixed;pointer-events:none;border:2px solid rgba(59,91,253,.55);background:rgba(59,91,253,.08);border-radius:6px}" +
    ".box.dead{border-color:rgba(194,65,12,.7);background:rgba(194,65,12,.10)}" +
    ".cursor{position:fixed;pointer-events:none;width:34px;height:34px;margin:-17px 0 0 -17px;border-radius:50%;border:3px solid #3b5bfd;background:rgba(59,91,253,.2);transition:left .5s ease,top .5s ease;display:none}" +
    ".panel{position:fixed;right:16px;bottom:16px;width:340px;max-width:calc(100vw - 32px);max-height:70vh;overflow:auto;pointer-events:auto;background:#fff;color:#10141d;border:1px solid #e3e6eb;border-radius:14px;box-shadow:0 12px 40px rgba(16,20,29,.18);padding:16px;font-size:14px;line-height:1.45}" +
    ".panel h2{margin:0;font-size:16px}.panel h3{margin:14px 0 6px;font-size:13px;color:#4a5162;font-weight:600}" +
    ".row{display:flex;gap:8px;align-items:center;flex-wrap:wrap}.sp{margin-left:auto}" +
    ".hint{color:#4a5162;font-size:13px}.mono{font-family:ui-monospace,Menlo,monospace;font-size:12px}" +
    "button{font:inherit;cursor:pointer;border:1px solid #d5dae3;background:#fff;color:#10141d;border-radius:8px;padding:6px 10px;min-height:36px}" +
    "button.on{background:#10141d;color:#fff;border-color:#10141d}" +
    ".pat{border:1px solid #e3e6eb;border-radius:10px;padding:10px;margin-top:8px}" +
    ".steps{margin:6px 0 0;padding-left:18px}.steps li.now{font-weight:700;color:#2238c9}" +
    ".err{color:#b13a0a}";
  root.appendChild(style);
  var layer = document.createElement("div");
  var cursor = document.createElement("div");
  cursor.className = "cursor";
  var panel = document.createElement("div");
  panel.className = "panel";
  panel.setAttribute("role", "dialog");
  panel.setAttribute("aria-label", "Trace page story");
  root.appendChild(layer);
  root.appendChild(cursor);
  root.appendChild(panel);
  (document.body || document.documentElement).appendChild(host);

  function el(tag, attrs, text) {
    var e = document.createElement(tag);
    for (var k in attrs || {}) {
      if (k === "className") e.className = attrs[k];
      else if (k === "onclick") e.addEventListener("click", attrs[k]);
      else e.setAttribute(k, attrs[k]);
    }
    if (text != null) e.textContent = String(text);
    return e;
  }
  function pct(x) {
    return Math.round((x || 0) * 100) + "%";
  }

  // ---- finding the story's elements on the page ------------------------------
  // The collector labels elements by their text with digits as #, like this.
  function label(node) {
    return (node.getAttribute("aria-label") || node.textContent || node.value || "").replace(/\s+/g, " ").trim().slice(0, 80).replace(/\d/g, "#");
  }
  function find(selector, text) {
    var all;
    try {
      all = Array.prototype.slice.call(document.querySelectorAll(selector));
    } catch (e) {
      return [];
    }
    all = all.filter(function (n) {
      return !host.contains(n);
    });
    if (text && all.length > 1) {
      var same = all.filter(function (n) {
        return label(n) === text;
      });
      if (same.length) return same;
    }
    return all;
  }

  // ---- drawing ---------------------------------------------------------------
  var drawn = [];
  function place() {
    for (var i = 0; i < drawn.length; i++) {
      var d = drawn[i];
      var r = d.node.getBoundingClientRect();
      var visible = r.width > 0 && r.height > 0;
      d.box.style.display = d.badge.style.display = visible ? "" : "none";
      d.box.style.left = r.left + "px";
      d.box.style.top = r.top + "px";
      d.box.style.width = r.width + "px";
      d.box.style.height = r.height + "px";
      d.badge.style.left = r.right + "px";
      d.badge.style.top = r.top + "px";
    }
    if (state.playing != null) moveCursor();
  }
  var pending = false;
  function schedule() {
    if (pending) return;
    pending = true;
    requestAnimationFrame(function () {
      pending = false;
      place();
    });
  }
  addEventListener("scroll", schedule, { passive: true, capture: true });
  addEventListener("resize", schedule, { passive: true });

  function draw() {
    layer.textContent = "";
    drawn = [];
    var s = state.story;
    if (!s || s.too_little_data) return;
    var missing = [];
    s.elements.forEach(function (e) {
      var nodes = find(e.selector, e.text);
      if (!nodes.length) return missing.push(e);
      var dead = state.view === "dead";
      if (dead && !e.dead_sessions) return;
      var text = dead ? pct(e.dead_share) + " did nothing" : pct(e.share);
      if (nodes.length > 1) text += " · one of " + nodes.length + " like this";
      nodes.forEach(function (node) {
        var box = el("div", { className: "box" + (dead ? " dead" : "") });
        var badge = el("div", { className: "badge" + (dead ? " dead" : "") }, text);
        layer.appendChild(box);
        layer.appendChild(badge);
        drawn.push({ node: node, box: box, badge: badge });
      });
    });
    state.missing = missing;
    place();
  }

  // ---- playing a pattern -------------------------------------------------------
  // Each action of a pattern is one beat: the marker moves to its element at a
  // fixed pace, never at any visit's own timing.
  // One beat per step: its first click (the other actions happened at the same moment).
  function beats(p) {
    return p.steps.map(function (st) {
      var click = st.actions.filter(function (a) {
        return a.kind === "click";
      })[0];
      return { action: click || st.actions[0], others: st.actions.length - 1, repeated: st.repeated, nothing: st.did_nothing_sessions, total: p.sessions };
    });
  }
  function describe(b) {
    var x = b.action;
    var what = x.kind === "click" ? "Click " + (x.text ? "“" + x.text + "”" : x.selector) : x.kind === "error" ? "An error (" + x.name + ")" : x.kind === "custom" ? "Event “" + x.name + "”" : x.kind === "submit" ? "Submit the form" : "Start filling in the form";
    if (b.others) what += " (+" + b.others + " at the same moment)";
    if (b.repeated) what += ", again and again";
    if (b.nothing) what += ": did nothing in " + b.nothing + " of " + b.total;
    return what;
  }
  function moveCursor() {
    var p = state.story.patterns[state.playing];
    var b = beats(p)[state.step];
    if (!b || b.action.kind !== "click") return;
    var nodes = find(b.action.selector, b.action.text);
    if (!nodes.length) return (cursor.style.display = "none");
    var r = nodes[0].getBoundingClientRect();
    cursor.style.display = "block";
    cursor.style.borderColor = b.nothing * 4 >= b.total ? "#c2410c" : "#3b5bfd";
    cursor.style.left = r.left + r.width / 2 + "px";
    cursor.style.top = r.top + r.height / 2 + "px";
  }
  function stop() {
    clearInterval(state.timer);
    state.timer = null;
    state.playing = null;
    state.step = -1;
    cursor.style.display = "none";
  }
  function play(i) {
    var again = state.playing === i;
    stop();
    if (again) return render();
    state.playing = i;
    state.step = 0;
    var n = beats(state.story.patterns[i]).length;
    state.timer = setInterval(function () {
      if (state.step >= n - 1) {
        clearInterval(state.timer);
        state.timer = null;
      } else state.step++;
      moveCursor();
      render();
    }, STEP_MS);
    moveCursor();
    render();
  }

  // ---- the panel -------------------------------------------------------------
  function render() {
    panel.textContent = "";
    var head = el("div", { className: "row" });
    head.appendChild(el("h2", null, "Trace · " + state.route));
    head.appendChild(el("button", { className: "sp", "aria-label": "Close the overlay", onclick: close }, "×"));
    panel.appendChild(head);
    if (state.error) return panel.appendChild(el("p", { className: "err" }, state.error));
    var s = state.story;
    if (!s) return panel.appendChild(el("p", { className: "hint" }, "Loading this page's story…"));
    var days = el("div", { className: "row" });
    [7, 14, 30].forEach(function (d) {
      days.appendChild(el("button", { className: d === state.days ? "on" : "", "aria-pressed": String(d === state.days), onclick: function () { state.days = d; load(); } }, d + " days"));
    });
    panel.appendChild(days);
    panel.appendChild(el("p", { className: "hint" }, s.sessions + " sessions" + (s.capped ? " (the latest)" : "") + ". Patterns, never single visits: nothing from fewer than " + s.min_sessions + " sessions is shown."));
    if (s.too_little_data) return panel.appendChild(el("p", null, "Too few sessions on this page yet to show how people use it."));

    var views = el("div", { className: "row" });
    [["use", "Use"], ["dead", "Clicks that did nothing"]].forEach(function (v) {
      views.appendChild(el("button", { className: state.view === v[0] ? "on" : "", "aria-pressed": String(state.view === v[0]), onclick: function () { state.view = v[0]; draw(); render(); } }, v[1]));
    });
    panel.appendChild(views);

    panel.appendChild(el("h3", null, "How people use it"));
    if (!s.patterns.length) panel.appendChild(el("p", { className: "hint" }, "No pattern is shared by enough sessions yet."));
    s.patterns.forEach(function (p, i) {
      var box = el("div", { className: "pat" });
      var row = el("div", { className: "row" });
      row.appendChild(el("strong", null, p.steps.length ? (p.goes_wrong ? "Goes wrong · " : "") + p.sessions + " of " + s.sessions + " sessions" + (p.repeats ? " · again and again" : "") : p.sessions + " sessions did nothing here"));
      if (p.steps.length) row.appendChild(el("button", { className: "sp" + (state.playing === i ? " on" : ""), onclick: function () { play(i); } }, state.playing === i ? "Stop" : "Play"));
      box.appendChild(row);
      var list = el("ol", { className: "steps" });
      beats(p).forEach(function (b, k) {
        list.appendChild(el("li", { className: state.playing === i && state.step === k ? "now" : "" }, describe(b)));
      });
      box.appendChild(list);
      panel.appendChild(box);
    });
    if (s.hidden_patterns) panel.appendChild(el("p", { className: "hint" }, s.hidden_patterns + " more sequence" + (s.hidden_patterns === 1 ? " is" : "s are") + " hidden: each comes from fewer than " + s.min_sessions + " sessions."));

    if (state.missing && state.missing.length) {
      panel.appendChild(el("h3", null, "Not on this page now"));
      state.missing.forEach(function (e) {
        var r = el("div", { className: "row" });
        r.appendChild(el("span", { className: "mono" }, e.text || e.selector));
        r.appendChild(el("span", { className: "sp hint" }, pct(e.share)));
        panel.appendChild(r);
      });
    }
  }

  function close() {
    stop();
    removeEventListener("scroll", schedule, true);
    removeEventListener("resize", schedule);
    clearInterval(watch);
    host.remove();
  }

  // ---- loading ---------------------------------------------------------------
  function load() {
    stop();
    state.story = null;
    state.error = null;
    render();
    if (!TOKEN || !SERVER) {
      state.error = "This overlay link is incomplete: open a new one from the page story.";
      return render();
    }
    fetch(SERVER + "/v1/overlay/story?route=" + encodeURIComponent(state.route) + "&days=" + state.days, { headers: { Authorization: "Bearer " + TOKEN }, credentials: "omit" })
      .then(function (res) {
        return res.json().then(
          function (body) {
            return { ok: res.ok, body: body };
          },
          function () {
            return { ok: false, body: {} };
          }
        );
      })
      .then(function (r) {
        if (!r.ok) {
          state.error = (r.body && r.body.error) || "Trace didn't answer. Open a new link from the page story.";
          layer.textContent = "";
          drawn = [];
        } else {
          state.story = r.body;
          draw();
        }
        render();
      })
      ["catch"](function () {
        state.error = "Couldn't reach Trace.";
        render();
      });
  }

  // A single-page app can move to another page: follow it, and redraw when the page changes.
  var watch = setInterval(function () {
    var now = routeName();
    if (now !== state.route) {
      state.route = now;
      load();
    } else if (state.story) draw();
  }, 1500);

  load();
})();
