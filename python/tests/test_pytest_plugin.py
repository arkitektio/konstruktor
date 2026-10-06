import os
import subprocess
import sys
from pathlib import Path

import pytest

from konstruktor import create_hub
from konstruktor.pytest_plugin import reap_dead_runs


def test_the_fixture_builds_a_hub_and_destroys_it(fake_konstruktor, pytester: pytest.Pytester) -> None:
    pytester.makepyfile(
        """
        import pytest

        @pytest.fixture(scope="session")
        def hub(konstruktor_hub):
            return konstruktor_hub(services=["rekuest", "mikro"], redeem_tokens=2)

        def test_first(hub):
            assert set(hub.services) == {"lok", "rekuest", "mikro"}
            assert hub.identifier == "hub1"
            # Not the compose project of anybody's real hub: see the plugin.
            assert hub.directory.name.startswith("pytest-") and hub.directory.name != "hub1"

        def test_second(hub):
            assert hub.redeem_token("a") != hub.redeem_token("b")
        """
    )
    result = pytester.runpytest_subprocess()
    result.assert_outcomes(passed=2)

    commands = [call[0] for call in fake_konstruktor.calls]
    # One hub for the session, not one per test — and gone when it ends.
    assert commands == ["hub", "wait", "destroy"]
    # A registry of its own, so the hub never shows up beside the user's real ones.
    registries = {entry["data_dir"] for entry in fake_konstruktor.entries}
    assert len(registries) == 1 and None not in registries


def test_keep_leaves_the_hub_and_says_where_it_is(fake_konstruktor, pytester: pytest.Pytester) -> None:
    pytester.makepyfile(
        """
        def test_it(konstruktor_hub):
            konstruktor_hub()
        """
    )
    result = pytester.runpytest_subprocess("--konstruktor-keep")
    result.assert_outcomes(passed=1)
    result.stdout.fnmatch_lines(["konstruktor: kept http://localhost:* in *"])
    assert "destroy" not in [call[0] for call in fake_konstruktor.calls]

    # Kept where the session put it — under this test's own temp directory.
    (create,) = [call for call in fake_konstruktor.calls if call[:2] == ["hub", "create"]]
    assert Path(create[2]).is_dir()
    assert Path(create[2]).parent.parent.parent.name == "konstruktor-pytest"


def test_a_marked_test_is_skipped_without_the_executable(
    pytester: pytest.Pytester, tmp_path: Path
) -> None:
    pytester.makepyfile(
        """
        import pytest

        @pytest.mark.konstruktor
        def test_it(konstruktor_hub):
            konstruktor_hub()
        """
    )
    result = pytester.runpytest_subprocess(
        "--konstruktor-bin", os.fspath(tmp_path / "nope"), "-rs"
    )
    # Either Docker is missing or the executable is: skipped both ways, never an error.
    result.assert_outcomes(skipped=1)


def test_a_dead_sessions_hubs_are_destroyed_by_the_next(fake_konstruktor, tmp_path: Path) -> None:
    runs = tmp_path / "runs"

    # A pid that has certainly exited: a child that already finished.
    child = subprocess.Popen([sys.executable, "-c", "pass"])
    child.wait()
    dead = runs / f"{child.pid}-abc"
    orphan = create_hub(dead / "hubs" / "hub1", data_dir=dead / "registry", wait=False)

    alive = runs / f"{os.getpid()}-def"
    living = create_hub(alive / "hubs" / "hub1", data_dir=alive / "registry", wait=False)

    removed = reap_dead_runs(runs, fake_konstruktor.path)

    assert removed == [dead]
    assert not dead.exists()
    assert living.directory.is_dir()
    destroyed = [entry for entry in fake_konstruktor.entries if entry["args"][0] == "destroy"]
    # With the registry it was created in: any other would refuse to find it.
    assert [(e["args"][1], e["data_dir"]) for e in destroyed] == [
        (os.fspath(orphan.directory), os.fspath(dead / "registry"))
    ]
