/*! Trace collector. MIT License, see LICENSE-collector next to this file.
 * Captures pageviews, clicks, rage and dead clicks, JS errors,
 * scroll depth, time on page and form starts/submits. It never records input values.
 *
 * Works the same on multi-page sites (full reloads, including back/forward cache
 * restores) and single-page apps (history API, Navigation API and hash routers).
 *
 *   <script src="https://YOUR-TRACE-SERVER/d.js" data-key="WRITE_KEY" defer></script>
 *
 * Options, as data-* attributes on the script tag, or as an object in
 * window.traceConfig / trace.init({...}) when the tag can't carry them
 * (bundlers, type="module", tag managers):
 *   key        write key (required)
 *   endpoint   collect URL, defaults to /v1/collect next to d.js
 *   routes     route patterns that group dynamic URLs, e.g. "/products/:id, /docs/*".
 *              Numeric, UUID and long-hash segments become :id automatically.
 *   sample     share of visitors to record, e.g. 0.25 (the rest pay no cost at all)
 *   pageviews  "auto" (default) or "manual", where you call trace.page()
 *   console    "false" to stop recording console.error calls
 *   consent    "banner" or "required": record nothing until the visitor agrees.
 *              "banner" shows a small bar with Allow / No thanks; "required"
 *              leaves asking to your own consent tool, which calls trace.consent().
 *              Before an answer nothing is stored, listened to or sent; the answer
 *              itself is kept in localStorage (trc_consent). Global Privacy
 *              Control counts as "no". Any element with data-trace-consent (e.g. a
 *              footer link) opens the bar again so visitors can change their mind.
 *   consent-text, consent-link   the bar's sentence, and a "How it works" link
 *   consent-allow, consent-deny, consent-link-text   the two buttons' labels and
 *              the link's text, for a site that is not in English
 *
 * Calls made before the script loads are queued if you add this stub first:
 *   <script>window.trace=window.trace||{q:[]};["track","identify","page","init"].forEach(function(m){trace[m]=trace[m]||function(){trace.q.push([m,arguments])}})</script>
 *
 * API: trace.track(name, props) | trace.identify(userId) | trace.page([route]) | trace.flush()
 *      trace.consent(true | false) records the visitor's answer; trace.consent() returns
 *      "yes", "no" or null
 * Tracking without JavaScript: data-trace-event="add_to_cart" sends that custom
 * event when the element is clicked (or, on a <form>, submitted). Static props can
 * go in data-trace-props='{"plan":"pro"}'. Declare event names in trace.json.
 * Markup: data-trace="name" gives an element a stable name; text inside
 * data-trace-mask is never sent.
 *
 * Privacy rules this file follows, so sensitive data never leaves the browser:
 * - What people type is never read. No DOM snapshots, no page titles.
 * - Text is read only from buttons, links and labels, never from containers or
 *   editors, and never inside a form with password or payment fields, or inside
 *   data-trace-mask. Every digit in that text is masked (amounts, balances).
 * - Emails, tokens, card numbers, IBANs and URL query strings are replaced in
 *   everything sent; path segments that look like ids, emails or tokens are grouped.
 * - The referrer is reduced to its origin. Custom-event props with sensitive
 *   names (password, token, card, email, iban, ...) are dropped. Email-like
 *   user ids are hashed. The server applies the same rules again.
 * - Rules an admin adds on the dashboard's Privacy page (text to never read,
 *   patterns to redact, pages to ignore, props to drop) are written into this
 *   file by the server and applied here first, then again on the server.
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
  // `trace` is a common name: only take it when it's free or is our own
  // loading stub; `traceAnalytics` always works.
  var existing = window.traceAnalytics || window.trace;
  if (existing && existing._loaded) return;
  var ours = !window.trace || !!window.trace.q || !!window.trace._loaded;
  var pending = (existing && existing.q) || [];
  var script = document.currentScript;
  var SCRIPT_SRC = (script && script.src) || "";
  var SESSION_IDLE_MS = 30 * 60 * 1000;
  var INTERACTIVE =
    "a,button,input,select,textarea,label,summary,option,[role=button],[role=link],[role=tab]," +
    "[role=menuitem],[role=checkbox],[role=switch],[onclick],[contenteditable],[tabindex]";
  var FORM_FIELD = /^(INPUT|SELECT|TEXTAREA|LABEL|OPTION)$/;

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

  // ---- redaction -------------------------------------------------------------
  var RX_URL_QUERY = /(https?:\/\/[^\s?#]+)[?#]\S*/g;
  var RX_EMAIL = /[A-Z0-9._%+-]+(@|%40)[A-Z0-9.-]+\.[A-Z]{2,}/gi;
  var RX_JWT = /\beyJ[\w-]+\.[\w-]+\.[\w-]+/g;
  var RX_IBAN = /\b[A-Z]{2}\d{2}(?:\s?[A-Z0-9]{4}){2,7}(?:\s?[A-Z0-9]{1,4})?\b/g;
  var RX_CARD = /\b(?:\d[ -]?){12,18}\d\b/g;
  var RX_TOKEN = /[A-Za-z0-9_\-+\/=]{20,}/g;
  // Phone numbers, account numbers, dates of birth: 7+ digits, possibly split by separators.
  var RX_LONG_NUMBER = /\+?\d(?:[\s().-]{0,2}\d){6,}/g;
  var SENSITIVE_KEY_WORDS =
    " password passwd pwd pass passcode passphrase secret token auth authorization bearer cookie session sid jwt" +
    " apikey key credential credentials card cc ccv cvv cvc pan iban swift bic account acct routing sortcode ssn sin" +
    " nin tax vat pin otp mfa totp email mail phone mobile tel address street zip postcode postal birth birthday dob" +
    " salary income balance passport license ";
  // Form fields whose presence makes a whole form off limits for text capture.
  var SENSITIVE_FIELDS =
    "input[type=password],[autocomplete^=cc-],[autocomplete=current-password],[autocomplete=new-password]," +
    "[autocomplete=one-time-code],[name*=card],[name*=cvv],[name*=cvc],[name*=iban],[name*=ssn],[name*=secret],[name*=token]";
  // The only elements whose own text is read.
  var TEXT_CONTROLS =
    "a,button,label,summary,[role=button],[role=link],[role=tab],[role=menuitem],[role=option],input[type=submit],input[type=button]";

  // The workspace's own rules, written in by the server when it serves this file.
  var HOST_RULES = /*__TRACE_RULES__*/ null;
  var HOST_REDACT = [];
  var HOST_DROP = {};
  var HOST_MASK = "";
  var HOST_IGNORE = [];
  (function (r) {
    if (!r) return;
    (r.redact || []).forEach(function (p) {
      try {
        var src = p[0];
        var flags = "g";
        if (src.indexOf("(?i)") === 0) {
          src = src.slice(4);
          flags += "i";
        }
        HOST_REDACT.push([new RegExp(src, flags), p[1]]);
      } catch (e) {}
    });
    (r.drop || []).forEach(function (k) {
      HOST_DROP[String(k).toLowerCase()] = true;
    });
    HOST_MASK = (r.mask || [])
      .filter(function (sel) {
        try {
          document.querySelector(sel);
          return true;
        } catch (e) {
          return false;
        }
      })
      .join(",");
    HOST_IGNORE = r.ignore || [];
  })(HOST_RULES);

  function matchesPattern(pattern, value) {
    return pattern.slice(-1) === "*" ? value.indexOf(pattern.slice(0, -1)) === 0 : value === pattern;
  }

  function isToken(m) {
    return /\d/.test(m) && /[A-Za-z]/.test(m);
  }

  // strict: also mask every digit (text read from the page).
  function redact(s, strict) {
    s = String(s);
    // Workspace patterns first: built-in masking would otherwise change what they look for.
    for (var i = 0; i < HOST_REDACT.length; i++) s = s.replace(HOST_REDACT[i][0], HOST_REDACT[i][1]);
    s = s
      .replace(RX_URL_QUERY, "$1?[redacted]")
      .replace(RX_EMAIL, "[email]")
      .replace(RX_JWT, "[token]")
      .replace(RX_IBAN, "[iban]")
      .replace(RX_CARD, "[card]")
      .replace(RX_TOKEN, function (m) {
        return isToken(m) ? "[token]" : m;
      });
    return strict
      ? s.replace(/\d/g, "#")
      : s.replace(RX_LONG_NUMBER, "[number]");
  }

  function isSensitiveKey(k) {
    var words = String(k)
      .replace(/([a-z0-9])([A-Z])/g, "$1 $2")
      .toLowerCase()
      .split(/[^a-z0-9]+/);
    for (var i = 0; i < words.length; i++) if (words[i] && SENSITIVE_KEY_WORDS.indexOf(" " + words[i] + " ") >= 0) return true;
    return false;
  }

  function cleanProps(v, depth) {
    if (v == null) return undefined;
    if (typeof v === "string") return redact(v.slice(0, 200), false);
    if (typeof v === "number" || typeof v === "boolean") return isFinite(v) || typeof v === "boolean" ? v : undefined;
    if (typeof v !== "object" || depth > 2) return undefined;
    var out = Array.isArray(v) ? [] : {};
    var n = 0;
    for (var k in v) {
      if (!Object.prototype.hasOwnProperty.call(v, k) || n >= 20) continue;
      if (!Array.isArray(v) && (isSensitiveKey(k) || HOST_DROP[k.toLowerCase()])) continue;
      var c = cleanProps(v[k], depth + 1);
      if (c === undefined) continue;
      if (Array.isArray(out)) out.push(c);
      else out[k] = c;
      n++;
    }
    return out;
  }

  function redactSegment(seg) {
    if (!seg || seg.charAt(0) === ":" || seg === "*") return seg;
    if (seg.indexOf("@") >= 0 || /%40/i.test(seg)) return ":email";
    if (seg.indexOf("=") >= 0 || (seg.split(".").length > 2 && seg.length >= 24)) return ":token";
    if ((seg.match(/\d/g) || []).length >= 3 || (seg.length >= 16 && (isToken(seg) || /^[0-9a-f]+$/i.test(seg)))) return ":id";
    return seg;
  }

  function referrerOrigin() {
    try {
      if (!document.referrer) return null;
      var o = new URL(document.referrer).origin;
      return o === location.origin || o === "null" ? null : o;
    } catch (e) {
      return null;
    }
  }

  // Public API. Until start() runs, calls are queued (like the stub snippet does).
  var api = {
    _loaded: true,
    q: pending,
    init: function (config) {
      pending.push(["init", [config]]);
    },
    track: function () {
      pending.push(["track", arguments]);
    },
    identify: function () {
      pending.push(["identify", arguments]);
    },
    page: function () {
      pending.push(["page", arguments]);
    },
    flush: function () {},
  };
  window.traceAnalytics = api;
  if (ours) window.trace = api;

  function scriptConfig() {
    if (!script) return null;
    var c = {};
    ["key", "endpoint", "routes", "sample", "pageviews", "console", "consent", "consent-text", "consent-link", "consent-link-text", "consent-allow", "consent-deny"].forEach(function (k) {
      var v = script.getAttribute("data-" + k);
      if (v != null) c[k] = v;
    });
    return c.key ? c : null;
  }

  var started = false;
  // Set when a visitor withdraws consent: nothing more is stored or sent.
  var optedOut = false;
  function start(config) {
    if (started || !config || !config.key) return;
    started = true;
    var KEY = String(config.key);
    var ENDPOINT = config.endpoint || (SCRIPT_SRC ? new URL("/v1/collect", SCRIPT_SRC).href : "/v1/collect");
    var SAMPLE = config.sample == null ? 1 : parseFloat(config.sample);
    var MANUAL_PAGEVIEWS = config.pageviews === "manual";
    // console.error capture can be turned off (console: false), e.g. if a
    // debugging setup relies on console.error's call site in devtools.
    var CAPTURE_CONSOLE = String(config.console) !== "false";
    var ROUTES = (Array.isArray(config.routes) ? config.routes : String(config.routes || "").split(","))
      .map(function (r) {
        return String(r).trim();
      })
      .filter(Boolean)
      .map(function (r) {
        return { name: r.replace(/\/+$/, "") || "/", segs: r.replace(/\/+$/, "").split("/") };
      });

    // ---- storage -------------------------------------------------------------
    var ls = (function () {
      try {
        var s = window.localStorage;
        s.setItem("trc_probe", "1");
        s.removeItem("trc_probe");
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
      if (optedOut) return;
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

    var vid = get("trc_vid") || uuid();
    set("trc_vid", vid);

    // Sampling is per visitor and stable, so a visitor is either always or never recorded.
    if (SAMPLE < 1) {
      var h = 2166136261;
      for (var i = 0; i < vid.length; i++) h = Math.imul(h ^ vid.charCodeAt(i), 16777619);
      if ((h >>> 0) / 4294967296 >= SAMPLE) {
        api.track = api.identify = api.page = api.init = api.flush = function () {};
        return;
      }
    }

    var uid = get("trc_uid") || null;
    if (uid && /@|%40|\s/.test(uid)) uid = null;
    var sid = get("trc_sid");
    var lastActivity = +get("trc_last") || 0;
    var lastWrite = 0;
    var sentSid = null;

    function session(now) {
      if (!sid || now - lastActivity > SESSION_IDLE_MS) {
        sid = uuid();
        set("trc_sid", sid);
        lastWrite = 0;
      }
      lastActivity = now;
      if (now - lastWrite > 5000) {
        set("trc_last", String(now));
        lastWrite = now;
      }
      return sid;
    }

    // ---- routes --------------------------------------------------------------
    // Hash routers (#/settings, #!/settings) use the hash as the route; plain
    // anchors (#section) don't. Dynamic segments are grouped so /orders/123 and
    // /orders/456 count as one page.
    var ID_SEGMENT = /^(\d+|[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}|[0-9a-f]{16,}|(?=[^/]*\d)[A-Za-z0-9_-]{24,})$/i;
    var routeHref = null;
    var routeName = null;
    var manualRoute = null;
    var manualHref = null;

    function route() {
      if (manualRoute) return manualRoute;
      var href = location.href;
      if (href === routeHref) return routeName;
      var path = location.pathname;
      if (/^#!?\//.test(location.hash)) path = location.hash.replace(/^#!?/, "").split("?")[0];
      path = path.replace(/\/+$/, "") || "/";
      var segs = path.split("/");
      var name = null;
      for (var r = 0; r < ROUTES.length && !name; r++) {
        var p = ROUTES[r].segs;
        var wildcard = p[p.length - 1] === "*";
        if (wildcard ? segs.length < p.length - 1 : segs.length !== p.length) continue;
        var ok = true;
        for (var s = 0; s < p.length && ok; s++) {
          if (wildcard && s === p.length - 1) break;
          ok = p[s].charAt(0) === ":" || p[s] === segs[s];
        }
        if (ok) name = ROUTES[r].name;
      }
      if (!name)
        name = segs
          .map(function (seg) {
            return ID_SEGMENT.test(seg) ? ":id" : redactSegment(seg);
          })
          .join("/");
      routeHref = href;
      routeName = name;
      return name;
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
      var ev = { type: type, t: now, path: route() };
      for (var k in data) if (data[k] != null) ev[k] = data[k];
      if (optedOut) return ev;
      // Pages the workspace ignores: nothing is recorded there at all.
      for (var g = 0; g < HOST_IGNORE.length; g++)
        if (matchesPattern(HOST_IGNORE[g], ev.path) || matchesPattern(HOST_IGNORE[g], location.pathname)) return ev;
      queue.push(ev);
      if (queue.length >= 25) idle(flush);
      else if (!timer)
        timer = setTimeout(function () {
          idle(flush);
        }, 5000);
      return ev;
    }

    function flush() {
      clearTimeout(timer);
      timer = null;
      if (optedOut) queue.length = 0;
      if (!queue.length || !sentSid) return;
      var body = JSON.stringify({
        key: KEY,
        sid: sentSid,
        vid: vid,
        uid: uid,
        vw: window.innerWidth,
        ref: referrerOrigin(),
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

    // ---- element description (runs when idle) --------------------------------
    function stableClass(c) {
      return c && c.length < 30 && !/\d{2,}|__|--|^css-|^sc-|^jsx-/.test(c);
    }

    function selector(el) {
      var parts = [];
      for (var depth = 0; el && el.nodeType === 1 && depth < 4; depth++, el = el.parentElement) {
        var tagged = el.getAttribute("data-trace");
        if (tagged) {
          parts.unshift('[data-trace="' + tagged + '"]');
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

    function sensitiveForm(el) {
      var form = el.closest("form");
      if (!form) return false;
      try {
        return !!form.querySelector(SENSITIVE_FIELDS);
      } catch (e) {
        return true;
      }
    }

    function label(el) {
      if (el.isContentEditable || el.closest("[data-trace-mask],[contenteditable]") || sensitiveForm(el)) return undefined;
      if (HOST_MASK && el.closest(HOST_MASK)) return undefined;
      var t = el.getAttribute("aria-label");
      if (!t && el.matches(TEXT_CONTROLS)) {
        // textContent, not innerText: innerText forces a layout. Large subtrees are skipped.
        if (el.tagName === "INPUT") t = el.value;
        else if (el.childElementCount <= 10) t = el.textContent;
      }
      t = t || el.getAttribute("title") || el.getAttribute("alt") || "";
      t = redact(String(t).slice(0, 300).replace(/\s+/g, " ").trim(), true);
      return t ? t.slice(0, 80) : undefined;
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

    function pageview(force) {
      var path = route();
      if (!force && page && page.path === path) return;
      leave();
      page = { path: path, start: Date.now(), maxScroll: 0, left: false, errors: 0 };
      docHeight = 0;
      // No page title: titles often carry names, order numbers or balances.
      push("pageview", { path: path });
      idle(sampleScroll);
    }

    function autoPageview() {
      if (MANUAL_PAGEVIEWS) return;
      setTimeout(
        safe(function () {
          // A route named with trace.page() lasts until the URL changes.
          if (manualRoute && location.href !== manualHref) manualRoute = null;
          pageview();
        }),
        0
      );
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

    // Same-document navigations. The Navigation API sees every one, whoever
    // calls pushState; patching history covers browsers without it.
    if (window.navigation && navigation.addEventListener) navigation.addEventListener("currententrychange", autoPageview);
    ["pushState", "replaceState"].forEach(function (fn) {
      var original = history[fn];
      if (typeof original !== "function") return;
      history[fn] = function () {
        var result = original.apply(this, arguments);
        autoPageview();
        return result;
      };
    });
    addEventListener("popstate", autoPageview);
    addEventListener("hashchange", autoPageview);

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
    // Multi-page sites: back/forward can restore a page from the bfcache without
    // re-running scripts. That's a new pageview.
    addEventListener(
      "pageshow",
      safe(function (e) {
        if (e.persisted && !MANUAL_PAGEVIEWS) pageview(true);
      })
    );

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

    // Declarative events: data-trace-event / data-trace-props.
    function markupEvent(el, t, path) {
      var name = el.getAttribute("data-trace-event");
      if (!name) return;
      var props;
      try {
        props = JSON.parse(el.getAttribute("data-trace-props") || "null");
      } catch (e) {
        props = undefined;
      }
      push("custom", { t: t, path: path, name: redact(name.slice(0, 128), false), props: cleanProps(props, 0) });
    }
    document.addEventListener(
      "click",
      safe(function (e) {
        var target = e.target;
        var raw = target && target.nodeType === 1 ? target : target && target.parentElement;
        if (!raw) return;
        var t = Date.now();
        var path = route();

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
        var deadCheck = !opensElsewhere && !FORM_FIELD.test(raw.tagName) && watch();
        var m = mutations;
        var s = scrolls;
        var href = location.href;
        var tagged = raw.closest("[data-trace-event]");
        if (tagged && tagged.tagName === "FORM") tagged = null; // forms fire on submit

        idle(function () {
          if (tagged) markupEvent(tagged, t, path);
          var el = raw.closest(INTERACTIVE) || raw;
          var data = { t: t, path: path, sel: selector(el), text: label(el) };
          push("click", data);
          if (rage) push("rage_click", data);
          if (!deadCheck) return;
          setTimeout(
            safe(function () {
              unwatch();
              if (FORM_FIELD.test(el.tagName)) return;
              if (mutations !== m || scrolls !== s || location.href !== href || document.hidden) return;
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
    var startedForms = typeof WeakSet === "function" ? new WeakSet() : null;
    document.addEventListener(
      "focusin",
      safe(function (e) {
        var form = e.target && e.target.form;
        if (!form || !startedForms || startedForms.has(form)) return;
        startedForms.add(form);
        idle(function () {
          push("form_start", { sel: selector(form) });
        });
      }),
      { capture: true, passive: true }
    );
    document.addEventListener(
      "submit",
      safe(function (e) {
        // On multi-page sites a submit navigates away, so send it right away.
        if (e.target && e.target.nodeType === 1) {
          push("submit", { sel: selector(e.target) });
          markupEvent(e.target, Date.now(), route());
        }
        flush();
      }),
      { capture: true, passive: true }
    );

    // ---- errors --------------------------------------------------------------
    // Five kinds, kept in `name`: exception (uncaught), rejection (unhandled
    // promise), console (console.error, where frameworks report the errors they
    // catch), network (fetch/XHR answered >= 400 or failed) and resource (a script,
    // stylesheet or image that didn't load). Messages are scrubbed like all text;
    // network errors keep only the method, path and status, never bodies or queries.
    var reporting = false;
    function error(kind, message, source) {
      if (reporting || (page && page.errors++ >= 20)) return;
      reporting = true;
      try {
        push("error", { name: kind, text: redact(String(message).slice(0, 300), false), sel: source });
      } finally {
        reporting = false;
      }
    }

    // A URL reduced to what's safe and useful: host (if another site) and the
    // path with ids grouped. No query string, no fragment.
    function shortUrl(raw) {
      try {
        var u = new URL(raw, location.href);
        var path = u.pathname.split("/").map(function (seg) {
          return ID_SEGMENT.test(seg) ? ":id" : redactSegment(seg);
        }).join("/");
        return (u.origin === location.origin ? "" : u.host) + path;
      } catch (e) {
        return "";
      }
    }

    // "file:line" of the first stack frame outside the collector.
    function frame(err) {
      var lines = String((err && err.stack) || "").split("\n");
      for (var i = 0; i < lines.length; i++) {
        var m = /((?:https?:\/\/|\/)[^\s()]+?):(\d+)(?::\d+)?\)?\s*$/.exec(lines[i]);
        if (m && m[1].split(/[?#]/)[0] !== SCRIPT_SRC.split(/[?#]/)[0])
          return m[1].replace(location.origin, "").split(/[?#]/)[0] + ":" + m[2];
      }
      return undefined;
    }

    function isOwnRequest(url) {
      return String(url).indexOf(ENDPOINT) === 0;
    }

    function networkError(method, url, status) {
      if (isOwnRequest(url)) return;
      error("network", String(method || "GET").toUpperCase() + " " + shortUrl(url) + " \u2192 " + (status || "failed"));
    }

    addEventListener(
      "error",
      safe(function (e) {
        var t = e.target;
        if (t && t !== window && t.nodeType === 1) {
          // Resource load failures don't bubble, hence the capture-phase listener.
          var url = t.currentSrc || t.src || t.href || "";
          if (url && url !== SCRIPT_SRC) error("resource", "Failed to load " + t.tagName.toLowerCase() + " " + shortUrl(url));
          return;
        }
        if (!e.message || (SCRIPT_SRC && e.filename === SCRIPT_SRC)) return;
        var file = e.filename ? e.filename.replace(location.origin, "").split(/[?#]/)[0] : "";
        error("exception", e.message, file ? file + ":" + e.lineno : undefined);
      }),
      true
    );
    addEventListener(
      "unhandledrejection",
      safe(function (e) {
        var r = e.reason;
        error("rejection", "Unhandled rejection: " + ((r && r.message) || r), frame(r));
      })
    );

    if (CAPTURE_CONSOLE && window.console && typeof console.error === "function") {
      var nativeConsoleError = console.error;
      console.error = function () {
        try {
          var first = arguments[0];
          var message = first instanceof Error ? first.message : typeof first === "string" ? first : null;
          if (message) {
            // Fill printf-style placeholders ("%s") with the string arguments that follow.
            var rest = Array.prototype.slice.call(arguments, 1);
            message = message.replace(/%[sdif]/g, function () {
              var next = rest.shift();
              return typeof next === "string" || typeof next === "number" ? String(next) : "\u2026";
            });
            error("console", message, first instanceof Error ? frame(first) : undefined);
          }
        } catch (e) {}
        return nativeConsoleError.apply(this, arguments);
      };
    }

    if (typeof window.fetch === "function") {
      var nativeFetch = window.fetch;
      window.fetch = function (input, init) {
        var result = nativeFetch.apply(this, arguments);
        try {
          var url = typeof input === "string" ? input : (input && input.url) || String(input);
          var method = (init && init.method) || (input && input.method) || "GET";
          if (!isOwnRequest(url))
            result.then(
              safe(function (res) {
                if (res.status >= 400) networkError(method, url, res.status);
              }),
              safe(function (err) {
                if (!err || err.name !== "AbortError") networkError(method, url, 0);
              })
            );
        } catch (e) {}
        return result;
      };
    }

    if (window.XMLHttpRequest) {
      var xhrOpen = XMLHttpRequest.prototype.open;
      var xhrSend = XMLHttpRequest.prototype.send;
      XMLHttpRequest.prototype.open = function (method, url) {
        try {
          this._dgRequest = [method, url];
        } catch (e) {}
        return xhrOpen.apply(this, arguments);
      };
      XMLHttpRequest.prototype.send = function () {
        try {
          var xhr = this;
          var req = xhr._dgRequest;
          if (req && !isOwnRequest(req[1])) {
            var aborted = false;
            xhr.addEventListener("abort", function () {
              aborted = true;
            });
            xhr.addEventListener(
              "loadend",
              safe(function () {
                if (!aborted && (xhr.status >= 400 || xhr.status === 0)) networkError(req[0], req[1], xhr.status);
              })
            );
          }
        } catch (e) {}
        return xhrSend.apply(this, arguments);
      };
    }

    // ---- public API ------------------------------------------------------------
    api.track = safe(function (name, props) {
      push("custom", { name: redact(String(name).slice(0, 128), false), props: cleanProps(props, 0) });
    });
    // User ids should be opaque. Email-like ids are replaced by a SHA-256 hash,
    // or dropped where the browser can't hash (non-HTTPS pages).
    api.identify = safe(function (id) {
      var value = id == null ? "" : String(id).trim();
      function save(v) {
        uid = v || null;
        set("trc_uid", v || "");
      }
      if (!value || (!/@|%40|\s/.test(value) && value.length <= 128)) return save(value);
      save(null);
      if (!window.crypto || !crypto.subtle || !window.TextEncoder) return;
      crypto.subtle.digest("SHA-256", new TextEncoder().encode(value.toLowerCase())).then(
        safe(function (buf) {
          var hex = "";
          var bytes = new Uint8Array(buf);
          for (var b = 0; b < 16; b++) hex += (bytes[b] < 16 ? "0" : "") + bytes[b].toString(16);
          save("h_" + hex);
        })
      );
    });
    api.page = safe(function (name) {
      manualRoute = name ? String(name).slice(0, 512).split("/").map(redactSegment).join("/") : null;
      manualHref = location.href;
      pageview(true);
    });
    api.flush = safe(flush);
    api.init = function () {};

    if (!MANUAL_PAGEVIEWS) pageview();
  }

  function replay() {
    var calls = pending.splice(0, pending.length);
    for (var c = 0; c < calls.length; c++) {
      var m = calls[c][0];
      if (m !== "init" && m !== "consent" && typeof api[m] === "function") api[m].apply(null, calls[c][1]);
    }
  }

  // ---- consent ---------------------------------------------------------------
  // With consent: "banner" or "required", start() waits for a yes. Until then
  // calls stay queued and nothing is stored, listened to or sent.
  var CONSENT_KEY = "trc_consent";
  var gated = null; // the config waiting for consent
  var bar = null;

  function storedAnswer() {
    try {
      return window.localStorage.getItem(CONSENT_KEY);
    } catch (e) {
      return null;
    }
  }
  function storeAnswer(v) {
    try {
      window.localStorage.setItem(CONSENT_KEY, v);
    } catch (e) {}
  }
  function forget() {
    try {
      var s = window.localStorage;
      ["trc_vid", "trc_sid", "trc_uid", "trc_last"].forEach(function (k) {
        s.removeItem(k);
      });
    } catch (e) {}
  }

  function setConsent(yes) {
    storeAnswer(yes ? "yes" : "no");
    hideBar();
    if (yes) {
      optedOut = false;
      if (gated && !started) {
        start(gated);
        replay();
      }
    } else {
      optedOut = true;
      pending.length = 0;
      forget();
    }
  }

  function hideBar() {
    if (bodyWatcher) {
      bodyWatcher.disconnect();
      bodyWatcher = null;
    }
    if (bar && bar.parentNode) bar.parentNode.removeChild(bar);
    bar = null;
  }

  // Turbo, and every other framework that swaps <body> on navigation, throws
  // the bar out with the old body. A bar that is no longer in the document is
  // no bar, and the watcher puts it back, so a visitor who clicks through
  // before answering is still asked rather than silently never recorded.
  var bodyWatcher = null;
  function barGone() {
    return bar !== null && !bar.isConnected;
  }
  function watchBody() {
    if (bodyWatcher || typeof MutationObserver !== "function") return;
    bodyWatcher = new MutationObserver(
      safe(function () {
        if (barGone()) showBar();
      })
    );
    bodyWatcher.observe(document.documentElement, { childList: true, subtree: true });
  }

  function showBar() {
    if (barGone()) bar = null;
    if (bar || !gated) return;
    if (!document.body) return document.addEventListener("DOMContentLoaded", safe(showBar));
    var c = gated;
    var text =
      c["consent-text"] ||
      c.consentText ||
      "This site uses Trace to see, in aggregate, how its pages are used. If you allow it, Trace keeps a random ID on this device. It never records what you type.";
    var link = c["consent-link"] || c.consentLink;
    var linkText = c["consent-link-text"] || c.consentLinkText || "How it works";
    var allow = c["consent-allow"] || c.consentAllow || "Allow";
    var deny = c["consent-deny"] || c.consentDeny || "No thanks";
    var font = "14px/1.45 -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif";
    bar = document.createElement("div");
    bar.setAttribute("role", "region");
    bar.setAttribute("aria-label", "Privacy choice");
    bar.setAttribute("data-trace-mask", "");
    bar.style.cssText =
      "position:fixed;left:50%;bottom:16px;transform:translateX(-50%);box-sizing:border-box;width:calc(100% - 32px);" +
      "max-width:680px;z-index:2147483000;display:flex;flex-wrap:wrap;align-items:center;gap:10px 16px;" +
      "padding:14px 16px;border-radius:12px;background:#10141d;color:#f3f5f9;box-shadow:0 8px 30px rgba(0,0,0,.3);" +
      "font:" + font + ";text-align:left;";
    var p = document.createElement("span");
    p.style.cssText = "flex:1 1 300px;margin:0;color:inherit;font:inherit;";
    p.textContent = text + " ";
    if (link) {
      var a = document.createElement("a");
      a.href = link;
      a.textContent = linkText;
      a.style.cssText = "color:#8ea3ff;text-decoration:underline;font:inherit;";
      p.appendChild(a);
    }
    bar.appendChild(p);
    var row = document.createElement("span");
    row.style.cssText = "display:flex;gap:8px;flex-wrap:wrap;";
    // The same look for both answers: saying no is as easy as saying yes.
    [[allow, true], [deny, false]].forEach(function (choice) {
      var b = document.createElement("button");
      b.type = "button";
      b.textContent = choice[0];
      b.style.cssText =
        "font:600 " + font + ";margin:0;min-height:40px;padding:8px 18px;border-radius:999px;cursor:pointer;" +
        "border:1.5px solid #f3f5f9;background:transparent;color:#f3f5f9;";
      b.addEventListener(
        "click",
        safe(function () {
          setConsent(choice[1]);
        })
      );
      row.appendChild(b);
    });
    bar.appendChild(row);
    document.body.appendChild(bar);
    watchBody();
  }

  api.consent = safe(function (yes) {
    if (arguments.length === 0) return storedAnswer();
    setConsent(!!yes);
  });

  function begin(config) {
    var mode = config && String(config.consent || "");
    if (!config || !config.key || (mode !== "banner" && mode !== "required")) {
      start(config);
      if (started) replay();
      return;
    }
    gated = config;
    // "Change your privacy choice" links anywhere on the page open the bar.
    document.addEventListener(
      "click",
      safe(function (e) {
        var t = e.target && e.target.closest && e.target.closest("[data-trace-consent]");
        if (!t) return;
        e.preventDefault();
        showBar();
      }),
      true
    );
    // A consent tool may have answered through the stub before this loaded.
    var answer = storedAnswer();
    for (var q = 0; q < pending.length; q++) if (pending[q][0] === "consent") answer = pending[q][1][0] ? "yes" : "no";
    if (answer === "yes") return setConsent(true);
    optedOut = true;
    if (answer === "no" || navigator.globalPrivacyControl === true) {
      pending.length = 0;
      return;
    }
    if (mode === "banner") showBar();
  }

  // Configuration: script attributes, then window.traceConfig, then a queued init().
  var queuedInit = null;
  for (var i = 0; i < pending.length; i++) if (pending[i][0] === "init") queuedInit = pending[i][1][0];
  var initial = scriptConfig() || window.traceConfig || queuedInit;
  if (initial) safe(begin)(initial);
  else {
    api.init = safe(function (config) {
      api.init = function () {};
      begin(config);
    });
    if (!pending.length) console.warn("[trace] waiting for trace.init({ key }) (no data-key on the script tag)");
  }
})();
