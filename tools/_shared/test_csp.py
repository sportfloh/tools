"""Tests for csp.py. Run from the repo root:

    python3 -m unittest discover -s tools/_shared
"""

import os
import re
import unittest

import csp

PAGE = """<!DOCTYPE html>
<html>
<head>
  <meta charset="UTF-8"/>
<script type="module">
import init from '/app-0123456789abcdef.js';
await init();
</script>
  <script src="/external.js"></script>
  <script>console.log('sw');
</script>
</head>
<body></body>
</html>
"""


class CspTest(unittest.TestCase):
    def test_inline_bodies_skip_external_scripts(self):
        bodies = csp.inline_bodies(PAGE, "script")
        self.assertEqual(len(bodies), 2)
        self.assertTrue(bodies[0].startswith("\nimport init"))
        self.assertEqual(bodies[1], "console.log('sw');\n")

    def test_hash_source_matches_known_value(self):
        # echo -n "alert('Hello, world.');" | openssl sha256 -binary | base64
        # (the example from the CSP spec)
        self.assertEqual(
            csp.hash_source("alert('Hello, world.');"),
            "'sha256-qznLcsROx4GACP2dm0UCKCzCG+HiZ1guq6ZZDob/Tng='",
        )

    def test_secure_page_puts_policy_first_in_head(self):
        out = csp.secure_page(PAGE)
        head = re.search(r"<head>\s*(<[^>]*>)", out)
        self.assertIn('http-equiv="Content-Security-Policy"', head.group(1))
        for body in csp.inline_bodies(PAGE, "script"):
            self.assertIn(csp.hash_source(body), head.group(1))
        self.assertIn("'wasm-unsafe-eval'", head.group(1))
        self.assertNotIn("unsafe-inline", out)
        # The rest of the page is untouched.
        self.assertEqual(out.replace("\n  " + head.group(1), "", 1), PAGE)

    def test_secure_page_rejects_unexpected_inline_scripts(self):
        extra = PAGE.replace("</head>", "<script>evil()</script></head>")
        with self.assertRaises(ValueError):
            csp.secure_page(extra)

    def test_inject_refuses_a_second_policy(self):
        once = csp.secure_page(PAGE)
        with self.assertRaises(ValueError):
            csp.inject(once, "default-src 'none'")

    def test_landing_page_meta_matches_its_style(self):
        path = os.path.join(os.path.dirname(__file__), "..", "index.html")
        with open(path, encoding="utf-8") as f:
            html = f.read()
        self.assertTrue(
            csp.meta_tag(csp.landing_policy(html)) in html,
            "tools/index.html: <style> changed; replace its CSP meta with the "
            "output of `python3 tools/_shared/csp.py --landing tools/index.html`",
        )


if __name__ == "__main__":
    unittest.main()
