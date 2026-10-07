import os
from pathlib import Path

import pytest


@pytest.fixture(autouse=True)
def _the_plugin_is_loaded_in_the_inner_run(pytester: pytest.Pytester) -> None:
    """An installed package announces its plugin to pytest; a source checkout on the path
    does not. The suites pytest runs inside these tests load it themselves in that case, so
    the tests say the same thing from a wheel and from a checkout."""
    pytester.makeconftest(
        """
        from importlib.metadata import entry_points

        if not any(entry.name == "konstruktor" for entry in entry_points(group="pytest11")):
            pytest_plugins = ["konstruktor.pytest_plugin"]
        """
    )


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
            assert hub.directory.name.startswith("test-") and hub.directory.name != "hub1"
            # Entered for the session: it can be asked things between tests.
            assert [check.service for check in hub.health_checks] == ["lok", "rekuest", "mikro"]

        def test_second(hub):
            assert hub.redeem_token("a") != hub.redeem_token("b")
        """
    )
    result = pytester.runpytest_subprocess()
    result.assert_outcomes(passed=2)

    commands = [call[0] for call in fake_konstruktor.calls]
    # One hub for the session, not one per test — and gone when it ends.
    assert commands == ["hub", "up", "wait", "down", "destroy"]
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
    assert [call[0] for call in fake_konstruktor.calls] == ["hub", "up", "wait"]

    # Kept where the session put it — under this test's own temp directory.
    (create,) = [call for call in fake_konstruktor.calls if call[:2] == ["hub", "create"]]
    assert Path(create[2]).is_dir()
    assert Path(create[2]).parent.parent.parent.name == "konstruktor-runs"


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


def test_a_hub_that_does_not_answer_fails_its_test_and_is_removed(
    fake_konstruktor, pytester: pytest.Pytester, monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.setenv("FAKE_KONSTRUKTOR_FAIL", "wait")
    pytester.makepyfile(
        """
        def test_it(konstruktor_hub):
            konstruktor_hub()
        """
    )
    result = pytester.runpytest_subprocess()
    result.assert_outcomes(errors=0, failed=1)
    assert [call[0] for call in fake_konstruktor.calls] == ["hub", "up", "wait", "down", "destroy"]


def test_every_hub_of_a_session_is_destroyed(fake_konstruktor, pytester: pytest.Pytester) -> None:
    pytester.makepyfile(
        """
        def test_it(konstruktor_hub):
            first = konstruktor_hub(services=["mikro"])
            second = konstruktor_hub(services=["fluss"])
            assert (first.identifier, second.identifier) == ("hub1", "hub2")
            assert first.ps is not None and second.directory != first.directory
        """
    )
    result = pytester.runpytest_subprocess()
    result.assert_outcomes(passed=1)

    commands = [call[0] for call in fake_konstruktor.calls]
    assert commands == ["hub", "up", "wait", "hub", "up", "wait", "down", "destroy", "down", "destroy"]
    created = [call[2] for call in fake_konstruktor.calls if call[0] == "hub"]
    destroyed = [call[1] for call in fake_konstruktor.calls if call[0] == "destroy"]
    # Last made, first removed -- and both of them.
    assert destroyed == created[::-1]
