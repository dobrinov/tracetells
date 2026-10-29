// Report a Node.js server's errors to Trace (https://tracetells.com).
//
//   import * as trace from "@tracetells/node";
//   trace.init(); // TRACE_SERVER_KEY, TRACE_ENDPOINT, TRACE_ENVIRONMENT, TRACE_RELEASE
//   app.use(trace.expressErrorHandler()); // after your routes
//
// What is sent: the error's class, its message, the stack (files relative to
// your project, lines, functions), the route template, the HTTP method and
// status, the environment, the release, the runtime, when and how often.
// Never sent: request or response bodies, parameters, query strings, headers,
// cookies, environment variables, source code, host names or who was signed
// in. There is no option to send them. Messages and paths are scrubbed first.

import * as scrub from "./scrub.js";

export { scrub };

export interface Config {
  /** The website's server key (tse_…). Without one, the client does nothing. */
  key?: string;
  /** Trace's address. Default: https://app.tracetells.com */
  endpoint?: string;
  /** Default: NODE_ENV, else "production". */
  environment?: string;
  /** Default: a commit id the platform sets, if any. */
  release?: string;
}

/** Where an error happened, when it happened during an HTTP request. */
export interface RequestInfo {
  /** The route template, e.g. /orders/:id. A raw path is scrubbed of ids. */
  route?: string;
  method?: string;
  status?: number;
}

interface Frame {
  file: string;
  line?: number;
  function: string;
  in_app: boolean;
}

interface ErrorEvent {
  class: string;
  message: string;
  backtrace: Frame[];
  route: string;
  method: string;
  status?: number;
  occurred_at: number;
  count: number;
}

const QUEUE = 1000;
const BATCH = 100;
const FOLD_MS = 60_000;
const SEND_EVERY_MS = 1000;

let config: Required<Config> | null = null;
let queue: ErrorEvent[] = [];
const recent = new Map<string, { start: number; repeats: number; latest?: ErrorEvent }>();
let timer: ReturnType<typeof setInterval> | null = null;
let pausedUntil = 0;
let backoff = 5000;
let sending: Promise<void> = Promise.resolve();

const env = (k: string) => {
  const v = typeof process !== "undefined" ? process.env[k] : undefined;
  return v && v.trim() ? v.trim() : undefined;
};

/** Starts the client and reports uncaught exceptions and unhandled rejections.
 * Without a key it does nothing. Calling it again has no effect. */
export function init(options: Config = {}): void {
  if (config) return;
  const key = options.key ?? env("TRACE_SERVER_KEY");
  if (!key) return;
  config = {
    key,
    endpoint: (options.endpoint ?? env("TRACE_ENDPOINT") ?? "https://app.tracetells.com").replace(/\/+$/, ""),
    environment: options.environment ?? env("TRACE_ENVIRONMENT") ?? env("NODE_ENV") ?? "production",
    release:
      options.release ??
      ["TRACE_RELEASE", "SOURCE_VERSION", "HEROKU_SLUG_COMMIT", "RENDER_GIT_COMMIT", "VERCEL_GIT_COMMIT_SHA", "GIT_COMMIT", "GITHUB_SHA"].map(env).find(Boolean) ??
      "",
  };
  timer = setInterval(tick, SEND_EVERY_MS);
  timer.unref?.();
  process.on("uncaughtException", onFatal);
  process.on("unhandledRejection", onRejection);
  process.on("beforeExit", () => void flush(2000));
}

/** Stops the client: sends what's queued and removes its process listeners. */
export async function close(timeoutMs = 2000): Promise<void> {
  await flush(timeoutMs);
  if (timer) clearInterval(timer);
  process.off("uncaughtException", onFatal);
  process.off("unhandledRejection", onRejection);
  config = null;
}

// Node's own behaviour without the client is to print the error and exit with
// code 1. With other listeners the app has decided what happens; otherwise
// the client sends the error (for up to two seconds) and does the same.
async function onFatal(err: unknown) {
  report(err, undefined, "uncaughtException");
  await flush(2000);
  if (process.listenerCount("uncaughtException") <= 1) {
    console.error(err);
    process.exit(1);
  }
}

async function onRejection(reason: unknown) {
  report(reason, undefined, "unhandledRejection");
  await flush(2000);
  if (process.listenerCount("unhandledRejection") <= 1 && process.listenerCount("uncaughtException") <= 1) {
    console.error(reason);
    process.exit(1);
  }
}

/** Reports an error you handled. */
export function report(err: unknown, request?: RequestInfo, fallbackClass = "Error"): void {
  if (!config) return;
  try {
    const e = toEvent(err, request, fallbackClass);
    const key = foldKey(e);
    const seen = recent.get(key);
    const now = Date.now();
    if (seen && now - seen.start < FOLD_MS) {
      seen.repeats++;
      seen.latest = e;
      return;
    }
    recent.set(key, { start: now, repeats: 0 });
    if (queue.length < QUEUE) queue.push(e);
  } catch {
    // Never break the app.
  }
}

/** Reports something that went wrong without an error value. */
export function reportMessage(className: string, message: string, request?: RequestInfo): void {
  report({ name: className, message, stack: new Error().stack } as Error, request);
}

function toEvent(err: unknown, request: RequestInfo | undefined, fallbackClass: string): ErrorEvent {
  const e = err instanceof Error ? err : null;
  const cls = e ? (e.constructor && e.constructor.name !== "Object" ? e.constructor.name : e.name) || fallbackClass : fallbackClass;
  let msg = e ? e.message : typeof err === "string" ? err : safeString(err);
  let cause = e && (e as { cause?: unknown }).cause;
  for (let depth = 0; cause && depth < 3; depth++) {
    msg += `: ${cause instanceof Error ? cause.message : safeString(cause)}`;
    cause = cause instanceof Error ? (cause as { cause?: unknown }).cause : null;
  }
  const status = request?.status && request.status >= 100 && request.status <= 599 ? request.status : undefined;
  return {
    class: scrub.truncate(scrub.oneLine(cls), 200),
    message: scrub.truncate(scrub.message(msg), 300),
    backtrace: frames(e?.stack ?? (err as { stack?: string })?.stack ?? ""),
    route: request?.route ? scrub.route(request.route) : "",
    method: (request?.method ?? "").toUpperCase(),
    status,
    occurred_at: Date.now(),
    count: 1,
  };
}

function safeString(v: unknown): string {
  try {
    return typeof v === "object" ? "non-error value thrown" : String(v);
  } catch {
    return "unknown";
  }
}

/** V8 stack lines: `    at fn (/path/file.js:12:5)` or `    at /path/file.js:12:5`. */
function frames(stack: string): Frame[] {
  const out: Frame[] = [];
  for (const line of stack.split("\n")) {
    const m = /^\s*at (?:(.+?) \()?(.+?):(\d+):\d+\)?$/.exec(line);
    if (!m) continue;
    const [, fn = "", file, ln] = m;
    if (file.includes("@tracetells/node") || file.includes("/clients/node/dist/")) continue;
    const internal = file.startsWith("node:") || file.startsWith("internal/");
    out.push({
      file: scrub.file(file),
      line: Number(ln),
      function: scrub.truncate(scrub.oneLine(fn.replace(/^async /, "")), 200),
      in_app: !internal && !file.includes("node_modules/"),
    });
    if (out.length >= 40) break;
  }
  return out;
}

function foldKey(e: ErrorEvent): string {
  const top = e.backtrace.find((f) => f.in_app);
  return [e.class, top ? `${top.file}|${top.function}` : e.message, e.route, e.status ?? ""].join("|");
}

/** Sends what's queued now, waiting at most `timeoutMs`. */
export async function flush(timeoutMs = 2000): Promise<void> {
  if (!config) return;
  foldExpired(true);
  const done = send();
  await Promise.race([done, new Promise((r) => setTimeout(r, timeoutMs).unref?.())]);
}

function tick() {
  foldExpired(false);
  if (queue.length) void send();
}

// Minutes that ended: their repeats go out as one error with a count.
function foldExpired(all: boolean) {
  const now = Date.now();
  for (const [key, r] of recent) {
    const ended = now - r.start >= FOLD_MS;
    if ((ended || all) && r.latest && r.repeats > 0) {
      if (queue.length < QUEUE) queue.push({ ...r.latest, count: r.repeats });
      r.repeats = 0;
      r.latest = undefined;
    }
    if (ended) recent.delete(key);
  }
}

function send(): Promise<void> {
  sending = sending.then(async () => {
    const cfg = config;
    if (!cfg) return;
    while (queue.length) {
      const batch = queue.splice(0, BATCH);
      if (Date.now() < pausedUntil) continue; // backing off: drop rather than pile up
      const runtime = (process.versions as Record<string, string | undefined>).bun
        ? { name: "bun", version: (process.versions as Record<string, string>).bun }
        : { name: "node", version: process.versions.node };
      const body = JSON.stringify({ errors: batch.map((e) => ({ ...e, environment: cfg.environment, release: cfg.release, runtime })) });
      try {
        const res = await fetch(`${cfg.endpoint}/v1/errors`, {
          method: "POST",
          headers: { authorization: `Bearer ${cfg.key}`, "content-type": "application/json" },
          body,
          signal: AbortSignal.timeout(5000),
        });
        if (res.status === 429 || res.status >= 500) throw new Error(`HTTP ${res.status}`);
        backoff = 5000;
      } catch {
        pausedUntil = Date.now() + backoff;
        backoff = Math.min(backoff * 2, 300_000);
      }
    }
  });
  return sending;
}

// ---- frameworks -------------------------------------------------------------

type Req = { method?: string; url?: string; originalUrl?: string; baseUrl?: string; route?: { path?: unknown } };

/** Express: add after your routes, `app.use(expressErrorHandler())`. It reports
 * and passes the error on, so your own error handling is unchanged. */
export function expressErrorHandler() {
  return function traceErrorHandler(err: unknown, req: Req, res: { statusCode?: number }, next: (err?: unknown) => void) {
    const e = err as { status?: number; statusCode?: number };
    const status = e?.status ?? e?.statusCode ?? (res.statusCode && res.statusCode >= 400 ? res.statusCode : 500);
    if (status >= 500) {
      const routePath = typeof req.route?.path === "string" ? `${req.baseUrl ?? ""}${req.route.path}` : (req.originalUrl ?? req.url ?? "");
      report(err, { route: routePath, method: req.method, status });
    }
    next(err);
  };
}

/** Fastify: `await app.register(fastifyPlugin)`. Reports errors with a 5xx status. */
export async function fastifyPlugin(app: { addHook: (name: "onError", hook: (req: any, reply: any, err: any) => Promise<void>) => void }) {
  app.addHook("onError", async (request, reply, error) => {
    const status = (error && (error.statusCode as number)) || (reply.statusCode >= 400 ? reply.statusCode : 500);
    if (status >= 500) report(error, { route: request.routeOptions?.url ?? request.url, method: request.method, status });
  });
}
// Not encapsulated: the hook applies to every route.
(fastifyPlugin as unknown as Record<symbol, boolean>)[Symbol.for("skip-override")] = true;

/** Plain `http` handlers: `http.createServer(wrapHandler(async (req, res) => …))`.
 * A thrown error is reported and answered with 500 when nothing was sent yet. */
export function wrapHandler<A extends Req, B extends { headersSent?: boolean; statusCode?: number; end: (s?: string) => void }>(
  handler: (req: A, res: B) => unknown,
  route?: (req: A) => string
) {
  return async (req: A, res: B) => {
    try {
      await handler(req, res);
    } catch (err) {
      report(err, { route: route ? route(req) : (req.url ?? ""), method: req.method, status: 500 });
      if (!res.headersSent) {
        res.statusCode = 500;
        res.end("Internal Server Error");
      }
    }
  };
}
