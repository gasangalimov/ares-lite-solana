"""Built-in solver: submit the season baseline module unchanged.

Needs no toolchain. It scores exactly the baseline (no improvement): use it to
check that the whole flow works, then switch to a real solver.
"""

from __future__ import annotations

from pathlib import Path

from ares_lite.solver_contract import read_request, write_response


def main() -> None:
    request = read_request()
    write_response(request, Path(request["baseline_path"]).read_bytes(), notes="season baseline, unchanged",
                   solver_name="baseline", solver_version="1")


if __name__ == "__main__":
    main()
