"""Where hubs made for a test live, and how the ones a dead process left are removed.

A hub made for a test is destroyed by the process that made it. A process that is
killed -- a cancelled CI job, an out-of-memory kill -- leaves its containers running
and its volumes behind, with nobody left who knows they are there. So each process
keeps its hubs in one folder that carries its pid, and whichever process makes the
next test hub destroys the hubs of the ones that are gone.
"""

import os
import shutil
import sys
import tempfile
import uuid
from pathlib import Path

from dokker.command import CommandError, astream_command

#: The folder under the temp directory that holds one folder per process:
#: ``<tmp>/konstruktor-runs/<pid>-<random>``.
RUNS = "konstruktor-runs"

_run: tuple[int, Path] | None = None


def runs_dir() -> Path:
    """Where every process on this machine keeps its run folder."""
    return Path(tempfile.gettempdir()) / RUNS


def run_dir() -> Path:
    """This process's folder: its hubs, and a registry of its own.

    The registry is separate from the user's, so a hub made for a test never shows
    up in the desktop app and a test can never touch a hub it did not make.
    """
    global _run
    if _run is None or _run[0] != os.getpid() or _run[1].parent != runs_dir() or not _run[1].is_dir():
        run = runs_dir() / f"{os.getpid()}-{uuid.uuid4().hex[:8]}"
        (run / "hubs").mkdir(parents=True)
        _run = (os.getpid(), run)
    return _run[1]


def hub_dir(identifier: str) -> Path:
    """The folder of this process's hub called ``identifier``.

    The folder's name is the compose project, and Docker has one namespace for
    those: a folder called ``hub`` here would be the same project as a real hub in a
    folder called ``hub`` anywhere else on the machine -- same containers, same
    volumes, destroyed together. So the folder carries the run's id.
    """
    run = run_dir()
    # And a few random characters of its own: two hubs of one name in one process are
    # still two hubs, never the second quietly taking the folder of the first.
    return run / "hubs" / f"test-{run.name}-{identifier}-{uuid.uuid4().hex[:6]}"


def registry_dir() -> Path:
    """The registry this process's test hubs are recorded in (``KONSTRUKTOR_DATA_DIR``)."""
    return run_dir() / "registry"


def discard_run_if_empty(hub_directory: Path) -> None:
    """Remove the run folder a destroyed test hub lived in, once it holds no hub.

    A run folder outlives its hubs only as litter: its owner is alive, so nobody reaps
    it, and it holds nothing but a registry that lists nothing.
    """
    run = hub_directory.parent.parent
    hubs = run / "hubs"
    if run.parent == runs_dir() and hubs.is_dir() and not any(hubs.iterdir()):
        shutil.rmtree(run, ignore_errors=True)


def _is_alive(pid: int) -> bool:
    if sys.platform == "win32":
        # There `os.kill` with any signal but the two console events *terminates*
        # the process. Not knowing is the safe answer: nothing is reaped.
        return True
    try:
        os.kill(pid, 0)
    except ProcessLookupError:
        return False
    except PermissionError:  # it exists, and belongs to somebody else
        return True
    except OSError:
        return False
    return True


async def areap_dead_runs(runs: Path, binary: Path) -> list[Path]:
    """Destroy the hubs of processes that died before they could.

    A run folder whose owner is gone is destroyed, hub by hub, by the same command
    a living process would have used.

    Returns:
        The run folders that were removed.
    """
    removed: list[Path] = []
    if not runs.is_dir():
        return removed
    for run in sorted(runs.iterdir()):
        pid, _, _ = run.name.partition("-")
        if not pid.isdigit() or _is_alive(int(pid)):
            continue
        hubs = run / "hubs"
        for directory in sorted(hubs.iterdir()) if hubs.is_dir() else []:
            command = [os.fspath(binary), "destroy", os.fspath(directory), "--yes", "--local-only"]
            try:
                async for _ in astream_command(command, env={"KONSTRUKTOR_DATA_DIR": os.fspath(run / "registry")}):
                    pass
            except CommandError:
                # Half written, or already gone. Nothing here is worth failing a
                # new hub over; the folder goes either way.
                pass
        shutil.rmtree(run, ignore_errors=True)
        removed.append(run)
    return removed
