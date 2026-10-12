"""Run the whole gate: Rust's, TypeScript's, and the end-to-end install.

Three gates, not one, because the change touched three different things that
fail in different ways. Rust pedantic failures are text; a TypeScript failure is
a build that produces a `dist/` missing a file; and the end-to-end check is the
only one that can see a package that installs and then does not run.

Run every gate rather than the one the change touched most, because the last
commit's mistake was staged `node_modules/` into a commit and only the
end-to-end check would have caught a dist/ that was not built.
"""

from __future__ import annotations

import pathlib
import subprocess
import sys

STEPS = [
    ("rust: fmt", ["cargo", "fmt", "--all", "--", "--check"]),
    ("rust: clippy", [
        "cargo", "+1.99.0", "clippy", "--workspace", "--all-targets",
        "--all-features", "--", "-D", "warnings",
    ]),
    ("rust: tests", ["cargo", "test", "--workspace"]),
    ("ts: typecheck", ["pnpm", "--filter", "sephera-mcp", "typecheck"]),
    ("ts: test", ["pnpm", "--filter", "sephera-mcp", "test"]),
    ("ts: build", ["pnpm", "--filter", "sephera-mcp", "build"]),
    ("e2e: install + probe", ["python", "scripts/check_mcp_package.py"]),
]


def main() -> int:
    import shutil

    def resolve(command: list[str]) -> list[str]:
        """Give `pnpm` a full path, because the bare name is not executable.

        `shutil.which("pnpm")` finds `pnpm.CMD`, but `subprocess` on Windows calls
        `CreateProcess` and that will not start a bare command name with a `.CMD`
        extension -- it needs the path. The same mistake cost a gate run before:
        the reported failure was `FileNotFoundError`, which reads as a missing
        pnpm rather than as a pnpm this script could not invoke.
        """
        if command[0] != "pnpm":
            return command
        found = shutil.which("pnpm")
        if found:
            return [found, *command[1:]]
        for candidate in [
            r"C:\Users\reim\AppData\Roaming\npm\pnpm.cmd",
            r"C:\Program Files\nodejs\pnpm.cmd",
            r"D:\nodejs\pnpm.cmd",
        ]:
            if pathlib.Path(candidate).is_file():
                return [candidate, *command[1:]]
        raise SystemExit("pnpm was not found")

    failed = []
    for label, command in STEPS:
        print(f"=== {label} ===", flush=True)
        done = subprocess.run(
            resolve(command), capture_output=True, text=True, check=False
        )
        if done.returncode != 0:
            print(f"FAILED\n{done.stdout[-1500:]}{done.stderr[-1500:]}")
            failed.append(label)
        else:
            print("ok")
    print()
    if failed:
        print(f"{len(failed)} gate(s) failed: {', '.join(failed)}")
        return 1
    print(f"all {len(STEPS)} gates green")
    return 0


if __name__ == "__main__":
    sys.exit(main())
