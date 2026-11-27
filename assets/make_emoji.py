"""Make assets/emoji.txt, the :shortcode: table of the Markdown preview.

Usage: gh api emojis | python assets/make_emoji.py > assets/emoji.txt

GitHub's emoji API maps each shortcode to an image URL. For a Unicode emoji,
the file name of that URL holds its code points (1f680.png, 1f1fa-1f1f8.png).
GitHub's own images (octocat and others) have no code points and are left out.
Each output line is `name code-points`, sorted by name.
"""
import json
import re
import sys

table = json.load(sys.stdin)
lines = []
for name, url in table.items():
    m = re.search(r"/unicode/([0-9a-f-]+)\.png", url)
    if m:
        lines.append(f"{name} {m.group(1)}")
sys.stdout.write("\n".join(sorted(lines)) + "\n")
