"""A stand-in for the ``konstruktor`` executable.

The wrapper's whole job is to run the right command and read back what it wrote,
so most of its tests need no Docker and no real binary: a script that records its
arguments and writes what the real one would is the other side of that contract.
The tests marked ``konstruktor`` run the real thing.
"""

import json
import os
import stat
import sys
import tempfile
from pathlib import Path

import pytest

pytest_plugins = ["pytester"]

_FAKE = '''#!{python}
import json, os, sys
from pathlib import Path

args = sys.argv[1:]
log = Path(os.environ["FAKE_KONSTRUKTOR_LOG"])
with log.open("a") as handle:
    handle.write(json.dumps({{"args": args, "data_dir": os.environ.get("KONSTRUKTOR_DATA_DIR")}}) + "\\n")

fail = os.environ.get("FAKE_KONSTRUKTOR_FAIL")
if fail and args and args[0] == fail:
    sys.stderr.write("  x it did not work\\n")
    sys.exit(3)

def flag(name):
    return args[args.index(name) + 1]

if args[:2] == ["hub", "create"]:
    directory = Path(args[2])
    port = flag("--http-port")
    base = "http://localhost:" + port
    # By name, or by image: the last part of an image's name is the service it is.
    named = flag("--services").split(",") if "--services" in args else [
        args[i + 1].rsplit("/", 1)[-1].split(":")[0]
        for i, arg in enumerate(args) if arg == "--service-image"
    ]
    services = ["lok"] + named
    access = {{
        "version": 1,
        "gateway_url": base,
        "fakts_url": base,
        "issuer": "lok",
        "hub": flag("--identifier") if "--identifier" in args else directory.name,
        "organization": flag("--org"),
        "admin": {{"username": "admin", "password": "admin-pass"}},
        "users": [{{"username": flag("--user"), "password": "user-pass"}}],
        "redeem_tokens": ["token-%d" % i for i in range(int(flag("--redeem-tokens")))],
        "services": {{
            name: {{
                "identifier": "live.arkitekt." + name,
                "url": base + "/" + name,
                "health_url": base + "/" + name + "/ht?format=json",
            }}
            for name in services
        }},
    }}
    (directory / "secrets").mkdir(parents=True, exist_ok=True)
    (directory / "secrets" / "access.json").write_text(json.dumps(access))
    (directory / "docker-compose.yaml").write_text("services: {{}}\\n")
    print(directory)
elif args[:1] == ["wait"]:
    print(json.dumps([{{"name": "fakts", "url": "http://localhost/.well-known/fakts", "status": 200, "ready": True}}]))
elif args[:1] == ["logs"]:
    print("mikro-1  | a log line")
elif args[:1] == ["destroy"]:
    import shutil
    shutil.rmtree(args[1], ignore_errors=True)
'''


class FakeKonstruktor:
    """The stand-in executable, and what it was called with."""

    def __init__(self, path: Path, log: Path) -> None:
        self.path = path
        self.log = log

    @property
    def calls(self) -> list[list[str]]:
        """The argument list of every call so far, oldest first."""
        return [entry["args"] for entry in self.entries]

    @property
    def entries(self) -> list[dict]:
        if not self.log.exists():
            return []
        return [json.loads(line) for line in self.log.read_text().splitlines()]


@pytest.fixture
def fake_konstruktor(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> FakeKonstruktor:
    """A fake ``konstruktor`` on disk, with ``$KONSTRUKTOR_BIN`` pointing at it."""
    if sys.platform == "win32":
        pytest.skip("the stand-in executable is a script with a shebang")
    path = tmp_path / "bin" / "konstruktor"
    path.parent.mkdir()
    path.write_text(_FAKE.format(python=sys.executable))
    path.chmod(path.stat().st_mode | stat.S_IEXEC)
    log = tmp_path / "calls.jsonl"
    monkeypatch.setenv("KONSTRUKTOR_BIN", os.fspath(path))
    # Where a process keeps the hubs it makes for tests. Not the machine's real temp directory:
    # the stand-in destroys nothing, so reaping a real dead run there would delete its
    # folder and leave its containers running.
    scratch = tmp_path / "tmp"
    scratch.mkdir()
    monkeypatch.setenv("TMPDIR", os.fspath(scratch))
    # And for this process, which asked for the temp directory long ago and remembers.
    monkeypatch.setattr(tempfile, "tempdir", os.fspath(scratch))
    monkeypatch.setenv("FAKE_KONSTRUKTOR_LOG", os.fspath(log))
    return FakeKonstruktor(path, log)
