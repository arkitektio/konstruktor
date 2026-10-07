import asyncio
import os
import socket
import subprocess
import sys
from pathlib import Path

import pytest
from dokker import HealthCheck, ProjectError

from konstruktor import (
    Hub,
    HubNotCreatedError,
    KonstruktorError,
    KonstruktorNotFoundError,
    NoRedeemTokenLeftError,
    find_konstruktor_bin,
    free_port,
    local_hub,
    testing_hub,
)
from konstruktor.runs import areap_dead_runs, runs_dir


def flag(call: list[str], name: str) -> str:
    return call[call.index(name) + 1]


def verbs(fake) -> list[str]:
    return [call[0] for call in fake.calls]


def test_entering_a_hub_does_nothing(fake_konstruktor, tmp_path: Path) -> None:
    with local_hub(tmp_path / "lab") as hub:
        assert fake_konstruktor.calls == []
        # Nothing is written, so there is nothing to say about how to get in.
        with pytest.raises(HubNotCreatedError, match="hub.create"):
            hub.fakts_url
    assert fake_konstruktor.calls == []


def test_up_writes_the_hub_unattended_and_then_starts_it(fake_konstruktor, tmp_path: Path) -> None:
    with local_hub(tmp_path / "lab", services=["rekuest", "mikro"], http_port=7190) as hub:
        hub.up()

        create, up = fake_konstruktor.calls
        assert create[:3] == ["hub", "create", os.fspath(tmp_path / "lab")]
        # The coordination server runs in the stack: nobody is asked, nothing is opened.
        assert flag(create, "--server") == "local"
        assert flag(create, "--services") == "rekuest,mikro"
        assert flag(create, "--http-port") == "7190"
        assert "--yes" in create and "--no-open" in create
        # Writing and starting are two steps: starting is `up`'s.
        assert "--no-start" in create
        assert up == ["up", os.fspath(tmp_path / "lab")]

        assert hub.fakts_url == "http://localhost:7190"
        # A name: the hub can be logged into at any address it answers on.
        assert hub.issuer == "lok"
        assert hub.services["mikro"].url == "http://localhost:7190/mikro"
        assert hub.services["mikro"].identifier == "live.arkitekt.mikro"
        assert hub.users[0].username == "demo"


def test_a_hub_is_written_once(fake_konstruktor, tmp_path: Path) -> None:
    with local_hub(tmp_path / "lab") as hub:
        hub.create()
        hub.create()
        hub.up()
    with local_hub(tmp_path / "lab") as hub:
        hub.up()
    assert verbs(fake_konstruktor) == ["hub", "up", "stop", "up", "stop"]


def test_the_seed_is_passed_through(fake_konstruktor, tmp_path: Path) -> None:
    with local_hub(
        tmp_path / "lab",
        identifier="lab-hub",
        organization="acme",
        user="ada",
        user_password="pass",
        redeem_tokens=3,
    ) as hub:
        hub.create()
    create = fake_konstruktor.calls[0]
    assert flag(create, "--identifier") == "lab-hub"
    assert flag(create, "--org") == "acme"
    assert flag(create, "--user") == "ada"
    assert flag(create, "--user-password") == "pass"
    assert flag(create, "--redeem-tokens") == "3"
    assert hub.identifier == "lab-hub"
    assert hub.organization == "acme"
    assert len(hub.redeem_tokens) == 3


def test_images_are_pinned_by_compose_service(fake_konstruktor, tmp_path: Path) -> None:
    with local_hub(
        tmp_path / "lab",
        images={"rekuest": "jhnnsrs/rekuest:5.0.1", "lok": "jhnnsrs/lok:3.1.0"},
    ) as hub:
        hub.create()
    create = fake_konstruktor.calls[0]
    pins = [create[i + 1] for i, arg in enumerate(create) if arg == "--image"]
    assert pins == ["rekuest=jhnnsrs/rekuest:5.0.1", "lok=jhnnsrs/lok:3.1.0"]


def test_services_can_be_named_by_the_images_that_host_them(fake_konstruktor, tmp_path: Path) -> None:
    with local_hub(
        tmp_path / "hub",
        service_images=["jhnnsrs/mikro:7", "jhnnsrs/rekuest:7"],
        data_dir=tmp_path / "registry",
    ) as hub:
        hub.create()

    args = fake_konstruktor.calls[0]
    assert [args[i + 1] for i, arg in enumerate(args) if arg == "--service-image"] == [
        "jhnnsrs/mikro:7",
        "jhnnsrs/rekuest:7",
    ]
    # The images say which services these are: no names beside them.
    assert "--services" not in args


def test_a_port_is_picked_when_none_is_given(fake_konstruktor, tmp_path: Path) -> None:
    with local_hub(tmp_path / "lab") as hub:
        hub.create()
    port = int(flag(fake_konstruktor.calls[0], "--http-port"))
    assert hub.gateway_url == f"http://localhost:{port}"
    # Free when it was picked: it can be bound.
    with socket.socket() as probe:
        probe.bind(("127.0.0.1", port))


def test_a_free_port_is_free() -> None:
    with socket.socket() as probe:
        probe.bind(("127.0.0.1", free_port()))


def test_the_registry_follows_the_hub(fake_konstruktor, tmp_path: Path) -> None:
    with local_hub(tmp_path / "lab", data_dir=tmp_path / "registry") as hub:
        hub.up()
        hub.wait_ready()
        hub.destroy()
    # Created in one registry and destroyed through another would refuse: the
    # deployment would not be found.
    assert {entry["data_dir"] for entry in fake_konstruktor.entries} == {
        os.fspath(tmp_path / "registry")
    }


def test_a_failing_command_says_what_the_cli_said(
    fake_konstruktor, tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.setenv("FAKE_KONSTRUKTOR_FAIL", "hub")
    with pytest.raises(KonstruktorError) as failure, local_hub(tmp_path / "lab") as hub:
        hub.create()
    assert failure.value.returncode == 3
    assert "it did not work" in str(failure.value)
    assert "hub create" in str(failure.value)
    assert failure.value.stderr == ["x it did not work"]


def test_not_answering_in_time_is_an_error(
    fake_konstruktor, tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.setenv("FAKE_KONSTRUKTOR_FAIL", "wait")
    with pytest.raises(KonstruktorError), local_hub(tmp_path / "lab") as hub:
        hub.up()
        hub.wait_ready(timeout=5)
    wait = next(call for call in fake_konstruktor.calls if call[0] == "wait")
    assert flag(wait, "--timeout") == "5"


def test_every_app_gets_a_redeem_token_of_its_own(fake_konstruktor, tmp_path: Path) -> None:
    with local_hub(tmp_path / "lab", redeem_tokens=2) as hub:
        hub.create()

    # How to get in is read from the folder: it needs no block.
    greeter = hub.redeem_token("greeter")
    caller = hub.redeem_token("caller")
    assert greeter != caller
    # And keeps it: an app that reconnects has to redeem the same one again.
    assert hub.redeem_token("greeter") == greeter
    assert hub.env("caller") == {"FAKTS_URL": hub.fakts_url, "FAKTS_REDEEM_TOKEN": caller}

    with pytest.raises(NoRedeemTokenLeftError, match="more `redeem_tokens`"):
        hub.redeem_token("one too many")


def test_a_token_stays_with_its_app_for_whoever_opens_the_hub_again(fake_konstruktor, tmp_path: Path) -> None:
    """The coordination server pins a token to the app that redeemed it, for as long as the
    hub lives: so which app has which is written down in the hub, not kept in one process."""
    with local_hub(tmp_path / "lab", redeem_tokens=2) as hub:
        hub.create()
    greeter = hub.redeem_token("greeter")

    again = Hub.load(tmp_path / "lab")
    assert again.redeem_token("greeter") == greeter
    assert again.redeem_token("caller") != greeter
    # And the first one learns of what the second took.
    with pytest.raises(NoRedeemTokenLeftError):
        hub.redeem_token("one too many")


def test_a_testing_hub_is_gone_after_its_block(fake_konstruktor) -> None:
    with testing_hub(services=["mikro"]) as hub:
        hub.up()
        folder = hub.directory
        assert hub.identifier == "hub"
        # Not the compose project of anybody's real hub.
        assert folder.name.startswith("test-") and folder.name != "hub"
        assert folder.parent.parent.parent == runs_dir()

    assert verbs(fake_konstruktor) == ["hub", "up", "down", "destroy"]
    # The data goes with the containers, confirmed since there is no terminal to ask on.
    assert fake_konstruktor.calls[2] == ["down", os.fspath(folder), "--volumes", "--yes"]
    # Its coordination server is its own, and already down: nobody is asked to forget it.
    assert fake_konstruktor.calls[3] == ["destroy", os.fspath(folder), "--yes", "--local-only"]
    assert not folder.exists()
    # A registry of its own, so the hub never shows up beside the user's real ones.
    registries = {entry["data_dir"] for entry in fake_konstruktor.entries}
    assert len(registries) == 1 and None not in registries


def test_a_testing_hub_is_gone_when_its_block_raises(fake_konstruktor) -> None:
    with pytest.raises(RuntimeError, match="in the middle"), testing_hub() as hub:
        hub.up()
        raise RuntimeError("in the middle")
    assert verbs(fake_konstruktor)[-2:] == ["down", "destroy"]


def test_two_testing_hubs_are_two_hubs(fake_konstruktor) -> None:
    """Even of one name: the second never takes the folder, and with it the services, of the first."""
    with testing_hub(services=["mikro"]) as first, testing_hub(services=["fluss"]) as second:
        first.create()
        second.create()
        assert first.directory != second.directory
        assert set(first.services) == {"lok", "mikro"}
        assert set(second.services) == {"lok", "fluss"}
    assert verbs(fake_konstruktor) == ["hub", "hub", "destroy", "destroy"]


def test_a_testing_hub_that_was_only_written_is_removed_too(fake_konstruktor) -> None:
    with testing_hub() as hub:
        hub.create()
    assert verbs(fake_konstruktor) == ["hub", "destroy"]


def test_a_hub_that_would_not_start_is_still_removed(
    fake_konstruktor, monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.setenv("FAKE_KONSTRUKTOR_FAIL", "up")
    with pytest.raises(KonstruktorError), testing_hub() as hub:
        hub.up()
    assert verbs(fake_konstruktor) == ["hub", "up", "down", "destroy"]


def test_a_local_hub_is_stopped_and_kept(fake_konstruktor, tmp_path: Path) -> None:
    with local_hub(tmp_path / "lab") as hub:
        hub.up()
    assert verbs(fake_konstruktor) == ["hub", "up", "stop"]
    assert (tmp_path / "lab").is_dir()

    # And its data is kept when it is taken down by hand.
    with hub:
        hub.down()
    assert fake_konstruktor.calls[-1] == ["down", os.fspath(tmp_path / "lab")]


def test_a_loaded_hub_is_left_as_it_is(fake_konstruktor, tmp_path: Path) -> None:
    with local_hub(tmp_path / "lab") as created:
        created.create()

    with Hub.load(tmp_path / "lab") as loaded:
        loaded.up()
        assert loaded.gateway_url == created.gateway_url
        assert loaded.redeem_tokens == created.redeem_tokens
    assert verbs(fake_konstruktor) == ["hub", "up"]


def test_a_folder_without_a_self_contained_hub_is_refused(fake_konstruktor, tmp_path: Path) -> None:
    with pytest.raises(FileNotFoundError, match="--server local"):
        Hub.load(tmp_path)


def test_a_service_can_run_from_a_source_tree_on_this_machine(fake_konstruktor, tmp_path: Path) -> None:
    """The tree is put where the hub's own checkout would go, so nothing is cloned."""
    tree = tmp_path / "mikro-server"
    tree.mkdir()
    (tree / "manage.py").write_text("")

    with local_hub(tmp_path / "lab", services=["mikro"], debug=["mikro"], mounts={"mikro": tree}) as hub:
        hub.create()

    args = fake_konstruktor.calls[0]
    assert args[args.index("--debug") + 1] == "mikro"
    assert args[args.index("--from-source") + 1] == "mikro"
    assert (tmp_path / "lab" / "mounts" / "mikro" / "manage.py").is_file()
    assert (tmp_path / "lab" / "mounts" / "mikro").resolve() == tree.resolve()

    with (
        pytest.raises(FileNotFoundError, match="nothing to mount"),
        local_hub(tmp_path / "other", mounts={"mikro": tmp_path / "nowhere"}) as other,
    ):
        other.create()


def test_a_job_and_an_account_are_asked_of_the_hubs_folder(fake_konstruktor, tmp_path: Path) -> None:
    with local_hub(tmp_path / "lab", services=["mikro"]) as hub:
        hub.job("mikro", "ensureadmin", "--quiet")
        assert fake_konstruktor.calls[-1] == ["job", "run", "--in", str(tmp_path / "lab"), "mikro", "ensureadmin", "--", "--quiet"]
        hub.job("mikro", "plan")
        assert fake_konstruktor.calls[-1] == ["job", "run", "--in", str(tmp_path / "lab"), "mikro", "plan"]

        hub.superuser("mikro", "ada", "s3cret-pass")
        assert fake_konstruktor.calls[-1][:4] == ["superuser", "mikro", "--in", str(tmp_path / "lab")]


def test_lifecycle_commands_name_the_folder(fake_konstruktor, tmp_path: Path) -> None:
    folder = os.fspath(tmp_path / "lab")
    with local_hub(tmp_path / "lab") as hub:
        hub.create()
        hub.pull()
        hub.up(stop_on_exit=False)
        (endpoint,) = hub.wait_ready(timeout=30)
        assert endpoint.ready and endpoint.status == 200
        hub.stop()
        hub.down()
        hub.down(volumes=True)
        hub.destroy()

    assert fake_konstruktor.calls[1:] == [
        ["pull", folder],
        ["up", folder],
        ["wait", folder, "--timeout", "30", "--json"],
        ["stop", folder],
        ["down", folder],
        # Deleting the data is confirmed, since there is no terminal to ask on.
        ["down", folder, "--volumes", "--yes"],
        ["destroy", folder, "--yes", "--local-only"],
    ]


def test_up_can_wait_for_the_hub_to_answer(fake_konstruktor, tmp_path: Path) -> None:
    folder = os.fspath(tmp_path / "lab")
    with local_hub(tmp_path / "lab") as hub:
        hub.up(wait=True, wait_timeout=30)
    assert fake_konstruktor.calls[1:3] == [["up", folder], ["wait", folder, "--timeout", "30"]]


def test_a_hub_is_started_whole(fake_konstruktor, tmp_path: Path) -> None:
    """Which containers a hub runs, and from which builds, is the generator's to say."""
    with local_hub(tmp_path / "lab") as hub:
        with pytest.raises(ProjectError, match="services"):
            hub.up(services=["mikro"])
        with pytest.raises(ProjectError, match="scales"):
            hub.up(scales={"mikro": 2})
        with pytest.raises(ProjectError, match="services"):
            hub.pull(services=["mikro"])
    assert "up" not in verbs(fake_konstruktor) and "pull" not in verbs(fake_konstruktor)


def test_every_service_gets_a_health_check_on_its_own_address(fake_konstruktor, tmp_path: Path) -> None:
    asked = HealthCheck(url="http://localhost/mine", service="mikro")
    hub = local_hub(tmp_path / "lab", services=["mikro"], http_port=7190)
    hub.health_checks.append(asked)
    with hub:
        hub.create()
        hub.create()
    checks = {check.service: check for check in hub.health_checks}
    assert set(checks) == {"lok", "mikro"} and len(hub.health_checks) == 2
    assert checks["lok"].url == "http://localhost:7190/lok/ht?format=json"
    # One that was asked for is left as it was.
    assert checks["mikro"] is asked


def test_compose_looks_at_the_stack_konstruktor_runs(fake_konstruktor, tmp_path: Path) -> None:
    """The project is the folder's name for both, or they would be two different stacks."""
    hub = local_hub(tmp_path / "lab")
    cli = asyncio.run(hub.aget_cli())
    assert cli.compose_files == [tmp_path / "lab" / "docker-compose.yaml"]
    assert cli.compose_project_directory == tmp_path / "lab"
    assert cli.compose_project_name is None


def _dead_pid() -> int:
    # A pid that has certainly exited: a child that already finished.
    child = subprocess.Popen([sys.executable, "-c", "pass"])
    child.wait()
    return child.pid


def test_a_dead_process_hubs_are_destroyed(fake_konstruktor, tmp_path: Path) -> None:
    runs = tmp_path / "runs"
    dead = runs / f"{_dead_pid()}-abc"
    with local_hub(dead / "hubs" / "hub1", data_dir=dead / "registry") as orphan:
        orphan.create()
    alive = runs / f"{os.getpid()}-def"
    with local_hub(alive / "hubs" / "hub1", data_dir=alive / "registry") as living:
        living.create()

    removed = asyncio.run(areap_dead_runs(runs, fake_konstruktor.path))

    assert removed == [dead]
    assert not dead.exists()
    assert living.directory.is_dir()
    destroyed = [entry for entry in fake_konstruktor.entries if entry["args"][0] == "destroy"]
    # With the registry it was created in: any other would refuse to find it.
    assert [(e["args"][1], e["data_dir"]) for e in destroyed] == [
        (os.fspath(orphan.directory), os.fspath(dead / "registry"))
    ]


def test_the_next_testing_hub_removes_what_a_dead_process_left(fake_konstruktor) -> None:
    dead = runs_dir() / f"{_dead_pid()}-abc"
    with local_hub(dead / "hubs" / "hub1", data_dir=dead / "registry") as orphan:
        orphan.create()

    with testing_hub() as hub:
        hub.create()
        assert not dead.exists()
        assert verbs(fake_konstruktor) == ["hub", "destroy", "hub"]
        assert hub.directory.is_dir()


def test_the_executable_is_found_where_it_is_named(
    fake_konstruktor, tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    assert find_konstruktor_bin() == fake_konstruktor.path
    assert find_konstruktor_bin(fake_konstruktor.path) == fake_konstruktor.path
    # Named and missing is an error, not a reason to go looking elsewhere.
    with pytest.raises(KonstruktorNotFoundError):
        find_konstruktor_bin(tmp_path / "nope")
    monkeypatch.setenv("KONSTRUKTOR_BIN", os.fspath(tmp_path / "nope"))
    with pytest.raises(KonstruktorNotFoundError):
        find_konstruktor_bin()
