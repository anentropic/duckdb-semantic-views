"""
Shared helpers for DuckLake integration tests.

Provides common boilerplate for loading the semantic_views extension and
attaching a DuckLake catalog. Used by both test_ducklake.py (local) and
test_ducklake_ci.py (CI).
"""

import os
from pathlib import Path


def get_project_root() -> Path:
    """Return the project root directory (3 levels up from this file)."""
    # test/integration/test_ducklake_helpers.py -> test/integration -> test -> project root
    return Path(__file__).resolve().parent.parent.parent


def get_ext_dir() -> str:
    """Return the project-local extension directory, creating it if needed."""
    ext_dir = get_project_root() / "test" / "data" / "duckdb_extensions"
    ext_dir.mkdir(parents=True, exist_ok=True)
    return str(ext_dir)


def get_extension_path() -> Path:
    """
    Return the semantic_views extension path.

    Checks SEMANTIC_VIEWS_EXTENSION_PATH environment variable first.
    Falls back to the CMake debug build path.
    """
    env_path = os.environ.get("SEMANTIC_VIEWS_EXTENSION_PATH")
    if env_path:
        return Path(env_path)
    return get_project_root() / "build" / "debug" / "semantic_views.duckdb_extension"


def load_extension(con, extension_path: Path) -> None:
    """
    Install and load the semantic_views extension plus DuckLake.

    Args:
        con: A duckdb.DuckDBPyConnection instance.
        extension_path: Path to the semantic_views .duckdb_extension file.
    """
    # FORCE INSTALL ensures the freshly-built binary overwrites any stale cached copy
    # in the project-local extension directory.
    con.execute(f"FORCE INSTALL '{extension_path}'")
    con.execute("LOAD semantic_views")
    con.execute("LOAD ducklake")


def attach_ducklake(con, ducklake_file: str, data_dir: str, alias: str = "jaffle") -> None:
    """
    Attach a DuckLake catalog to an existing connection.

    Args:
        con: A duckdb.DuckDBPyConnection instance.
        ducklake_file: Path to the .ducklake metadata file.
        data_dir: Path to the data directory. Must end with '/'.
        alias: The catalog alias to use in SQL (default: 'jaffle').
    """
    con.execute(
        f"ATTACH 'ducklake:{ducklake_file}' AS {alias} (DATA_PATH '{data_dir}')"
    )
