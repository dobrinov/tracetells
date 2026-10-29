#!/usr/bin/env python3
"""Renders tracetells.com from site/ into OUT_DIR.

site/src/layout.html wraps every page. Pages are site/src/pages/*.html, each
starting with a JSON front matter comment; comparison pages are generated from
site/src/compare.json. Everything else in site/ outside src/ is copied as is.
Also writes sitemap.xml, robots.txt and redirects for old addresses.

    deploy/render-site.py site OUT_DIR
"""
import html, json, os, re, shutil, subprocess, sys

BASE = "https://tracetells.com"
REDIRECTS = {"pricing.html": "/pricing/", "switching.html": "/compare/", "help.html": "/docs/"}
ORG = {
    "@type": "Organization", "@id": BASE + "/#org", "name": "Trace", "alternateName": "tracetells",
    "url": BASE + "/", "logo": BASE + "/apple-touch-icon.png", "email": "hello@tracetells.com",
    "sameAs": ["https://github.com/dobrinov/tracetells"],
}
SOFTWARE = {
    "@type": "SoftwareApplication", "name": "Trace", "url": BASE + "/", "applicationCategory": "BusinessApplication",
    "applicationSubCategory": "Web analytics", "operatingSystem": "Web", "license": "https://www.gnu.org/licenses/agpl-3.0.html",
    "isAccessibleForFree": True, "publisher": {"@id": BASE + "/#org"},
    "description": "Cookieless, open-source analytics for the AI era. Finds dead clicks, rage clicks, errors and drop-offs, hands them to your coding agent over MCP, and checks whether each fix worked.",
    "offers": {"@type": "Offer", "name": "Hosted Free", "price": "0", "priceCurrency": "EUR"},
}
DOCS = [  # sidebar, in order
    ("Start", [("/docs/", "Get started"), ("/docs/install/", "Install the tag"), ("/docs/trace-json/", "trace.json"),
               ("/docs/coding-agent/", "Your coding agent")]),
    ("Run it", [("/docs/privacy-controls/", "Privacy controls"), ("/docs/self-hosting/", "Self-hosting")]),
]
FRONT = re.compile(r"\A<!--\s*(\{.*?\})\s*-->\s*", re.S)


def esc(s):
    return html.escape(s, quote=True)


def text(fragment):
    return re.sub(r"\s+", " ", html.unescape(re.sub(r"<[^>]+>", "", fragment))).strip()


def out_file(out, path):
    if path.endswith(".html") or path.endswith(".xml") or path.endswith(".txt"):
        return os.path.join(out, path.lstrip("/"))
    return os.path.join(out, path.strip("/"), "index.html")


def faq_items(body):
    items = []
    for block in re.findall(r'<div class="faq">(.*?)</div>', body, re.S):
        for q, a in re.findall(r"<summary>(.*?)</summary>\s*(.*?)</details>", block, re.S):
            items.append({"@type": "Question", "name": text(q), "acceptedAnswer": {"@type": "Answer", "text": text(a)}})
    return items


def crumbs_html(crumbs):
    parts = []
    for name, href in crumbs:
        parts.append(f'<a href="{href}">{esc(name)}</a>' if href else f'<span aria-current="page">{esc(name)}</span>')
    return '<nav class="breadcrumb" aria-label="Breadcrumb">' + " / ".join(parts) + "</nav>"


def docs_wrap(path, body):
    nav, flat = [], []
    for group, links in DOCS:
        nav.append(f"<strong>{group}</strong>")
        for href, name in links:
            flat.append((href, name))
            cur = ' aria-current="page"' if href == path else ""
            nav.append(f'<a href="{href}"{cur}>{name}</a>')
    nxt = ""
    hrefs = [h for h, _ in flat]
    if path in hrefs:
        i = hrefs.index(path)
        prev = f'<a href="{flat[i-1][0]}">← {flat[i-1][1]}</a>' if i > 0 else "<span></span>"
        after = f'<a href="{flat[i+1][0]}">{flat[i+1][1]} →</a>' if i + 1 < len(flat) else "<span></span>"
        nxt = f'<nav class="next-page" aria-label="Docs pages">{prev}{after}</nav>'
    return (f'<div class="wrap docs"><nav class="docs-nav" aria-label="Docs">{"".join(nav)}</nav>'
            f'<div class="docs-body prose">{body}{nxt}</div></div>')


def render(layout, meta, body):
    path = meta["path"]
    url = BASE + ("/" + path.lstrip("/") if path != "/" else "/")
    graph = [ORG]
    if "software" in meta.get("schema", []):
        graph.append(SOFTWARE)
    crumbs = meta.get("crumbs")
    if crumbs:
        body = body.replace("{{crumbs}}", crumbs_html(crumbs))
        graph.append({"@type": "BreadcrumbList", "itemListElement": [
            {"@type": "ListItem", "position": i + 1, "name": n, "item": BASE + (h or path)} for i, (n, h) in enumerate(crumbs)]})
    if meta.get("docs"):
        body = docs_wrap(path, body)
    faq = faq_items(body)
    if faq:
        graph.append({"@type": "FAQPage", "mainEntity": faq})
    jsonld = '<script type="application/ld+json">' + json.dumps({"@context": "https://schema.org", "@graph": graph}) + "</script>"
    og = meta.get("og_image", "og.png")
    page = layout
    for key, value in {
        "title": esc(meta["title"]), "description": esc(meta["description"]), "url": url,
        "og_title": esc(meta.get("og_title", meta["title"])), "og_image": BASE + "/" + og,
        "robots": '<meta name="robots" content="noindex">\n' if meta.get("noindex") else "", "jsonld": jsonld,
    }.items():
        page = page.replace("{{" + key + "}}", value)
    page = re.sub(r"\{\{nav:(\w+)\}\}", lambda m: ' aria-current="page"' if m.group(1) == meta.get("nav") else "", page)
    page = page.replace("{{body}}", body)
    left = re.findall(r"\{\{[^}]*\}\}", page)
    if left:
        sys.exit(f"{path}: unfilled {left}")
    return page


def compare_page(tool, all_tools):
    t = tool
    rows = "\n".join(f'<tr><th scope="row">{r[0]}</th><td>{r[1]}</td><td class="us">{r[2]}</td></tr>' for r in t["rows"])
    maps = "\n".join(f"<tr><td>{a}</td><td>{b}</td></tr>" for a, b in t["map"])
    keep = "\n".join(f"<li>{x}</li>" for x in t["keep"])
    pick = "\n".join(f"<li>{x}</li>" for x in t["pick"])
    faq = "\n".join(f"<details><summary>{q}</summary><p>{a}</p></details>" for q, a in t["faq"])
    by_slug = {x["slug"]: x for x in all_tools}
    related = " · ".join(f'<a href="/compare/{s}/">{by_slug[s]["link"]}</a>' for s in t["related"]) + ' · <a href="/compare/">All comparisons</a>'
    extra = "".join(f'<section class="wrap section-sm rule"><h2>{e[0]}</h2><div class="prose">{e[1]}</div></section>' for e in t.get("extra", []))
    n = t["name"]
    body = f"""
<section class="wrap page-head">
{{{{crumbs}}}}
<h1 class="mt">{t['h1']}</h1>
<p class="lede">{t['sub']}</p>
<div class="actions"><a class="btn btn-p" href="/try/" data-trace-event="open_demo" data-trace-props='{{"place":"compare"}}'>Try the demo</a><a class="btn btn-s" href="https://app.tracetells.com/#/signup" data-trace-event="start_free" data-trace-props='{{"place":"compare"}}'>Start free</a></div>
</section>

<section class="wrap" style="padding-bottom: 64px;">
<div class="grid">
<div class="card"><h2 style="font-size: 20px;">Keep {n} if you need</h2><ul class="list-lines mt">{keep}</ul></div>
<div class="card dark"><h2 style="font-size: 20px;">Choose Trace if you want</h2><ul class="list-lines mt">{pick}</ul></div>
</div>
</section>

<section class="band section-sm">
<div class="wrap">
<h2>Side by side</h2>
<div class="table-wrap"><table class="versus">
<thead><tr><th scope="col"></th><th scope="col">{n}</th><th scope="col" class="us">Trace</th></tr></thead>
<tbody>
{rows}
</tbody></table></div>
<p class="tiny muted mt-s">{n} details from its public documentation, checked {t['checked']}. Spot something out of date? <a href="mailto:hello@tracetells.com">Tell us</a>.</p>
</div>
</section>

<section class="wrap section-sm">
<h2>Moving from {n}: what maps to what</h2>
<p class="sub">{t['map_intro']}</p>
<div class="table-wrap"><table style="min-width: 560px;">
<thead><tr><th scope="col">In {n}</th><th scope="col">In Trace</th></tr></thead>
<tbody>
{maps}
</tbody></table></div>
</section>
{extra}
<section class="wrap section-sm rule">
<h2>Run them side by side for a month</h2>
<div class="steps">
<div><span class="k">DAY 1</span><p>Add the Trace tag next to {n}. It adds 8.3 KB and does its work when the browser is idle.</p></div>
<div><span class="k">WEEK 1</span><p>{t['week1']}</p></div>
<div><span class="k">WEEK 4</span><p>{t['week4']}</p></div>
</div>
</section>

<section class="rule">
<div class="wrap section-sm">
<h2>Questions from {n} users</h2>
<div class="faq">
{faq}
</div>
<p class="mt muted">Related: {related}</p>
</div>
</section>
"""
    meta = {"path": f"/compare/{t['slug']}/", "title": t["title"], "description": t["description"], "nav": "compare",
            "og_image": f"og/compare-{t['slug']}.png", "og_title": t["og_title"],
            "crumbs": [["Compare", "/compare/"], [t["link"], None]]}
    return meta, body


def lastmod(src):
    try:
        out = subprocess.run(["git", "log", "-1", "--format=%cs", "--", src], capture_output=True, text=True, check=True).stdout.strip()
        return out or None
    except Exception:
        return None


def main():
    src_root, out = sys.argv[1], sys.argv[2]
    src = os.path.join(src_root, "src")
    os.makedirs(out, exist_ok=True)
    for name in os.listdir(src_root):
        if name == "src":
            continue
        a, b = os.path.join(src_root, name), os.path.join(out, name)
        if os.path.isdir(a):
            shutil.copytree(a, b, dirs_exist_ok=True)
        else:
            shutil.copy2(a, b)
    layout = open(os.path.join(src, "layout.html")).read()
    pages = []
    for name in sorted(os.listdir(os.path.join(src, "pages"))):
        f = os.path.join(src, "pages", name)
        raw = open(f).read()
        m = FRONT.match(raw)
        if not m:
            sys.exit(f"{name}: no front matter")
        pages.append((json.loads(m.group(1)), raw[m.end():], f))
    tools = json.load(open(os.path.join(src, "compare.json")))
    for t in tools:
        meta, body = compare_page(t, tools)
        pages.append((meta, body, os.path.join(src, "compare.json")))
    seen = set()
    urls = []
    for meta, body, f in pages:
        if meta["path"] in seen:
            sys.exit(f"two pages at {meta['path']}")
        seen.add(meta["path"])
        dest = out_file(out, meta["path"])
        os.makedirs(os.path.dirname(dest), exist_ok=True)
        open(dest, "w").write(render(layout, meta, body))
        if not meta.get("noindex"):
            urls.append((BASE + ("/" + meta["path"].lstrip("/") if meta["path"] != "/" else "/"), lastmod(f)))
    for old, new in REDIRECTS.items():
        open(os.path.join(out, old), "w").write(
            f'<!doctype html><html lang="en"><head><meta charset="utf-8"><title>Moved</title>'
            f'<link rel="canonical" href="{BASE}{new}"><meta name="robots" content="noindex">'
            f'<meta http-equiv="refresh" content="0; url={new}"></head>'
            f'<body><p>This page moved to <a href="{new}">{BASE}{new}</a>.</p></body></html>\n')
    entries = "\n".join(f"  <url><loc>{u}</loc>" + (f"<lastmod>{d}</lastmod>" if d else "") + "</url>" for u, d in sorted(urls))
    open(os.path.join(out, "sitemap.xml"), "w").write(
        f'<?xml version="1.0" encoding="UTF-8"?>\n<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">\n{entries}\n</urlset>\n')
    open(os.path.join(out, "robots.txt"), "w").write(f"User-agent: *\nAllow: /\n\nSitemap: {BASE}/sitemap.xml\n")
    print(f"rendered {len(pages)} pages, {len(REDIRECTS)} redirects")


if __name__ == "__main__":
    main()
