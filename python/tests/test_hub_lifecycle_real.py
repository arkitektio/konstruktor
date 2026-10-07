"""The real executable, and a real stack: what a hub does around its ``with`` block.

``test_real_hub.py`` asks a hub the session keeps; these make their own, one at a time,
to see what entering, leaving and coming back do to the containers and the folder. Each
is gone before the next is made, so no more than one runs beside the session's.
"""

import asyncio
import subprocess
import uuid
from pathlib import Path

import pytest

from konstruktor import Hub, local_hub, testing_hub

pytestmark = pytest.mark.konstruktor


def _states(project: str) -> list[str]:
    """The state of every container of a compose project, as Docker says: no hub is asked."""
    said = subprocess.run(
        ["docker", "ps", "--all", "--filter", f"label=com.docker.compose.project={project}", "--format", "{{.State}}"],
        capture_output=True,
        text=True,
        check=True,
    )
    return said.stdout.split()


def test_a_testing_hub_is_there_for_its_block_and_gone_after_it() -> None:
    with testing_hub(services=["rekuest"], name="block") as hub:
        # Entering made nothing: not a folder, not a container.
        assert not hub.directory.exists()

        hub.create()
        folder, project = hub.directory, hub.directory.name
        assert folder.is_dir() and hub.fakts_url.startswith("http://localhost:")
        # Written is not started.
        assert _states(project) == [] and hub.ps() == []

        hub.up()
        endpoints = hub.wait_ready(300)
        assert endpoints and all(endpoint.ready for endpoint in endpoints)
        running = {container.service for container in hub.ps() if container.is_running}
        assert {"lok", "rekuest"} <= running
        hub.check_health()

    # Containers, folder and the run's registry entry: nothing is left to find.
    assert _states(project) == []
    assert not folder.exists()


def test_a_testing_hub_is_gone_when_its_block_raises() -> None:
    with pytest.raises(RuntimeError, match="in the middle"), testing_hub(services=["rekuest"], name="raises") as hub:
        hub.up()
        folder, project = hub.directory, hub.directory.name
        assert _states(project)
        raise RuntimeError("in the middle")

    assert _states(project) == []
    assert not folder.exists()


def test_a_local_hub_is_stopped_kept_and_found_again(tmp_path: Path) -> None:
    # The folder's name is the compose project: one nobody else on this machine has.
    folder = tmp_path / f"kept-{uuid.uuid4().hex[:8]}"
    registry = tmp_path / "registry"
    project = folder.name

    try:
        with local_hub(folder, services=["rekuest"], redeem_tokens=2, data_dir=registry) as hub:
            hub.up()
            hub.wait_ready(300)
            address, token = hub.fakts_url, hub.redeem_token("first")
            assert "running" in _states(project)

        # Stopped, not removed: the containers are there, and so is everything in the folder.
        states = _states(project)
        assert states and "running" not in states
        assert folder.is_dir()

        with Hub.load(folder, data_dir=registry) as again:
            # The same hub: its address, and which app holds which token.
            assert again.fakts_url == address
            assert again.redeem_token("first") == token
            assert again.redeem_token("second") != token

            again.pull()
            again.up()
            again.wait_ready(300)
            assert "running" in _states(project)
            # Its data survived the stop: the database was not made anew.
            assert again.job("rekuest", "plan") is not None

        # A loaded hub is left as it is.
        assert "running" in _states(project)
    finally:
        if (folder / "secrets" / "access.json").is_file():
            with Hub.load(folder, data_dir=registry) as last:
                last.destroy()

    assert _states(project) == []
    assert not folder.exists()


def test_a_hub_is_driven_from_async_code() -> None:
    async def drive() -> tuple[Path, str]:
        async with testing_hub(services=["rekuest"], name="async") as hub:
            await hub.aup()
            endpoints = await hub.await_ready(300)
            assert all(endpoint.ready for endpoint in endpoints)

            containers = await hub.aps()
            assert {"lok", "rekuest"} <= {container.service for container in containers if container.is_running}
            said = await hub.aexec("rekuest", ["python", "-c", "print('from inside')"])
            assert "from inside" in said.stdout
            assert await hub.alogs("rekuest", tail=10)
            await hub.acheck_health()
            assert await hub.aget_port("gateway", 80) == int(hub.gateway_url.rsplit(":", 1)[1])
            return hub.directory, hub.directory.name

    folder, project = asyncio.run(drive())

    assert _states(project) == []
    assert not folder.exists()


def test_a_kept_session_hub_is_left_running_where_it_says(pytester: pytest.Pytester) -> None:
    pytester.makeconftest(
        """
        from importlib.metadata import entry_points

        if not any(entry.name == "konstruktor" for entry in entry_points(group="pytest11")):
            pytest_plugins = ["konstruktor.pytest_plugin"]
        """
    )
    pytester.makepyfile(
        """
        def test_it(konstruktor_hub):
            hub = konstruktor_hub(services=["rekuest"])
            assert hub.ps()
        """
    )
    result = pytester.runpytest_subprocess("--konstruktor-keep", "-p", "no:cacheprovider")
    result.assert_outcomes(passed=1)

    (kept,) = [line for line in result.stdout.lines if line.startswith("konstruktor: kept ")]
    folder = Path(kept.rsplit(" in ", 1)[1].strip())
    project = folder.name
    try:
        assert folder.is_dir()
        assert "running" in _states(project)
    finally:
        # Recorded in the registry of the session that made it: two folders up from the hub.
        with Hub.load(folder, data_dir=folder.parent.parent / "registry") as hub:
            hub.destroy()

    assert _states(project) == []
    assert not folder.exists()
