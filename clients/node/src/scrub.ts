// The same scrubbing Trace's server does (crates/trace-core/src/privacy.rs and
// errors.rs), done here first so nothing personal leaves the server.
// clients/test-vectors.json holds the cases every client must agree on.

const EMAIL = /[a-z0-9._%+-]+(@|%40)[a-z0-9.-]+\.[a-z]{2,}/gi;
const JWT = /\beyJ[\w-]+\.[\w-]+\.[\w-]+/g;
const URL_QUERY = /(https?:\/\/[^\s?#]+)[?#][^\s]*/g;
const IBAN = /\b[A-Z]{2}\d{2}(?:\s?[A-Z0-9]{4}){2,7}(?:\s?[A-Z0-9]{1,4})?\b/g;
const CARD = /\b(?:\d[ -]?){12,18}\d\b/g;
const TOKEN = /[A-Za-z0-9_\-+/=]{20,}/g;
const LONG_NUMBER = /\+?\d(?:[\s().-]{0,2}\d){6,}/g;

const isToken = (s: string) => /\d/.test(s) && /[A-Za-z]/.test(s);

/** On one line: control and invisible characters become spaces, backticks quotes. */
export function oneLine(s: string): string {
  const hidden = new RegExp("[\\u0000-\\u001f\\u007f-\\u009f\\u00ad\\u200b-\\u200f\\u2028-\\u202e\\u2060-\\u2069\\ufeff]", "g");
  return s.replace(hidden, " ").replace(/`/g, "'").replace(/ {2,}/g, " ").trim();
}

export function truncate(s: string, max: number): string {
  const chars = Array.from(s);
  return chars.length > max ? chars.slice(0, max).join("") : s;
}

/** Emails, tokens, card numbers, IBANs, phone-like numbers and URL query strings. */
export function message(s: string): string {
  return oneLine(s)
    .replace(URL_QUERY, "$1?[redacted]")
    .replace(EMAIL, "[email]")
    .replace(JWT, "[token]")
    .replace(IBAN, "[iban]")
    .replace(CARD, "[card]")
    .replace(TOKEN, (m) => (isToken(m) ? "[token]" : m))
    .replace(LONG_NUMBER, "[number]");
}

/** A route as Trace stores it: no query string, segments that look like ids,
 * emails or tokens replaced. Route parameters (`:id`, `{id}`) pass through. */
export function route(path: string): string {
  const p = oneLine(path.trim()).replace(/ /g, "%20").split(/[?#]/)[0];
  if (!p.startsWith("/")) return "";
  const out = p
    .split("/")
    .map((seg) => {
      if (seg.length > 2 && seg.startsWith("{") && seg.endsWith("}")) return `:${seg.slice(1, -1).replace(/^\*/, "")}`;
      if (!seg || seg.startsWith(":") || seg === "*") return seg;
      if (seg.includes("@") || seg.toLowerCase().includes("%40")) return ":email";
      if (seg.includes("=") || ((seg.match(/\./g) || []).length >= 2 && seg.length >= 24)) return ":token";
      if ((seg.match(/\d/g) || []).length >= 3 || (seg.length >= 16 && isToken(seg)) || (seg.length >= 16 && /^[0-9a-fA-F]+$/.test(seg))) return ":id";
      return seg;
    })
    .join("/");
  return truncate(out, 200);
}

const DEPENDENCY_ROOTS = ["node_modules/", "gems/", "registry/src/", "site-packages/", "vendor/"];
const PROJECT_ROOTS = ["src/", "app/", "lib/", "crates/", "packages/", "config/", "bin/", "spec/", "test/", "tests/"];

/** A frame's file, relative to the project or the dependency it is in. */
export function file(name: string): string {
  let f = oneLine(name).replace(/\\/g, "/");
  if (f.startsWith("file://")) f = f.slice(7);
  f = f.split(/[?#]/)[0];
  const absolute = f.startsWith("/") || f[1] === ":" || f.startsWith("~/");
  let rel: string;
  if (!absolute) {
    rel = f;
  } else {
    let dep: [number, string] | null = null;
    for (const m of DEPENDENCY_ROOTS) {
      const i = f.lastIndexOf(m);
      if (i >= 0 && (!dep || i > dep[0])) dep = [i, m];
    }
    const project = PROJECT_ROOTS.map((m) => f.indexOf(`/${m}`)).find((i) => i >= 0);
    if (dep) {
      const [i, m] = dep;
      if (m === "registry/src/") {
        const rest = f.slice(i + m.length);
        const slash = rest.indexOf("/");
        rel = slash >= 0 ? rest.slice(slash + 1) : rest;
      } else {
        rel = f.slice(i);
      }
    } else if (project !== undefined) {
      rel = f.slice(project + 1);
    } else {
      rel = f.split("/").pop() || "";
    }
  }
  return truncate(
    rel
      .split("/")
      .map((seg) => (seg.includes("@") ? ":email" : seg))
      .join("/"),
    200
  );
}
