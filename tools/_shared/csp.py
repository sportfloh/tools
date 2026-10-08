#!/usr/bin/env python3
"""Content-Security-Policy for the tools (GitHub Pages cannot send headers).

As a Trunk post_build hook (no arguments, see each tool's Trunk.toml) it
adds a <meta http-equiv="Content-Security-Policy"> to the staged
index.html of a release build. The page's two inline scripts (Trunk's
module bootstrap, whose text contains the hashed file names, and the
inlined register-sw.js) are allowed by their sha256 hashes, so no
'unsafe-inline' is needed. Any other number of inline scripts fails the
build rather than shipping a page without (or with a broken) policy.

    python3 csp.py --landing ../index.html

prints the meta tag the static landing page must carry for its current
<style> block (test_csp.py checks it in CI).
"""

import base64
import hashlib
import os
import re
import sys

EXPECTED_INLINE_SCRIPTS = 2

_INLINE = r"<{tag}\b(?P<attrs>[^>]*)>(?P<body>.*?)</{tag}>"
_META = re.compile(r'<meta\s+http-equiv="Content-Security-Policy"', re.IGNORECASE)


def inline_bodies(html: str, tag: str) -> list[str]:
    """Text of every <tag>…</tag> without a src attribute, in document order."""
    pattern = re.compile(_INLINE.format(tag=tag), re.DOTALL | re.IGNORECASE)
    return [
        m.group("body")
        for m in pattern.finditer(html)
        if not re.search(r"\bsrc\s*=", m.group("attrs"), re.IGNORECASE)
    ]


def hash_source(body: str) -> str:
    """CSP hash source for an inline element's exact text."""
    digest = hashlib.sha256(body.encode("utf-8")).digest()
    return "'sha256-" + base64.b64encode(digest).decode("ascii") + "'"


def tool_policy(script_hashes: list[str]) -> str:
    # 'wasm-unsafe-eval' lets the page compile its .wasm; connect-src covers
    # fetching it and the service worker's fetches.
    return "; ".join([
        "default-src 'self'",
        "script-src 'self' " + " ".join(script_hashes) + " 'wasm-unsafe-eval'",
        "style-src 'self'",
        "img-src 'self'",
        "connect-src 'self'",
        "worker-src 'self'",
        "manifest-src 'self'",
        "object-src 'none'",
        "base-uri 'self'",
        "form-action 'self'",
    ])


def landing_policy(html: str) -> str:
    styles = [hash_source(b) for b in inline_bodies(html, "style")]
    return "; ".join([
        "default-src 'self'",
        "style-src " + " ".join(styles),
        "img-src 'self'",
        "object-src 'none'",
        "base-uri 'self'",
        "form-action 'self'",
    ])


def meta_tag(policy: str) -> str:
    return f'<meta http-equiv="Content-Security-Policy" content="{policy}"/>'


def inject(html: str, policy: str) -> str:
    """Insert the policy as the first element of <head>."""
    if _META.search(html):
        raise ValueError("page already has a Content-Security-Policy meta tag")
    head = re.search(r"<head\b[^>]*>", html, re.IGNORECASE)
    if head is None:
        raise ValueError("no <head> element")
    end = head.end()
    return html[:end] + "\n  " + meta_tag(policy) + html[end:]


def secure_page(html: str) -> str:
    scripts = inline_bodies(html, "script")
    if len(scripts) != EXPECTED_INLINE_SCRIPTS:
        raise ValueError(
            f"expected {EXPECTED_INLINE_SCRIPTS} inline scripts, found {len(scripts)}; "
            "update csp.py if the page legitimately changed"
        )
    return inject(html, tool_policy([hash_source(s) for s in scripts]))


def main(argv: list[str]) -> int:
    if argv[1:2] == ["--landing"]:
        with open(argv[2], encoding="utf-8") as f:
            print(meta_tag(landing_policy(f.read())))
        return 0
    # `trunk serve` injects its live-reload script and websocket; only
    # release builds (what deploy.yml ships) get the policy.
    if os.environ.get("TRUNK_PROFILE") != "release":
        print("csp.py: not a release build, no CSP added")
        return 0
    path = os.path.join(os.environ["TRUNK_STAGING_DIR"], "index.html")
    with open(path, encoding="utf-8") as f:
        html = f.read()
    try:
        html = secure_page(html)
    except ValueError as e:
        print(f"csp.py: {path}: {e}", file=sys.stderr)
        return 1
    with open(path, "w", encoding="utf-8") as f:
        f.write(html)
    print(f"csp.py: added CSP to {path}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
