"""Builds the docs site: one page per file in docs-site/pages, wrapped in docs-site/template.html.

Run from the repository root after editing any docs page:

    python3 scripts/build_docs.py

Each page file starts with a comment giving its place in the docs:

    <!-- order: 2 | slug: install | title: Install | group: Getting started | description: ... -->

Pages are written to site/docs/<slug>/index.html (the empty slug is the docs home). Headings
get ids for linking, the "On this page" list and search.

It also writes what search engines read: site/sitemap.xml, site/robots.txt, structured data on
every docs page, and the landing page's FAQ structured data (from its FAQ, between the
<!-- faq-jsonld --> markers), so they never drift from the pages.
"""

import html
import json
import pathlib
import re

ROOT = pathlib.Path(__file__).resolve().parent.parent
PAGES = ROOT / "docs-site" / "pages"
TEMPLATE = ROOT / "docs-site" / "template.html"
OUT = ROOT / "site" / "docs"
SITE = "https://sayql.dev"
LANDING = ROOT / "site" / "index.html"
META = re.compile(r"<!--\s*(.*?)\s*-->\s*", re.S)
HEADING = re.compile(r"<(h[23])>(.*?)</\1>", re.S)


def text_of(fragment: str) -> str:
    return html.unescape(re.sub(r"<[^>]+>", "", fragment)).strip()


def slugify(text: str) -> str:
    return re.sub(r"[^a-z0-9]+", "-", text.lower()).strip("-")[:60] or "section"


def url_of(slug: str) -> str:
    return f"/docs/{slug}/" if slug else "/docs/"


def load_pages() -> list[dict]:
    pages = []
    for path in sorted(PAGES.glob("*.html")):
        raw = path.read_text()
        m = META.match(raw)
        if not m:
            raise SystemExit(f"{path.name}: missing the <!-- order: … --> header")
        meta = dict(part.split(":", 1) for part in m.group(1).split(" | "))
        meta = {k.strip(): v.strip() for k, v in meta.items()}
        pages.append({**meta, "order": int(meta["order"]), "body": raw[m.end():], "url": url_of(meta["slug"])})
    return sorted(pages, key=lambda p: p["order"])


def add_heading_ids(body: str) -> tuple[str, list[tuple[str, str, str]]]:
    """Gives every h2/h3 a unique id and a hover anchor; returns the body and (level, text, id)."""
    seen: dict[str, int] = {}
    found = []

    def repl(m: re.Match) -> str:
        level, inner = m.group(1), m.group(2)
        text = text_of(inner)
        base = slugify(text)
        seen[base] = seen.get(base, 0) + 1
        hid = base if seen[base] == 1 else f"{base}-{seen[base]}"
        found.append((level, text, hid))
        return f'<{level} id="{hid}"><a class="anchor" href="#{hid}" aria-hidden="true">#</a>{inner}</{level}>'

    return HEADING.sub(repl, body), found


def nav_html(pages: list[dict], current: dict) -> str:
    groups: dict[str, list[dict]] = {}
    for p in pages:
        groups.setdefault(p["group"], []).append(p)
    parts = []
    for name, items in groups.items():
        links = "".join(
            f'<li><a href="{p["url"]}"{" aria-current=\"page\"" if p is current else ""}>{html.escape(p["title"])}</a></li>'
            for p in items
        )
        parts.append(f'<nav class="group" aria-label="{html.escape(name)}"><h2>{html.escape(name)}</h2><ul>{links}</ul></nav>')
    return "\n    ".join(parts)


def onpage_html(headings: list[tuple[str, str, str]]) -> str:
    if not headings:
        return ""
    items = "".join(f'<li><a class="{level}" href="#{hid}">{html.escape(text)}</a></li>' for level, text, hid in headings)
    return f'<h2>On this page</h2><ul>{items}</ul><a class="top" href="#">Back to top ↑</a>'


def pager_html(prev: dict | None, nxt: dict | None) -> str:
    parts = []
    if prev:
        parts.append(f'<a class="prev" href="{prev["url"]}"><small>← Previous</small><b>{html.escape(prev["title"])}</b></a>')
    if nxt:
        parts.append(f'<a class="next" href="{nxt["url"]}"><small>Next →</small><b>{html.escape(nxt["title"])}</b></a>')
    return f'<nav class="pager" aria-label="Previous and next page">{"".join(parts)}</nav>'


def ld(data: dict) -> str:
    return '<script type="application/ld+json">' + json.dumps(data, ensure_ascii=False).replace("</", "<\\/") + "</script>"


def page_jsonld(p: dict) -> str:
    """A TechArticle and its breadcrumb (SayQL › Docs › page) for search results."""
    crumbs = [("SayQL", "/"), ("Docs", "/docs/")] + ([(p["title"], p["url"])] if p["slug"] else [])
    return "\n".join([
        ld({"@context": "https://schema.org", "@type": "TechArticle", "headline": p["title"],
            "description": p["description"], "url": SITE + p["url"], "image": SITE + "/og.png",
            "about": {"@type": "SoftwareApplication", "name": "SayQL", "url": SITE + "/"},
            "publisher": {"@type": "Organization", "name": "SayQL", "logo": {"@type": "ImageObject", "url": SITE + "/icon-512.png"}}}),
        ld({"@context": "https://schema.org", "@type": "BreadcrumbList", "itemListElement": [
            {"@type": "ListItem", "position": i + 1, "name": name, "item": SITE + url} for i, (name, url) in enumerate(crumbs)]}),
    ])


def write_seo(pages: list[dict]) -> None:
    """sitemap.xml, robots.txt, and the landing page's FAQ structured data."""
    urls = ["/"] + [p["url"] for p in pages]
    (ROOT / "site" / "sitemap.xml").write_text(
        '<?xml version="1.0" encoding="UTF-8"?>\n<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">\n'
        + "".join(f"  <url><loc>{SITE}{u}</loc></url>\n" for u in urls) + "</urlset>\n")
    (ROOT / "site" / "robots.txt").write_text(f"User-agent: *\nAllow: /\nDisallow: /download/\n\nSitemap: {SITE}/sitemap.xml\n")

    landing = LANDING.read_text()
    faq = re.search(r'<div class="faq">(.*?)</div>\s*</div>\s*</section>', landing, re.S)
    if not faq:
        raise SystemExit("site/index.html: FAQ not found")
    qa = re.findall(r"<summary>(.*?)<svg.*?</summary>\s*<p>(.*?)</p>", faq.group(1), re.S)
    data = {"@context": "https://schema.org", "@type": "FAQPage", "mainEntity": [
        {"@type": "Question", "name": text_of(q), "acceptedAnswer": {"@type": "Answer", "text": text_of(a)}} for q, a in qa]}
    marked = re.sub(r"<!-- faq-jsonld -->.*?<!-- /faq-jsonld -->",
                    lambda _: "<!-- faq-jsonld -->" + ld(data) + "<!-- /faq-jsonld -->", landing, count=1, flags=re.S)
    if marked == landing and "<!-- faq-jsonld -->" not in landing:
        raise SystemExit("site/index.html: missing the <!-- faq-jsonld --> markers")
    LANDING.write_text(marked)


def main() -> None:
    pages = load_pages()
    template = TEMPLATE.read_text()
    for p in pages:
        p["body"], p["headings"] = add_heading_ids(p["body"])
    index = [
        {"t": p["title"], "d": p["description"], "g": p["group"], "u": p["url"],
         "h": [[text, f'{p["url"]}#{hid}'] for _, text, hid in p["headings"]]}
        for p in pages
    ]
    for i, p in enumerate(pages):
        values = {
            "JSONLD": page_jsonld(p),
            "TITLE": html.escape(p["title"]),
            "DESCRIPTION": html.escape(p["description"]),
            "GROUP": html.escape(p["group"]),
            "URL": p["url"],
            "NAV": nav_html(pages, p),
            "CONTENT": p["body"].strip(),
            "ONPAGE": onpage_html(p["headings"]),
            "PAGER": pager_html(pages[i - 1] if i else None, pages[i + 1] if i + 1 < len(pages) else None),
            "INDEX": json.dumps(index, ensure_ascii=False).replace("</", "<\\/"),
        }
        page = template
        for key, value in values.items():
            page = page.replace("{{" + key + "}}", value)
        left = re.findall(r"\{\{[A-Z]+\}\}", page)
        if left:
            raise SystemExit(f"{p['slug'] or 'home'}: unfilled {left}")
        target = OUT / p["slug"] / "index.html" if p["slug"] else OUT / "index.html"
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(page)
    write_seo(pages)
    print(f"Built {len(pages)} docs pages into {OUT.relative_to(ROOT)}/, plus the sitemap and robots.txt")


if __name__ == "__main__":
    main()
