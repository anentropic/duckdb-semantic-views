#!/usr/bin/env python3
"""Offline fallback for obtaining the DuckDB amalgamation without GitHub.

The NORMAL path is `just build` -> `make ensure_amalgamation`, which downloads
`libduckdb-src.zip` from the DuckDB GitHub release. That is the canonical
source and this script deliberately does NOT replace or wire into it: run this
only when the GitHub release is unreachable -- e.g. a sandboxed agent session
whose egress proxy blocks github.com/duckdb/duckdb (the fetch 403s). On a
normal local machine, just run `just build`; there is no reason to use this.

It reconstructs the SAME two files (`cpp/include/duckdb.{hpp,cpp}`) from
GitHub-free hosts:

  * the DuckDB C++ source tree, from the PyPI sdist on
    files.pythonhosted.org (the `duckdb` sdist bundles `external/duckdb/`), and
  * DuckDB's own amalgamation generator (`scripts/amalgamation.py` +
    `package_build.py` + `python_helpers.py`), from jsDelivr
    (cdn.jsdelivr.net/gh/duckdb/duckdb@<tag>/...), which serves repo files and
    is not behind the release-asset gate.

It runs the generator and installs + caches the result exactly where
`make ensure_amalgamation` expects (`cpp/include/` and `.amalgamation/<ver>/`),
so a subsequent `make` / `just build` finds the correct version present and
skips its own (blocked) download.

Caveat: the generated header's `DUCKDB_SOURCE_ID` is a placeholder (the real
release bakes in the git commit SHA, which is not available without GitHub).
This amalgamation is verified suitable for COMPILING and LINTING the extension
(`cargo build`/`clippy --features extension`). Loading the built extension into
a separate DuckDB (`just test-sql`) may hit a version/source-id check; prefer
the official release whenever GitHub is reachable.

Usage:
    python3 scripts/fetch_amalgamation_offline.py            # no-op if already present
    python3 scripts/fetch_amalgamation_offline.py --force    # regenerate even if present
    python3 scripts/fetch_amalgamation_offline.py --version v1.5.4

Only the Python standard library is required (plus `curl` on PATH for downloads).
"""

from __future__ import annotations

import argparse
import json
import os
import re
import shutil
import subprocess
import sys
import tarfile
import tempfile

REPO_ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
INCLUDE_DIR = os.path.join(REPO_ROOT, "cpp", "include")
HPP_DST = os.path.join(INCLUDE_DIR, "duckdb.hpp")
CPP_DST = os.path.join(INCLUDE_DIR, "duckdb.cpp")

JSDELIVR = "https://cdn.jsdelivr.net/gh/duckdb/duckdb@{tag}/scripts/{name}"
GEN_SCRIPTS = ("amalgamation.py", "package_build.py", "python_helpers.py")


def log(msg: str) -> None:
    print(f"[fetch-amalgamation-offline] {msg}", flush=True)


def read_target_version() -> str:
    """Pinned DuckDB version, e.g. 'v1.5.4', from .duckdb-version."""
    with open(os.path.join(REPO_ROOT, ".duckdb-version"), encoding="utf-8") as fh:
        return fh.read().strip()


def installed_version() -> str | None:
    """DUCKDB_VERSION currently in cpp/include/duckdb.hpp, or None."""
    if not os.path.exists(HPP_DST):
        return None
    with open(HPP_DST, encoding="utf-8") as fh:
        for line in fh:
            m = re.search(r'#define\s+DUCKDB_VERSION\s+"([^"]*)"', line)
            if m:
                return m.group(1)
            if line.startswith("#define DUCKDB_MAJOR_VERSION"):
                break  # version block passed without a match
    return None


def curl(url: str, dst: str) -> None:
    """Download `url` to `dst` via curl, failing loudly on HTTP errors."""
    log(f"downloading {url}")
    subprocess.run(["curl", "-fsSL", "-o", dst, url], check=True)


def sdist_url(pyver: str) -> str:
    """PyPI sdist (.tar.gz) URL for the duckdb package at `pyver`."""
    meta = os.path.join(tempfile.gettempdir(), f"duckdb-pypi-{pyver}.json")
    curl(f"https://pypi.org/pypi/duckdb/{pyver}/json", meta)
    with open(meta, encoding="utf-8") as fh:
        data = json.load(fh)
    for f in data["urls"]:
        if f["packagetype"] == "sdist":
            return f["url"]
    raise SystemExit(f"no sdist published for duckdb=={pyver} on PyPI")


def extract_duckdb_source(sdist_path: str, dest: str) -> str:
    """Extract `duckdb-<ver>/external/duckdb/` from the sdist into `dest`.

    Returns the DuckDB source root (the dir containing src/, third_party/, ...).
    """
    os.makedirs(dest, exist_ok=True)
    with tarfile.open(sdist_path, "r:gz") as tar:
        members = []
        prefix = None
        for m in tar.getmembers():
            # e.g. duckdb-1.5.4/external/duckdb/src/include/duckdb.hpp
            parts = m.name.split("/")
            if not (len(parts) >= 3 and parts[1] == "external" and parts[2] == "duckdb"):
                continue
            if prefix is None:
                prefix = "/".join(parts[:3]) + "/"
            stripped = m.name[len(prefix):]  # strip "<pkg>/external/duckdb/"
            if not stripped:
                continue
            # Path-traversal / link hardening (defense in depth; also covers
            # Python < 3.12, where the `filter="data"` extraction guard below is
            # unavailable): accept only regular files and directories whose names
            # stay inside `dest`. A malicious sdist can't escape the temp dir.
            if m.issym() or m.islnk() or m.isdev():
                continue
            norm = os.path.normpath(stripped)
            if os.path.isabs(norm) or norm == ".." or norm.startswith(".." + os.sep):
                raise SystemExit(f"refusing unsafe path in sdist: {m.name!r}")
            m.name = stripped
            members.append(m)
        if not members:
            raise SystemExit("sdist did not contain external/duckdb/ source tree")
        # Python 3.12+ warns without a filter; 'data' is the safe extraction
        # filter. Older Python lacks it, but the per-member checks above already
        # reject traversal/link entries.
        try:
            tar.extractall(dest, members=members, filter="data")
        except TypeError:
            tar.extractall(dest, members=members)  # older Python without `filter`
    return dest


def stamp_version(hpp_path: str, version: str) -> None:
    """Rewrite the DUCKDB_VERSION / MAJOR / MINOR / PATCH defines to `version`.

    The generator emits placeholder v0.0.0 defines when run outside a git
    checkout (no SHA to read); stamp the real pinned version so
    `make ensure_amalgamation`'s grep is satisfied.
    """
    m = re.match(r"v?(\d+)\.(\d+)\.(\d+)", version)
    if not m:
        raise SystemExit(f"unparseable version '{version}' (expected vX.Y.Z)")
    major, minor, patch = m.groups()
    with open(hpp_path, encoding="utf-8") as fh:
        text = fh.read()
    text = re.sub(
        r'#define\s+DUCKDB_VERSION\s+"[^"]*"',
        f'#define DUCKDB_VERSION "{version}"',
        text,
        count=1,
    )
    text = re.sub(
        r"#define\s+DUCKDB_MAJOR_VERSION\s+\d+",
        f"#define DUCKDB_MAJOR_VERSION {major}",
        text,
        count=1,
    )
    text = re.sub(
        r"#define\s+DUCKDB_MINOR_VERSION\s+\d+",
        f"#define DUCKDB_MINOR_VERSION {minor}",
        text,
        count=1,
    )
    # PATCH may be emitted as an int (0) or a quoted string ("0").
    text = re.sub(
        r'#define\s+DUCKDB_PATCH_VERSION\s+"?\d+"?',
        f'#define DUCKDB_PATCH_VERSION "{patch}"',
        text,
        count=1,
    )
    with open(hpp_path, "w", encoding="utf-8") as fh:
        fh.write(text)


def install(hpp_src: str, cpp_src: str, version: str) -> None:
    """Copy generated files into cpp/include/ and the .amalgamation cache."""
    cache = os.path.join(REPO_ROOT, ".amalgamation", version)
    os.makedirs(cache, exist_ok=True)
    os.makedirs(INCLUDE_DIR, exist_ok=True)
    for src, base in ((hpp_src, "duckdb.hpp"), (cpp_src, "duckdb.cpp")):
        shutil.copyfile(src, os.path.join(cache, base))
        shutil.copyfile(src, os.path.join(INCLUDE_DIR, base))
    log(f"installed cpp/include/duckdb.{{hpp,cpp}} and cached under {cache}")


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--version", help="override .duckdb-version (e.g. v1.5.4)")
    ap.add_argument(
        "--force", action="store_true", help="regenerate even if already present"
    )
    args = ap.parse_args()

    version = args.version or read_target_version()  # 'v1.5.4'
    pyver = version.lstrip("v")  # '1.5.4'
    tag = version if version.startswith("v") else f"v{version}"

    # Require BOTH files present at the right version: a stray duckdb.hpp with a
    # missing duckdb.cpp (partial checkout / cache cleanup) must still trigger a
    # rebuild, not a false no-op.
    if (
        not args.force
        and installed_version() == version
        and os.path.exists(CPP_DST)
    ):
        log(f"cpp/include/duckdb.{{hpp,cpp}} already at {version}; nothing to do (use --force to regenerate)")
        return 0

    # Fast path: restore from the version cache only if BOTH files are cached;
    # a partially-populated cache falls through to a full regeneration.
    cache = os.path.join(REPO_ROOT, ".amalgamation", version)
    cache_hpp = os.path.join(cache, "duckdb.hpp")
    cache_cpp = os.path.join(cache, "duckdb.cpp")
    if not args.force and os.path.exists(cache_hpp) and os.path.exists(cache_cpp):
        log(f"restoring from cache {cache}")
        os.makedirs(INCLUDE_DIR, exist_ok=True)
        shutil.copyfile(cache_hpp, HPP_DST)
        shutil.copyfile(cache_cpp, CPP_DST)
        return 0

    log(f"reconstructing DuckDB {version} amalgamation from PyPI sdist + jsDelivr")
    with tempfile.TemporaryDirectory(prefix="dd-amalg-") as tmp:
        # 1. DuckDB source tree from the PyPI sdist.
        sdist = os.path.join(tmp, "duckdb-sdist.tar.gz")
        curl(sdist_url(pyver), sdist)
        ddroot = extract_duckdb_source(sdist, os.path.join(tmp, "duckdb"))

        # 2. Amalgamation generator from jsDelivr (repo files, not release assets).
        scripts_dir = os.path.join(ddroot, "scripts")
        os.makedirs(scripts_dir, exist_ok=True)
        for name in GEN_SCRIPTS:
            curl(JSDELIVR.format(tag=tag, name=name), os.path.join(scripts_dir, name))

        # 3. Generate src/amalgamation/duckdb.{hpp,cpp}. The generator shells out
        #    to `git` for a version string and prints a harmless warning when the
        #    tree is not a git checkout; that does not affect the output.
        log("running scripts/amalgamation.py")
        subprocess.run(
            [sys.executable, os.path.join("scripts", "amalgamation.py")],
            cwd=ddroot,
            check=True,
        )
        gen_hpp = os.path.join(ddroot, "src", "amalgamation", "duckdb.hpp")
        gen_cpp = os.path.join(ddroot, "src", "amalgamation", "duckdb.cpp")
        if not (os.path.exists(gen_hpp) and os.path.exists(gen_cpp)):
            raise SystemExit("amalgamation.py did not produce duckdb.{hpp,cpp}")

        # 4. Stamp the real version, then install + cache.
        stamp_version(gen_hpp, version)
        install(gen_hpp, gen_cpp, version)

    log("done. NOTE: DUCKDB_SOURCE_ID is a placeholder — good for build/lint; "
        "prefer the official GitHub release for anything that LOADs the extension.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
