"""Independent vectors against the packaged native shell parser, before Nix effects."""
import pathlib
import subprocess
import sys
import tempfile

program, golden = sys.argv[1:]
usage = pathlib.Path(golden).read_bytes()
with tempfile.TemporaryDirectory() as work:
    def run(*args):
        return subprocess.run([program.encode(), *args], cwd=work, capture_output=True, check=False)

    for args in [(b"--help",), (b"-h",), (b"--plan", b"--plan", b"--no-lock", b"--no-lock", b"--help")]:
        result = run(*args)
        assert (result.returncode, result.stdout, result.stderr) == (0, usage, b""), result
    for flag in [b"--root", b"--nixfied-url"]:
        for operand in [None, b"", b"--help", b"--value"]:
            args = (flag,) if operand is None else (flag, operand, b"--help")
            result = run(*args)
            assert (result.returncode, result.stdout, result.stderr) == (2, b"", b"missing " + flag + b" value\n"), result
    for token in [b"--root=.", b"positional", b"--", b"-hh", b"\xff"]:
        result = run(token, b"--help")
        assert (result.returncode, result.stdout, result.stderr) == (2, b"", b"unknown upgrade argument: " + token + b"\n" + usage), result
    # These paths all stop at the native missing-flake check. No Nix call occurs.
    for args, path in [
        ((), b"."),
        ((b"--root", b"-h"), b"-h"),
        ((b"--root", b"first", b"--root", b"second"), b"second"),
        ((b"--root", b"trimmed\n\n"), b"trimmed"),
        ((b"--root", b"bytes-\xff"), b"bytes-\xff"),
        ((b"--nixfied-url", b"first", b"--nixfied-url", b"-h"), b"."),
    ]:
        result = run(*args)
        assert (result.returncode, result.stdout, result.stderr) == (
            3, b"", b"no Nixfied flake.nix to upgrade in " + path + b"\nNo files were changed.\nRun 'nixfied install' first to scaffold a project.\n"
        ), result
