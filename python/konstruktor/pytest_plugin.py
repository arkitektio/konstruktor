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

The hub is a :class:`konstruktor.Hub`, entered for the session: started and
answering when the factory returns it, and open to everything a hub can be asked
(``hub.ps()``, ``hub.logs("mikro")``, ``hub.exec(...)``, ``hub.create_watcher(...)``).

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
from collections.abc import Callable, Iterator, Mapping, Sequence
from pathlib import Path

import pytest
from dokker import DokkerError

from konstruktor._binary import KonstruktorNotFoundError, find_konstruktor_bin
from konstruktor.hub import Hub, testing_hub
from konstruktor.runs import run_dir

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


@pytest.fixture(scope="session")
def konstruktor_bin(pytestconfig: pytest.Config) -> Path:
    """The ``konstruktor`` executable the session uses."""
    return find_konstruktor_bin(pytestconfig.getoption("--konstruktor-bin"))


@pytest.fixture(scope="session")
def konstruktor_run_dir() -> Iterator[Path]:
    """This session's folder: its hubs, and a registry of its own.

    The registry is separate from the user's, so hubs a test creates never show up
    in the desktop app and a test can never touch a hub it did not create.
    """
    run = run_dir()
    yield run
    # Only what is empty: a kept hub's folder stays where it was printed. And it may be
    # gone already: the last hub destroyed takes the folder with it.
    hubs = run / "hubs"
    if hubs.is_dir() and not any(hubs.iterdir()):
        shutil.rmtree(run, ignore_errors=True)


HubFactory = Callable[..., Hub]


@pytest.fixture(scope="session")
def konstruktor_hub(
    pytestconfig: pytest.Config, konstruktor_bin: Path, konstruktor_run_dir: Path
) -> Iterator[HubFactory]:
    """A factory for self-contained hubs that live as long as the session.

    Call it with what :func:`konstruktor.testing_hub` takes::

        hub = konstruktor_hub(services=["rekuest", "mikro"], redeem_tokens=2)

    or, by the images that host the services::

        hub = konstruktor_hub(service_images=["jhnnsrs/mikro:7"])

    The hub is entered, started and answering when it is returned, and stays
    entered until the session ends. A hub's address is part of every token it
    issues, so each one gets a free port of its own.
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
        hub = testing_hub(
            name=name or f"hub{len(hubs) + 1}",
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
            binary=konstruktor_bin,
            # Kept: leaving the session leaves the hub exactly as it is.
            policy="manual" if keep else "testing",
        )
        hub.enter()
        # Before anything can fail: whatever was started is this session's to remove.
        hubs.append(hub)
        try:
            hub.up()
            hub.wait_ready(timeout)
        except DokkerError:
            # Written and started, but not answering: say why before it is gone.
            with contextlib.suppress(Exception):
                print(hub.logs(tail=120).stdout)
            raise
        return hub

    yield factory

    reporter = pytestconfig.pluginmanager.get_plugin("terminalreporter")
    # Last made, first left: the first hub entered owns the loop the others run on.
    for hub in reversed(hubs):
        if keep and reporter is not None and hub.project.exists:
            reporter.write_line(f"konstruktor: kept {hub.gateway_url} in {hub.directory}")
        try:
            hub.exit()
        except Exception as error:  # noqa: BLE001 -- one hub that will not go must not keep the others
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
        with contextlib.suppress(Exception):
            report.sections.append((f"konstruktor logs: {hub.identifier}", hub.logs(tail=80).stdout))
