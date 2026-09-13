"""Run with python3 tests/install.py; all installs and commands stay in a temporary directory."""
import os
from pathlib import Path
import subprocess
import tempfile

source = (Path(__file__).resolve().parents[1] / "scripts/install.sh").read_text()
with tempfile.TemporaryDirectory(prefix="ledge-install-test-") as directory:
    root = Path(directory)
    apps = root / "Applications"
    apps.mkdir()
    commands = root / "bin"
    commands.mkdir()
    mocks = {
        "uname": 'case "$1" in -s) echo Darwin;; -m) echo arm64;; esac',
        "curl": 'case "$*" in *releases/latest*) echo \'{"browser_download_url": "https://example.test/Ledge.dmg"}\';; esac',
        "hdiutil": '''if [ "$1" = attach ]; then
          while [ "$1" != -mountpoint ]; do shift; done
          mkdir -p "$2/Ledge.app"
          echo new > "$2/Ledge.app/version"
        fi''',
        "cp": 'if [ "$LEDGE_TEST_FAILURE" = copy ]; then exit 1; fi\nexec /bin/cp "$@"',
        "mv": '''case "$1" in
          */.ledge.*/Ledge.app) [ "$LEDGE_TEST_FAILURE" != move ] || exit 1;;
        esac
        exec /bin/mv "$@"''',
    }
    for name, body in mocks.items():
        path = commands / name
        path.write_text("#!/bin/sh\nset -eu\n" + body + "\n")
        path.chmod(0o700)
    script = root / "install.sh"
    script.write_text(source.replace("/Applications", str(apps)))
    app = apps / "Ledge.app"
    app.mkdir()
    for failure, expected in [("copy", "old"), ("move", "old"), ("none", "new")]:
        (app / "version").write_text("old\n")
        env = {**os.environ, "PATH": f"{commands}:/usr/bin:/bin", "LEDGE_TEST_FAILURE": failure}
        result = subprocess.run(["sh", str(script)], env=env, capture_output=True, text=True)
        assert (result.returncode == 0) == (failure == "none"), result.stderr
        assert (app / "version").read_text().strip() == expected, result.stderr
        assert not list(apps.glob(".ledge.*")), "staging was left behind"
print("PASS: installer preserves the previous app on copy/move failure and replaces it on success")
