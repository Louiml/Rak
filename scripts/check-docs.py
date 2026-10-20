#!/usr/bin/env python3
"""Check the documentation set for the mistakes that are easy to make and
hard to see: a link to a page that does not exist, a link to an anchor that
does not exist, a page that is not in the site's manifest, a code fence that
was left open, and a heading that carries a version number.

The site renders the Markdown at run time, so a broken link is invisible in
the source and only shows up as a dead link in the browser. This runs in CI
so that it shows up in the log instead.

    python scripts/check-docs.py
"""

import glob
import os
import re
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
CONTENT = os.path.join(ROOT, "docs", "content")
SCRIPT = os.path.join(ROOT, "docs", "script.js")


def slug(text):
    """The anchor the site's renderer gives a heading."""
    text = text.lower()
    text = re.sub(r"[^a-z0-9\s-]", "", text)
    return re.sub(r"\s+", "-", text.strip())


def manifest():
    """The pages the site knows about, from the manifest in script.js."""
    src = open(SCRIPT, encoding="utf-8").read()
    return re.findall(r"file: '([^']+)', title: '([^']+)'", src)


def main():
    entries = manifest()
    pages = {name for name, _ in entries}
    files = {os.path.basename(p): p for p in glob.glob(os.path.join(CONTENT, "*.md"))}
    problems = []

    for name, _ in entries:
        if name not in files:
            problems.append("the manifest lists a page that does not exist: " + name)
    for name in files:
        if name not in pages:
            problems.append("a page is not in the manifest, so the site cannot reach it: " + name)

    anchors = {}
    contents = {}
    for name, path in files.items():
        src = open(path, encoding="utf-8").read()
        contents[name] = src
        found = set()
        for line in src.split("\n"):
            m = re.match(r"^#{1,3} (.+)$", line)
            if m:
                found.add(slug(m.group(1)))
        anchors[name] = found

    link = re.compile(r"\]\(([^)]+)\)")
    for name, src in contents.items():
        for href in link.findall(src):
            if href.startswith(("http", "mailto:")):
                continue
            m = re.match(r"^([A-Za-z0-9_-]+)\.html(?:#(.+))?$", href)
            if m:
                target = m.group(1) + ".md"
                if target not in pages:
                    problems.append("%s: links to a page the site does not have: %s" % (name, href))
                elif m.group(2) and m.group(2) not in anchors.get(target, set()):
                    problems.append("%s: links to an anchor that does not exist: %s" % (name, href))
            elif href.startswith("#"):
                if href[1:] and href[1:] not in anchors.get(name, set()):
                    problems.append("%s: links to an anchor on this page that does not exist: %s" % (name, href))
            else:
                problems.append("%s: link the site cannot resolve: %s" % (name, href))

        if src.count("```") % 2:
            problems.append("%s: an unclosed code fence" % name)

        for line in src.split("\n"):
            if re.match(r"^#{1,3} .*\(\d+\.\d+", line):
                problems.append(
                    "%s: a heading carries a version number, which puts it in the anchor: %s"
                    % (name, line.strip())
                )

    if problems:
        print("documentation problems:")
        for p in problems:
            print("  - " + p)
        return 1

    print("%d pages, %d links: no problems" % (len(files), sum(len(link.findall(s)) for s in contents.values())))
    return 0


if __name__ == "__main__":
    sys.exit(main())
