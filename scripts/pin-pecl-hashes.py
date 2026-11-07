#!/usr/bin/env python3
"""Pin SHA-256 hashes of PECL Windows builds for OLS's extension installer.

downloads.php.net publishes no checksums for its Windows PECL builds, so OLS can only
verify a build against a hash it ships. This script is how that list is made: it walks the
same release folders the app walks (stable releases newest first, at most 12), takes the
newest build of each extension for each PHP minor, downloads it over HTTPS, hashes it and
writes crates/ols-core/catalog/pecl-pins.json.

The hashes are "pinned at review time": they prove a later download is the same file the
maintainer reviewed, not that the file is good. Review the list (and, ideally, scan the
builds) before a release.

Usage:  python scripts/pin-pecl-hashes.py [--ext redis --ext apcu ...] [--minor 8.4 ...]
"""

import argparse
import datetime
import hashlib
import json
import pathlib
import re
import sys
import urllib.request

RELEASES = "https://downloads.php.net/~windows/pecl/releases/"
DEFAULT_EXTS = ["apcu", "redis", "igbinary", "imagick", "mongodb", "xdebug", "pcov", "yaml"]
DEFAULT_MINORS = ["8.1", "8.2", "8.3", "8.4", "8.5"]
OUT = pathlib.Path(__file__).resolve().parent.parent / "crates/ols-core/catalog/pecl-pins.json"


def get(url: str) -> bytes:
    req = urllib.request.Request(url, headers={"User-Agent": "OLS-pecl-pin"})
    with urllib.request.urlopen(req, timeout=120) as r:
        if not r.geturl().startswith("https://"):
            raise RuntimeError(f"{url} was redirected off HTTPS to {r.geturl()}")
        return r.read()


def hrefs(html: str):
    return re.findall(r'href="([^"]+)"', html)


def releases_newest_first(html: str):
    def key(v):
        return [int(p) for p in re.split(r"\D+", v) if p]

    def stable(v):
        return all(c.isdigit() or c == "." for c in v)

    names = sorted(
        {h[:-1] for h in hrefs(html) if h.endswith("/") and h[:1].isdigit()},
        key=lambda v: (not stable(v), [-k for k in key(v)]),
    )
    return names


def matching_zip(html: str, name: str, release: str, minor: str, ts: bool):
    prefix = f"php_{name}-{release}-{minor}-{'ts' if ts else 'nts'}-".lower()
    for h in hrefs(html):
        f = h.rsplit("/", 1)[-1]
        if f.lower().startswith(prefix) and f.lower().endswith("-x64.zip"):
            return f
    return None


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--ext", action="append")
    ap.add_argument("--minor", action="append")
    ap.add_argument("--ts", action="store_true", help="also pin thread-safe builds")
    args = ap.parse_args()
    exts = args.ext or DEFAULT_EXTS
    minors = args.minor or DEFAULT_MINORS
    flavours = [False, True] if args.ts else [False]

    existing = json.loads(OUT.read_text()) if OUT.exists() else {}
    builds = dict(existing.get("builds", {}))
    for ext in exts:
        listing = get(f"{RELEASES}{ext}/").decode("utf-8", "replace")
        releases = releases_newest_first(listing)[:12]
        files_by_release = {}
        for minor in minors:
            for ts in flavours:
                for rel in releases:
                    if rel not in files_by_release:
                        files_by_release[rel] = get(f"{RELEASES}{ext}/{rel}/").decode("utf-8", "replace")
                    z = matching_zip(files_by_release[rel], ext, rel, minor, ts)
                    if not z:
                        continue
                    if z not in builds:
                        data = get(f"{RELEASES}{ext}/{rel}/{z}")
                        builds[z] = hashlib.sha256(data).hexdigest()
                        print(f"pinned {z}  {builds[z]}  ({len(data)} bytes)")
                    else:
                        print(f"kept   {z}")
                    break
                else:
                    print(f"none   {ext} for PHP {minor} {'ts' if ts else 'nts'}")
    OUT.write_text(
        json.dumps(
            {
                "note": "SHA-256 of PECL Windows builds, pinned at review time by scripts/pin-pecl-hashes.py. "
                "downloads.php.net publishes no checksums: a hash here proves a download is the file "
                "that was reviewed, not that it is safe. Review before each release.",
                "source": RELEASES,
                "pinned_on": datetime.date.today().isoformat(),
                "reviewed": False,
                "builds": dict(sorted(builds.items())),
            },
            indent=2,
        )
        + "\n"
    )
    print(f"wrote {OUT} ({len(builds)} builds)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
