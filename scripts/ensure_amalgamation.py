#!/usr/bin/env python3
"""Install `cpp/include/duckdb.{hpp,cpp}` for the engine this build targets.

`build.rs` compiles the DuckDB amalgamation *into* the extension binary
alongside `cpp/src/shim.cpp`, because the parser-extension hook the shim
registers is C++ internals with no C-API equivalent. That makes the
amalgamation an ABI contract, not a convenience download: whatever engine
loads the extension must agree with the headers it was compiled against.

Two sources, in priority order:

1. **An engine source tree** (`--src-dir`, default `./duckdb`). A harness that
   builds the extension against a specific engine clones it there: Haybarn's
   fork of `_extension_distribution.yml` runs `git clone --branch
   <duckdb_version> <override_duckdb_repository> duckdb` before every build.
   When that tree is present it *is* the engine the extension will be loaded
   into, so the amalgamation is generated from it with the engine's own
   `scripts/amalgamation.py`. This is what makes an alternative distribution
   build correctly: Haybarn's fork diverges from upstream in headers the shim
   compiles against, so an upstream amalgamation would silently bake the wrong
   layouts into the binary. Upstream community-extensions supplies no tree for a
   C-API extension, so its builds take path 2.
2. **The pinned upstream release** (`--url`), downloaded and cached. This is
   the local-developer and upstream-CI path, unchanged from before.

Provenance is recorded in `cpp/include/.amalgamation_id` and compared on every
build, so switching engines rebuilds and switching back is a cache hit. The
identity is the engine commit for a source tree (`src-<sha>`) and the version
for a release download (`v1.5.5`) — a bare `DUCKDB_VERSION` check cannot tell
the two apart, because a fork labels its amalgamation with the upstream version
it is based on.

Usage (normally invoked by `make ensure_amalgamation`):

    python3 scripts/ensure_amalgamation.py                        # auto-detect
    python3 scripts/ensure_amalgamation.py --force                # re-install
    python3 scripts/ensure_amalgamation.py --src-dir ''           # release only

See also `scripts/fetch_amalgamation_offline.py`, the GitHub-free fallback for
sandboxed sessions whose egress proxy blocks the release asset.

Only the Python standard library is required (plus `curl` on PATH).
"""

from __future__ import annotations

import argparse
import contextlib
import filecmp
import os
import re
import shutil
import subprocess
import sys
import tempfile
import zipfile
from typing import NoReturn

DEFAULT_ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
FILES = ("duckdb.hpp", "duckdb.cpp")
STAMP_NAME = ".amalgamation_id"
# Stamp written when an install cannot be identified (an engine tree that is not
# a git checkout). It never matches a computed identity, so such a tree is
# regenerated on every build; what it does is mark the install as "known to be
# of non-release provenance", which is what keeps the adopt-in-place path below
# from mistaking those headers for the pinned release.
UNIDENTIFIED = "src-unidentified"


def log(msg: str) -> None:
    print(f"[ensure-amalgamation] {msg}", flush=True)


def fail(msg: str) -> NoReturn:
    raise SystemExit(f"[ensure-amalgamation] ERROR: {msg}")


def read_target_version(root: str) -> str:
    """Pinned DuckDB version, e.g. 'v1.5.5', from .duckdb-version."""
    with open(os.path.join(root, ".duckdb-version"), encoding="utf-8") as fh:
        return fh.read().strip()


def header_version(hpp_path: str) -> str | None:
    """`DUCKDB_VERSION` as defined in `hpp_path`, or None.

    The define sits in the first handful of lines; stop at the version block's
    end rather than scanning 2 MB of header.
    """
    if not os.path.exists(hpp_path):
        return None
    with open(hpp_path, encoding="utf-8") as fh:
        for line in fh:
            m = re.search(r'#define\s+DUCKDB_VERSION\s+"([^"]*)"', line)
            if m:
                return m.group(1)
            if line.startswith("#define DUCKDB_MAJOR_VERSION"):
                break  # version block passed without a match
    return None


def engine_source_tree(root: str, src_dir: str) -> str | None:
    """Absolute path of a usable engine source tree, or None.

    "Usable" means it carries the generator we would run (`scripts/
    amalgamation.py`). A bare `duckdb/` directory that is something else — an
    empty submodule mount point, a stray checkout of the Python package — is
    not mistaken for an engine.
    """
    if not src_dir:
        return None
    path = src_dir if os.path.isabs(src_dir) else os.path.join(root, src_dir)
    if os.path.exists(os.path.join(path, "scripts", "amalgamation.py")):
        return os.path.abspath(path)
    return None


def git_commit(tree: str) -> str | None:
    """Short commit hash of `tree`, or None when it is not a git checkout."""
    try:
        # `safe.directory=*` because CI hands the tree to the build across an
        # ownership boundary (the community-extensions Linux leg clones on the
        # host and builds inside a container), where git otherwise refuses the
        # repository outright and the tree would look uncacheable.
        out = subprocess.run(
            ["git", "-c", "safe.directory=*", "-C", tree, "rev-parse", "--short=10", "HEAD"],
            capture_output=True,
            text=True,
            check=True,
        )
    except (OSError, subprocess.CalledProcessError):
        return None
    return out.stdout.strip() or None


def read_stamp(include_dir: str) -> str | None:
    path = os.path.join(include_dir, STAMP_NAME)
    if not os.path.exists(path):
        return None
    with open(path, encoding="utf-8") as fh:
        return fh.read().strip() or None


def write_stamp(include_dir: str, amalgamation_id: str) -> None:
    with open(os.path.join(include_dir, STAMP_NAME), "w", encoding="utf-8") as fh:
        fh.write(amalgamation_id + "\n")


def cache_dir(root: str, amalgamation_id: str) -> str:
    return os.path.join(root, ".amalgamation", amalgamation_id)


def cache_complete(cache: str) -> bool:
    return all(os.path.exists(os.path.join(cache, name)) for name in FILES)


def download_release(url: str, cache: str) -> None:
    """Fetch `libduckdb-src.zip` from `url` and extract the two files we need."""
    os.makedirs(cache, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="dd-amalg-") as tmp:
        zip_path = os.path.join(tmp, "libduckdb-src.zip")
        log(f"downloading {url}")
        try:
            subprocess.run(["curl", "-fsSL", "-o", zip_path, url], check=True)
        except (OSError, subprocess.CalledProcessError) as exc:
            fail(
                f"could not download the amalgamation from {url} ({exc}).\n"
                "  If GitHub is unreachable (sandboxed session behind a proxy), run\n"
                "  `python3 scripts/fetch_amalgamation_offline.py` to reconstruct the\n"
                "  same files from GitHub-free hosts, then re-run the build."
            )
        with zipfile.ZipFile(zip_path) as zf:
            names = set(zf.namelist())
            missing = [name for name in FILES if name not in names]
            if missing:
                fail(f"{url} does not contain {', '.join(missing)}")
            for name in FILES:
                with zf.open(name) as src, open(os.path.join(cache, name), "wb") as dst:
                    shutil.copyfileobj(src, dst)
    log(f"cached {cache}/duckdb.{{hpp,cpp}}")


def generate_from_source(tree: str, cache: str, version: str) -> None:
    """Run the engine's own amalgamation generator over `tree` into `cache`.

    `OVERRIDE_GIT_DESCRIBE` is what DuckDB's build system uses to label a build
    (`scripts/package_build.py: get_git_describe`), and it is what decides the
    `DUCKDB_VERSION` / `DUCKDB_SOURCE_ID` defines the generator emits. Default
    it to the version we target so a fork's tag (`haybarn-v1.5.5-rc1`, which
    `git describe --match 'v*.*.*'` cannot see) still produces headers labelled
    with the DuckDB release the fork is based on — exactly what the fork's own
    release build does. An explicit value in the environment wins, because the
    harness that supplied the tree knows better than we do.
    """
    env = dict(os.environ)
    env.setdefault("OVERRIDE_GIT_DESCRIBE", version)
    log(
        f"generating the amalgamation from {tree} "
        f"(OVERRIDE_GIT_DESCRIBE={env['OVERRIDE_GIT_DESCRIBE']})"
    )
    try:
        subprocess.run(
            [sys.executable, os.path.join("scripts", "amalgamation.py")],
            cwd=tree,
            env=env,
            check=True,
        )
    except (OSError, subprocess.CalledProcessError) as exc:
        fail(f"the engine's scripts/amalgamation.py failed in {tree} ({exc})")
    generated = os.path.join(tree, "src", "amalgamation")
    missing = [
        name for name in FILES if not os.path.exists(os.path.join(generated, name))
    ]
    if missing:
        fail(f"scripts/amalgamation.py did not produce {', '.join(missing)}")
    os.makedirs(cache, exist_ok=True)
    for name in FILES:
        shutil.copyfile(os.path.join(generated, name), os.path.join(cache, name))
    log(f"cached {cache}/duckdb.{{hpp,cpp}}")


def install(cache: str, include_dir: str) -> bool:
    """Copy the cached files into `include_dir`; True if anything changed.

    Identical content is left alone: `build.rs` reruns on the *mtime* of
    `cpp/include/duckdb.cpp`, so a needless copy costs a ~10 min rebuild of the
    25 MB amalgamation translation unit.
    """
    os.makedirs(include_dir, exist_ok=True)
    changed = False
    for name in FILES:
        src = os.path.join(cache, name)
        dst = os.path.join(include_dir, name)
        if os.path.exists(dst) and filecmp.cmp(src, dst, shallow=False):
            continue
        shutil.copyfile(src, dst)
        changed = True
    return changed


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--repo-root", default=DEFAULT_ROOT, help="repository root")
    ap.add_argument("--version", help="target DuckDB version (default: .duckdb-version)")
    ap.add_argument(
        "--src-dir",
        default="duckdb",
        help="engine source tree to generate from; '' disables the source path",
    )
    ap.add_argument("--url", help="release zip to download when no source tree is present")
    ap.add_argument(
        "--force", action="store_true", help="re-install even when the stamp matches"
    )
    args = ap.parse_args(argv)

    root = os.path.abspath(args.repo_root)
    include_dir = os.path.join(root, "cpp", "include")
    version = args.version or read_target_version(root)
    url = args.url or (
        f"https://github.com/duckdb/duckdb/releases/download/{version}/libduckdb-src.zip"
    )

    tree = engine_source_tree(root, args.src_dir)
    if tree:
        commit = git_commit(tree)
        # A non-git tree has no stable identity to cache under, so it is
        # regenerated every build rather than silently reused after it changes.
        amalgamation_id = f"src-{commit}" if commit else None
    else:
        amalgamation_id = version

    installed = [os.path.join(include_dir, name) for name in FILES]
    have_all = all(os.path.exists(p) for p in installed)
    stamp = read_stamp(include_dir)

    if not args.force and amalgamation_id and have_all and stamp == amalgamation_id:
        return 0

    # Adopt an unstamped install that already matches what we would download.
    # Checkouts predating the stamp have the pinned release in place; copying
    # over it byte-identically would be free, but writing the stamp without
    # touching mtimes keeps their next `cargo build` incremental.
    #
    # `stamp is None` is doing real work here: an install this script made from
    # an engine tree carries UNIDENTIFIED even when it could not be named, and a
    # fork labels its amalgamation with the upstream version it is based on, so
    # without that marker the version check below would happily adopt a fork's
    # headers as the release.
    if (
        not args.force
        and not tree
        and have_all
        and stamp is None
        and header_version(installed[0]) == version
    ):
        write_stamp(include_dir, version)
        log(f"adopted the installed {version} amalgamation")
        return 0

    if tree:
        log(f"engine source tree: {tree} (commit {commit or 'unknown'})")

    with contextlib.ExitStack() as stack:
        cache = cache_dir(root, amalgamation_id) if amalgamation_id else None
        if cache and cache_complete(cache) and not args.force:
            log(f"restoring from cache {cache}")
        else:
            # Without a stable identity there is nothing to cache under, so
            # stage into a temp dir that is discarded after installation.
            staged = cache or stack.enter_context(
                tempfile.TemporaryDirectory(prefix="dd-amalg-")
            )
            if tree:
                generate_from_source(tree, staged, version)
            else:
                download_release(url, staged)
            cache = staged
        _finish(include_dir, cache, amalgamation_id, version, tree)
    return 0


def _finish(
    include_dir: str,
    cache: str,
    amalgamation_id: str | None,
    version: str,
    tree: str | None,
) -> None:
    """Verify the staged amalgamation, install it, and record its provenance."""
    staged_version = header_version(os.path.join(cache, "duckdb.hpp"))
    if staged_version != version:
        origin = f"the engine source tree {tree}" if tree else "the release download"
        fail(
            f"{origin} produced an amalgamation labelled {staged_version or 'unknown'}, "
            f"but this extension targets {version} (.duckdb-version).\n"
            "  Building the C++ shim against a different DuckDB than the one that will "
            "load it is not supported.\n"
            "  Point --src-dir at a matching engine checkout, or update .duckdb-version."
        )
    changed = install(cache, include_dir)
    write_stamp(include_dir, amalgamation_id or UNIDENTIFIED)
    state = "installed" if changed else "already current"
    log(f"{state}: cpp/include/duckdb.{{hpp,cpp}} ({version}, {amalgamation_id or UNIDENTIFIED})")


if __name__ == "__main__":
    sys.exit(main())
