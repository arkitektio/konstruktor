"""pytest fixtures that build a real Arkitekt deployment for a test session.

Registered as a pytest plugin when the package is installed, so a test only asks
for the fixture::

    import pytest

    @pytest.fixture(scope="session")
    def hub(konstruktor_hub):
        return konstruktor_hub(services=["rekuest", "mikro"], redeem_tokens=2)

    @pytest.mark.konstruktor
    def test_my_app(hub):
        ...  # connect to hub.fakts_url with hub.redeem_token("my-app")

Tests marked ``konstruktor`` are skipped where Docker is not running. Hubs are
destroyed when the session ends -- and, should a session be killed before it
could, by the next one that starts.

Which *build* of a service a session runs against is not the suite's to say: set
``$KONSTRUKTOR_IMAGES`` (``rekuest=jhnnsrs/rekuest:1.2.3,mikro=...``) where the
suite is run, and every hub the CLI creates there uses those images.
"""

import contextlib
import os
import shutil
import subprocess
import sys
import tempfile
import uuid
from collections.abc import Callable, Iterator, Mapping, Sequence
from pathlib import Path

import pytest

from konstruktor._binary import KonstruktorNotFoundError, find_konstruktor_bin
from konstruktor.hub import ACCESS_FILE, Hub, KonstruktorError, create_hub

#: Where one session keeps its hubs: ``<tmp>/konstruktor-pytest/<pid>-<random>``.
#: The pid in the name is how a later session knows this one is gone.
_RUNS = "konstruktor-pytest"
_HUBS = pytest.StashKey[list[Hub]]()


def pytest_addoption(parser: pytest.Parser) -> None:
    """The plugin's command line options."""
    group = parser.getgroup("konstruktor")
    group.addoption(
        "--konstruktor-keep",
        action="store_true",
        help="Leave the hubs running after the session, to look at them. "
        "Their folders are printed; `konstruktor destroy <folder>` removes one.",
    )
    group.addoption(
        "--konstruktor-bin",
        default=None,
        help="The konstruktor executable to use. Defaults to $KONSTRUKTOR_BIN, then "
        "the one this package ships, then PATH.",
    )
    group.addoption(
        "--konstruktor-timeout",
        type=float,
        default=600.0,
        help="Seconds a hub gets to answer on every endpoint after it is started.",
    )


def pytest_configure(config: pytest.Config) -> None:
    """Register the marker, so ``--strict-markers`` accepts it."""
    config.addinivalue_line(
        "markers",
        "konstruktor: builds a real deployment with konstruktor (needs Docker).",
    )
    config.stash[_HUBS] = []


def docker_available() -> bool:
    """Whether a Docker daemon answers on this machine."""
    docker = shutil.which("docker")
    if docker is None:
        return False
    try:
        return (
            subprocess.run(
                [docker, "info"], capture_output=True, timeout=30, check=False
            ).returncode
            == 0
        )
    except (OSError, subprocess.TimeoutExpired):
        return False


def pytest_collection_modifyitems(config: pytest.Config, items: list[pytest.Item]) -> None:
    """Skip the tests that need a deployment where one cannot be built."""
    marked = [item for item in items if item.get_closest_marker("konstruktor")]
    if not marked:
        return
    reason = None
    if not docker_available():
        reason = "konstruktor builds deployments with Docker, which is not running here."
    else:
        try:
            find_konstruktor_bin(config.getoption("--konstruktor-bin"))
        except KonstruktorNotFoundError as error:
            reason = str(error)
    if reason is not None:
        skip = pytest.mark.skip(reason=reason)
        for item in marked:
            item.add_marker(skip)


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


def reap_dead_runs(runs: Path, binary: Path) -> list[Path]:
    """Destroy the hubs of sessions that died before they could.

    A session that is killed -- a cancelled CI job, an out-of-memory kill -- leaves
    its containers running and its volumes behind, with nobody left who knows they
    are there. Each session's folder carries the pid that owned it; a folder whose
    owner is gone is destroyed, hub by hub, by the same command a living session
    would have used.

    Returns:
        The run folders that were removed.
    """
    removed: list[Path] = []
    if not runs.is_dir():
        return removed
    for run in runs.iterdir():
        pid, _, _ = run.name.partition("-")
        if not pid.isdigit() or _is_alive(int(pid)):
            continue
        hubs = run / "hubs"
        for directory in sorted(hubs.iterdir()) if hubs.is_dir() else []:
            try:
                Hub.load(directory, data_dir=run / "registry", binary=binary).destroy()
            except (KonstruktorError, FileNotFoundError, ValueError, KeyError):
                # Half written, or already gone. Nothing here is worth failing a
                # new session over; the folder goes either way.
                pass
        shutil.rmtree(run, ignore_errors=True)
        removed.append(run)
    return removed


@pytest.fixture(scope="session")
def konstruktor_bin(pytestconfig: pytest.Config) -> Path:
    """The ``konstruktor`` executable the session uses."""
    return find_konstruktor_bin(pytestconfig.getoption("--konstruktor-bin"))


@pytest.fixture(scope="session")
def konstruktor_run_dir(konstruktor_bin: Path) -> Iterator[Path]:
    """This session's folder: its hubs, and a registry of its own.

    The registry is separate from the user's, so hubs a test creates never show up
    in the desktop app and a test can never touch a hub it did not create.
    """
    runs = Path(tempfile.gettempdir()) / _RUNS
    reap_dead_runs(runs, konstruktor_bin)
    run = runs / f"{os.getpid()}-{uuid.uuid4().hex[:8]}"
    (run / "hubs").mkdir(parents=True)
    yield run
    # Only what is empty: a kept hub's folder stays where it was printed.
    if not any((run / "hubs").iterdir()):
        shutil.rmtree(run, ignore_errors=True)


HubFactory = Callable[..., Hub]


@pytest.fixture(scope="session")
def konstruktor_hub(
    pytestconfig: pytest.Config, konstruktor_bin: Path, konstruktor_run_dir: Path
) -> Iterator[HubFactory]:
    """A factory for self-contained hubs that live as long as the session.

    Call it with what :func:`konstruktor.create_hub` takes, minus the folder::

        hub = konstruktor_hub(services=["rekuest", "mikro"], redeem_tokens=2)

    or, by the images that host the services::

        hub = konstruktor_hub(service_images=["jhnnsrs/mikro:7"])

    The hub is started and answering when it is returned. A hub's address is part
    of every token it issues, so each one gets a free port of its own.
    """
    hubs = pytestconfig.stash[_HUBS]
    keep = bool(pytestconfig.getoption("--konstruktor-keep"))
    timeout = float(pytestconfig.getoption("--konstruktor-timeout"))

    def factory(
        *,
        services: Sequence[str] = ("rekuest",),
        service_images: Sequence[str] = (),
        redeem_tokens: int = 1,
        images: Mapping[str, str] | None = None,
        debug: Sequence[str] = (),
        mounts: Mapping[str, str | os.PathLike[str]] | None = None,
        organization: str = "demo",
        user: str = "demo",
        user_password: str | None = None,
        http_port: int | None = None,
        name: str | None = None,
    ) -> Hub:
        # The folder's name is the compose project, and Docker has one namespace for
        # those: a folder called `hub` here would be the same project as a real hub
        # in a folder called `hub` anywhere else on the machine — same containers,
        # same volumes, destroyed together. So the folder carries this run's id.
        identifier = name or f"hub{len(hubs) + 1}"
        directory = konstruktor_run_dir / "hubs" / f"pytest-{konstruktor_run_dir.name}-{identifier}"
        try:
            hub = create_hub(
                directory,
                identifier=identifier,
                services=services,
                service_images=service_images,
                http_port=http_port,
                organization=organization,
                user=user,
                user_password=user_password,
                redeem_tokens=redeem_tokens,
                images=images,
                debug=debug,
                mounts=mounts,
                timeout=timeout,
                data_dir=konstruktor_run_dir / "registry",
                binary=konstruktor_bin,
            )
        except KonstruktorError:
            # Written and started, but not answering: keep it for teardown, and say
            # why before it is gone.
            if (directory / ACCESS_FILE).is_file():
                failed = Hub.load(
                    directory, data_dir=konstruktor_run_dir / "registry", binary=konstruktor_bin
                )
                hubs.append(failed)
                with contextlib.suppress(KonstruktorError):
                    print(failed.logs(tail=120))
            raise
        hubs.append(hub)
        return hub

    yield factory

    reporter = pytestconfig.pluginmanager.get_plugin("terminalreporter")
    for hub in hubs:
        if keep:
            if reporter is not None:
                reporter.write_line(f"konstruktor: kept {hub.gateway_url} in {hub.directory}")
            continue
        try:
            hub.destroy()
        except KonstruktorError as error:
            if reporter is not None:
                reporter.write_line(f"konstruktor: could not destroy {hub.directory}: {error}")
    hubs.clear()


@pytest.hookimpl(hookwrapper=True)
def pytest_runtest_makereport(item: pytest.Item, call: pytest.CallInfo[None]) -> Iterator[None]:
    """Attach the hubs' logs to a failed test that used one.

    The services' side of a failure is in the containers, and the containers are
    gone by the time anybody reads the report.
    """
    outcome = yield
    report = outcome.get_result()
    if report.when != "call" or not report.failed:
        return
    if item.get_closest_marker("konstruktor") is None:
        return
    for hub in item.config.stash.get(_HUBS, []):
        with contextlib.suppress(KonstruktorError):
            report.sections.append((f"konstruktor logs: {hub.identifier}", hub.logs(tail=80)))
