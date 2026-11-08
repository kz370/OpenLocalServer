#!/usr/bin/env python3
"""Maintainer tool: rebuilds crates/ols-core/catalog/eol.json from https://endoflife.date.

Run before each release:  python scripts/update-eol.py
The app never fetches this data at runtime; the file is embedded in the binary.
"""
import json
import datetime
import os
import urllib.request

PRODUCTS = ["php", "nodejs", "python", "mariadb", "postgresql", "mongodb", "redis", "nginx"]
OUT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "crates", "ols-core", "catalog", "eol.json")


def date_or_none(value):
    # endoflife.date uses a date string, or false/true when no date is known.
    return value if isinstance(value, str) else None


def fetch(product):
    url = f"https://endoflife.date/api/{product}.json"
    req = urllib.request.Request(url, headers={"User-Agent": "OpenLocalServer-update-eol"})
    with urllib.request.urlopen(req, timeout=20) as resp:
        return json.load(resp)


def cycle_key(item):
    return [int(p) if p.isdigit() else 0 for p in item["cycle"].split(".")]


def main():
    products = {}
    for product in PRODUCTS:
        entries = []
        for e in fetch(product):
            security = date_or_none(e.get("support")) if "support" in e else None
            # `support` is the end of active support; the security-fix window ends at `eol`.
            entries.append(
                {
                    "cycle": str(e["cycle"]),
                    "active_until": security,
                    "eol": date_or_none(e.get("eol")),
                }
            )
        entries.sort(key=cycle_key)
        products[product] = entries
    data = {
        "note": "End-of-life dates from https://endoflife.date, copied by scripts/update-eol.py. Refresh before each release.",
        "updated": datetime.date.today().isoformat(),
        "products": products,
    }
    with open(OUT, "w", encoding="utf-8", newline="\n") as f:
        json.dump(data, f, indent=2)
        f.write("\n")
    print("wrote", os.path.normpath(OUT))


if __name__ == "__main__":
    main()
