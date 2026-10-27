/*! Telltale collector. Captures pageviews, clicks, rage and dead clicks, JS errors,
 * scroll depth, time on page and form starts/submits. It never records input values.
 *
 *   <script src="https://YOUR-TELLTALE/t.js" data-key="WRITE_KEY" defer></script>
 *
 * Optional attributes: data-endpoint="https://..." to post somewhere else, and
 * data-sample="0.25" to record only a share of visitors (others pay no cost at all).
 * Mark elements with data-telltale="name" for stable names, and wrap private
 * areas in data-telltale-mask to drop their text. Custom events:
 *   telltale.track("purchase", { plan: "pro" }); telltale.identify("user-42");
 *
 * Performance rules this file follows, so it never slows the host page:
 * - Event handlers are passive and only note what happened (target, time, position);
 *   describing elements and queueing happens later, when the browser is idle.
 * - Nothing reads layout on the hot path: no innerText, and scroll height is
 *   sampled at most every 250 ms (re-measured at most every 2 s).
 * - DOM mutations are observed only during the second after a click, not continuously.
 * - Storage writes are throttled; network goes out in batches via sendBeacon.
 * - Every callback is wrapped so a collector bug can never throw into the page.
 */
(function () {
  "use strict";
  var script = document.currentScript;
  if (!script || (window.telltale && window.telltale._loaded)) return;
  var KEY = script.getAttribute("data-key");
  if (!KEY) {
    console.warn("[telltale] missing data-key on the script tag");
    return;
  }
  var ENDPOINT = script.getAttribute("data-endpoint") || new URL("/v1/collect", script.src).href;
  var SAMPLE = parseFloat(script.getAttribute("data-sample") || "1");
  var SELF = script.src;
  var SESSION_IDLE_MS = 30 * 60 * 1000;
  var INTERACTIVE =
    "a,button,input,select,textarea,label,summary,option,[role=button],[role=link],[role=tab]," +
    "[role=menuitem],[role=checkbox],[role=switch],[onclick],[contenteditable],[tabindex]";

  function safe(fn) {
    return function () {
      try {
        return fn.apply(this, arguments);
      } catch (e) {
        /* never let the collector break the page */
      }
    };
  }

  var idle = window.requestIdleCallback
    ? function (fn) {
        requestIdleCallback(safe(fn), { timeout: 1000 });
      }
    : function (fn) {
        setTimeout(safe(fn), 1);
      };

  // ---- storage ---------------------------------------------------------------
  var ls = (function () {
    try {
      var s = window.localStorage;
      s.setItem("tt_probe", "1");
      s.removeItem("tt_probe");
      return s;
    } catch (e) {
      return null;
    }
  })();
  var memory = {};
  function get(k) {
    try {
      return ls ? ls.getItem(k) : memory[k] || null;
    } catch (e) {
      return null;
    }
  }
  function set(k, v) {
    try {
      if (ls) ls.setItem(k, v);
      else memory[k] = v;
    } catch (e) {}
  }
  function uuid() {
    if (window.crypto && crypto.randomUUID) return crypto.randomUUID();
    return "xxxxxxxx-xxxx-4xxx-yxxx-xxxxxxxxxxxx".replace(/[xy]/g, function (c) {
      var r = (Math.random() * 16) | 0;
      return (c === "x" ? r : (r & 3) | 8).toString(16);
    });
  }

  var vid = get("tt_vid") || uuid();
  set("tt_vid", vid);

  // Sampling is per visitor and stable, so a visitor is either always or never recorded.
  if (SAMPLE < 1) {
    var h = 2166136261;
    for (var i = 0; i < vid.length; i++) h = Math.imul(h ^ vid.charCodeAt(i), 16777619);
    if ((h >>> 0) / 4294967296 >= SAMPLE) {
      window.telltale = { _loaded: true, track: function () {}, identify: function () {}, flush: function () {} };
      return;
    }
  }

  var uid = get("tt_uid") || null;
  var sid = get("tt_sid");
  var lastActivity = +get("tt_last") || 0;
  var lastWrite = 0;
  var sentSid = null;

  function session(now) {
    if (!sid || now - lastActivity > SESSION_IDLE_MS) {
      sid = uuid();
      set("tt_sid", sid);
      lastWrite = 0;
    }
    lastActivity = now;
    if (now - lastWrite > 5000) {
      set("tt_last", String(now));
      lastWrite = now;
    }
    return sid;
  }

  // ---- transport -----------------------------------------------------------
  var queue = [];
  var timer = null;

  function push(type, data) {
    var now = Date.now();
    var s = session(now);
    if (s !== sentSid) {
      flush();
      sentSid = s;
    }
    var ev = { type: type, t: now, path: location.pathname };
    for (var k in data) if (data[k] != null) ev[k] = data[k];
    queue.push(ev);
    if (queue.length >= 25) idle(flush);
    else if (!timer)
      timer = setTimeout(function () {
        idle(flush);
      }, 5000);
  }

  function flush() {
    clearTimeout(timer);
    timer = null;
    if (!queue.length || !sentSid) return;
    var body = JSON.stringify({
      key: KEY,
      sid: sentSid,
      vid: vid,
      uid: uid,
      vw: window.innerWidth,
      ref: document.referrer || null,
      now: Date.now(),
      events: queue.splice(0, queue.length),
    });
    // text/plain keeps this a "simple" request: no CORS preflight, and sendBeacon accepts it.
    if (navigator.sendBeacon && navigator.sendBeacon(ENDPOINT, new Blob([body], { type: "text/plain" }))) return;
    try {
      fetch(ENDPOINT, { method: "POST", body: body, keepalive: true, headers: { "Content-Type": "text/plain" } })["catch"](
        function () {}
      );
    } catch (e) {}
  }

  // ---- element description (runs when idle) ----------------------------------
  function stableClass(c) {
    return c && c.length < 30 && !/\d{2,}|__|--|^css-|^sc-|^jsx-/.test(c);
  }

  function selector(el) {
    var parts = [];
    for (var depth = 0; el && el.nodeType === 1 && depth < 4; depth++, el = el.parentElement) {
      var tagged = el.getAttribute("data-telltale");
      if (tagged) {
        parts.unshift('[data-telltale="' + tagged + '"]');
        break;
      }
      var tag = el.tagName.toLowerCase();
      if (el.id && !/\d{3,}|:/.test(el.id)) {
        parts.unshift((tag === "div" || tag === "span" ? "" : tag) + "#" + el.id);
        break;
      }
      var cls = (typeof el.className === "string" ? el.className : "").trim().split(/\s+/).filter(stableClass).slice(0, 2);
      parts.unshift(tag + (cls.length ? "." + cls.join(".") : ""));
      // Stop at the first classed ancestor: short selectors survive layout changes.
      if (tag === "body" || (cls.length && parts.length > 1)) break;
    }
    return parts.join(" > ");
  }

  function label(el) {
    if (el.closest("[data-telltale-mask]")) return undefined;
    var tag = el.tagName;
    var t = el.getAttribute("aria-label");
    if (!t && tag === "INPUT") t = el.type === "submit" || el.type === "button" ? el.value : el.getAttribute("placeholder");
    // textContent, not innerText: innerText forces a layout. Large containers are skipped.
    else if (!t && tag !== "SELECT" && tag !== "TEXTAREA" && el.childElementCount <= 10) t = el.textContent;
    t = t || el.getAttribute("title") || el.getAttribute("alt") || "";
    t = String(t).slice(0, 300).replace(/\s+/g, " ").trim();
    return t ? t.slice(0, 80) : undefined;
  }

  function describe(raw) {
    var el = raw.closest(INTERACTIVE) || raw;
    return { el: el, sel: selector(el), text: label(el) };
  }

  // ---- pages, scroll, time on page --------------------------------------------
  var page = null;
  var scrolls = 0;
  var docHeight = 0;
  var heightAt = 0;
  var scrollSampled = false;

  function scrollDepth() {
    var now = Date.now();
    if (!docHeight || now - heightAt > 2000) {
      docHeight = document.documentElement.scrollHeight;
      heightAt = now;
    }
    if (!docHeight) return 100;
    return Math.min(100, Math.round(((window.scrollY + window.innerHeight) / docHeight) * 100));
  }

  function sampleScroll() {
    scrollSampled = false;
    if (page) page.maxScroll = Math.max(page.maxScroll, scrollDepth());
  }

  function leave() {
    if (!page || page.left) return;
    page.left = true;
    push("scroll", { path: page.path, value: page.maxScroll });
    push("leave", { path: page.path, value: Date.now() - page.start });
  }

  function pageview() {
    if (page && page.path === location.pathname) return;
    leave();
    page = { path: location.pathname, start: Date.now(), maxScroll: 0, left: false, errors: 0 };
    docHeight = 0;
    push("pageview", { text: (document.title || "").slice(0, 120) });
    idle(sampleScroll);
  }

  addEventListener(
    "scroll",
    function () {
      scrolls++;
      if (!scrollSampled) {
        scrollSampled = true;
        setTimeout(safe(sampleScroll), 250);
      }
    },
    { passive: true }
  );

  ["pushState", "replaceState"].forEach(function (fn) {
    var original = history[fn];
    if (typeof original !== "function") return;
    history[fn] = function () {
      var result = original.apply(this, arguments);
      setTimeout(safe(pageview), 0);
      return result;
    };
  });
  addEventListener("popstate", function () {
    setTimeout(safe(pageview), 0);
  });

  function hide() {
    leave();
    flush();
  }
  document.addEventListener(
    "visibilitychange",
    safe(function () {
      if (document.visibilityState === "hidden") hide();
    })
  );
  addEventListener("pagehide", safe(hide));

  // ---- clicks: plain, rage and dead -------------------------------------------
  // A MutationObserver runs only while some click is waiting to see whether the
  // page reacted, so pages pay nothing for it between clicks.
  var observer = null;
  var watching = 0;
  var mutations = 0;

  function watch() {
    if (!window.MutationObserver) return false;
    if (!observer)
      observer = new MutationObserver(function (records) {
        mutations += records.length;
      });
    if (watching++ === 0)
      observer.observe(document.documentElement, { childList: true, subtree: true, attributes: true, characterData: true });
    return true;
  }

  function unwatch() {
    if (--watching === 0) {
      mutations += observer.takeRecords().length;
      observer.disconnect();
    }
  }

  var burst = [];
  var lastDead = {};
  document.addEventListener(
    "click",
    safe(function (e) {
      var target = e.target;
      var raw = target && target.nodeType === 1 ? target : target && target.parentElement;
      if (!raw) return;
      var t = Date.now();
      var path = location.pathname;

      // Rage click: 3+ clicks within 700 ms, close together. Cheap arithmetic only.
      burst = burst.filter(function (c) {
        return t - c.t < 700 && Math.abs(c.x - e.clientX) < 40 && Math.abs(c.y - e.clientY) < 40;
      });
      burst.push({ t: t, x: e.clientX, y: e.clientY });
      var rage = burst.length === 3;

      // Dead click: nothing in the page changed within a second. Links that open
      // elsewhere (new tab, download) legitimately leave the page unchanged.
      var link = raw.closest("a[href]");
      var opensElsewhere = link && (e.metaKey || e.ctrlKey || e.shiftKey || link.target || link.hasAttribute("download"));
      var deadCheck = !opensElsewhere && !/^(INPUT|SELECT|TEXTAREA|LABEL|OPTION)$/.test(raw.tagName) && watch();
      var m = mutations;
      var s = scrolls;
      var href = location.href;

      idle(function () {
        var d = describe(raw);
        var data = { t: t, path: path, sel: d.sel, text: d.text };
        push("click", data);
        if (rage) push("rage_click", data);
        if (!deadCheck) return;
        if (/^(INPUT|SELECT|TEXTAREA|LABEL|OPTION)$/.test(d.el.tagName)) deadCheck = false;
        setTimeout(
          safe(function () {
            unwatch();
            if (!deadCheck || mutations !== m || scrolls !== s || location.href !== href || document.hidden) return;
            // Repeated clicks on the same dead element count once (the rage click covers the rest).
            if (lastDead.sel === data.sel && t - lastDead.t < 2000) return;
            lastDead = { sel: data.sel, t: t };
            push("dead_click", data);
          }),
          Math.max(0, 1000 - (Date.now() - t))
        );
      });
    }),
    { capture: true, passive: true }
  );

  // ---- forms ---------------------------------------------------------------
  var started = typeof WeakSet === "function" ? new WeakSet() : null;
  document.addEventListener(
    "focusin",
    safe(function (e) {
      var form = e.target && e.target.form;
      if (!form || !started || started.has(form)) return;
      started.add(form);
      idle(function () {
        push("form_start", { sel: selector(form) });
      });
    }),
    { capture: true, passive: true }
  );
  document.addEventListener(
    "submit",
    safe(function (e) {
      // Submits often navigate away, so this one is recorded and sent right away.
      if (e.target && e.target.nodeType === 1) push("submit", { sel: selector(e.target) });
      flush();
    }),
    { capture: true, passive: true }
  );

  // ---- errors --------------------------------------------------------------
  function error(message, source) {
    if (page && page.errors++ >= 20) return;
    push("error", { text: String(message).slice(0, 300), sel: source });
  }
  addEventListener(
    "error",
    safe(function (e) {
      if (!e.message || e.filename === SELF) return; // failed <img>/<script> loads carry no message
      var file = e.filename ? e.filename.replace(location.origin, "") : "";
      error(e.message, file ? file + ":" + e.lineno : undefined);
    })
  );
  addEventListener(
    "unhandledrejection",
    safe(function (e) {
      var r = e.reason;
      error("Unhandled rejection: " + ((r && r.message) || r));
    })
  );

  // ---- public API ------------------------------------------------------------
  window.telltale = {
    _loaded: true,
    track: safe(function (name, props) {
      push("custom", { name: String(name).slice(0, 128), props: props });
    }),
    identify: safe(function (id) {
      uid = id == null ? null : String(id);
      set("tt_uid", uid || "");
    }),
    flush: safe(flush),
  };

  safe(pageview)();
})();
