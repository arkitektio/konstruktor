import os
import socket
from pathlib import Path

import pytest

from konstruktor import (
    Hub,
    KonstruktorError,
    KonstruktorNotFoundError,
    NoRedeemTokenLeftError,
    create_hub,
    find_konstruktor_bin,
    free_port,
)


def flag(call: list[str], name: str) -> str:
    return call[call.index(name) + 1]


def test_creating_a_hub_is_one_unattended_command(fake_konstruktor, tmp_path: Path) -> None:
    hub = create_hub(tmp_path / "lab", services=["rekuest", "mikro"], http_port=7190)

    create, wait = fake_konstruktor.calls
    assert create[:3] == ["hub", "create", os.fspath(tmp_path / "lab")]
    # The coordination server runs in the stack: nobody is asked, nothing is opened.
    assert flag(create, "--server") == "local"
    assert flag(create, "--services") == "rekuest,mikro"
    assert flag(create, "--http-port") == "7190"
    assert "--yes" in create and "--no-open" in create
    assert "--no-start" not in create
    # Started is not answering, so it waits before it hands the hub back.
    assert wait[:2] == ["wait", os.fspath(tmp_path / "lab")]
    assert "--json" in wait

    assert hub.fakts_url == "http://localhost:7190"
    # A name: the hub can be logged into at any address it answers on.
    assert hub.issuer == "lok"
    assert hub.services["mikro"].url == "http://localhost:7190/mikro"
    assert hub.services["mikro"].identifier == "live.arkitekt.mikro"
    assert hub.users[0].username == "demo"


def test_the_seed_is_passed_through(fake_konstruktor, tmp_path: Path) -> None:
    hub = create_hub(
        tmp_path / "lab",
        identifier="lab-hub",
        organization="acme",
        user="ada",
        user_password="pass",
        redeem_tokens=3,
    )
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
    create_hub(
        tmp_path / "lab",
        images={"rekuest": "jhnnsrs/rekuest:5.0.1", "lok": "jhnnsrs/lok:3.1.0"},
    )
    create = fake_konstruktor.calls[0]
    pins = [create[i + 1] for i, arg in enumerate(create) if arg == "--image"]
    assert pins == ["rekuest=jhnnsrs/rekuest:5.0.1", "lok=jhnnsrs/lok:3.1.0"]


def test_services_can_be_named_by_the_images_that_host_them(fake_konstruktor, tmp_path: Path) -> None:
    create_hub(
        tmp_path / "hub",
        service_images=["jhnnsrs/mikro:7", "jhnnsrs/rekuest:7"],
        data_dir=tmp_path / "registry",
    )

    args = fake_konstruktor.calls[0]
    assert [args[i + 1] for i, arg in enumerate(args) if arg == "--service-image"] == [
        "jhnnsrs/mikro:7",
        "jhnnsrs/rekuest:7",
    ]
    # The images say which services these are: no names beside them.
    assert "--services" not in args


def test_a_port_is_picked_when_none_is_given(fake_konstruktor, tmp_path: Path) -> None:
    hub = create_hub(tmp_path / "lab")
    port = int(flag(fake_konstruktor.calls[0], "--http-port"))
    assert hub.gateway_url == f"http://localhost:{port}"
    # Free when it was picked: it can be bound.
    with socket.socket() as probe:
        probe.bind(("127.0.0.1", port))


def test_a_free_port_is_free() -> None:
    with socket.socket() as probe:
        probe.bind(("127.0.0.1", free_port()))


def test_a_hub_can_be_written_without_being_started(fake_konstruktor, tmp_path: Path) -> None:
    create_hub(tmp_path / "lab", start=False)
    assert [call[0] for call in fake_konstruktor.calls] == ["hub"]
    assert "--no-start" in fake_konstruktor.calls[0]


def test_starting_without_waiting_returns_at_once(fake_konstruktor, tmp_path: Path) -> None:
    create_hub(tmp_path / "lab", wait=False)
    assert [call[0] for call in fake_konstruktor.calls] == ["hub"]


def test_the_registry_follows_the_hub(fake_konstruktor, tmp_path: Path) -> None:
    hub = create_hub(tmp_path / "lab", data_dir=tmp_path / "registry")
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
    with pytest.raises(KonstruktorError) as failure:
        create_hub(tmp_path / "lab")
    assert failure.value.returncode == 3
    assert "it did not work" in str(failure.value)
    assert "hub create" in str(failure.value)


def test_not_answering_in_time_is_an_error(
    fake_konstruktor, tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.setenv("FAKE_KONSTRUKTOR_FAIL", "wait")
    with pytest.raises(KonstruktorError):
        create_hub(tmp_path / "lab", timeout=5)
    assert flag(fake_konstruktor.calls[1], "--timeout") == "5"


def test_every_app_gets_a_redeem_token_of_its_own(fake_konstruktor, tmp_path: Path) -> None:
    hub = create_hub(tmp_path / "lab", redeem_tokens=2)

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
    hub = create_hub(tmp_path / "lab", redeem_tokens=2)
    greeter = hub.redeem_token("greeter")

    again = Hub.load(tmp_path / "lab")
    assert again.redeem_token("greeter") == greeter
    assert again.redeem_token("caller") != greeter
    # And the first one learns of what the second took.
    with pytest.raises(NoRedeemTokenLeftError):
        hub.redeem_token("one too many")


def test_a_hub_used_as_a_block_is_gone_after_it(fake_konstruktor, tmp_path: Path) -> None:
    with create_hub(tmp_path / "lab") as hub:
        assert hub.fakts_url
    assert fake_konstruktor.calls[-1][:2] == ["destroy", str(tmp_path / "lab")]

    with pytest.raises(RuntimeError, match="in the middle"), create_hub(tmp_path / "other"):
        raise RuntimeError("in the middle")
    assert fake_konstruktor.calls[-1][:2] == ["destroy", str(tmp_path / "other")]


def test_a_service_can_run_from_a_source_tree_on_this_machine(fake_konstruktor, tmp_path: Path) -> None:
    """The tree is put where the hub's own checkout would go, so nothing is cloned."""
    tree = tmp_path / "mikro-server"
    tree.mkdir()
    (tree / "manage.py").write_text("")

    create_hub(tmp_path / "lab", services=["mikro"], debug=["mikro"], mounts={"mikro": tree})

    args = fake_konstruktor.calls[0]
    assert args[args.index("--debug") + 1] == "mikro"
    assert args[args.index("--from-source") + 1] == "mikro"
    assert (tmp_path / "lab" / "mounts" / "mikro" / "manage.py").is_file()
    assert (tmp_path / "lab" / "mounts" / "mikro").resolve() == tree.resolve()

    with pytest.raises(FileNotFoundError, match="nothing to mount"):
        create_hub(tmp_path / "other", mounts={"mikro": tmp_path / "nowhere"})


def test_a_job_and_an_account_are_asked_of_the_hubs_folder(fake_konstruktor, tmp_path: Path) -> None:
    hub = create_hub(tmp_path / "lab", services=["mikro"])

    hub.job("mikro", "ensureadmin", "--quiet")
    assert fake_konstruktor.calls[-1] == ["job", "run", "--in", str(tmp_path / "lab"), "mikro", "ensureadmin", "--", "--quiet"]
    hub.job("mikro", "plan")
    assert fake_konstruktor.calls[-1] == ["job", "run", "--in", str(tmp_path / "lab"), "mikro", "plan"]

    hub.superuser("mikro", "ada", "s3cret-pass")
    assert fake_konstruktor.calls[-1][:4] == ["superuser", "mikro", "--in", str(tmp_path / "lab")]


def test_lifecycle_commands_name_the_folder(fake_konstruktor, tmp_path: Path) -> None:
    hub = create_hub(tmp_path / "lab", start=False)
    folder = os.fspath(tmp_path / "lab")

    hub.up()
    (endpoint,) = hub.wait(timeout=30)
    assert endpoint.ready and endpoint.status == 200
    assert "a log line" in hub.logs("mikro", tail=5)
    hub.stop()
    hub.down()
    hub.down(volumes=True)
    hub.destroy()

    assert fake_konstruktor.calls[1:] == [
        ["up", folder],
        ["wait", folder, "--timeout", "30", "--json"],
        ["logs", folder, "--tail", "5", "--service", "mikro"],
        ["stop", folder],
        ["down", folder],
        # Deleting the data is confirmed, since there is no terminal to ask on.
        ["down", folder, "--volumes", "--yes"],
        ["destroy", folder, "--yes"],
    ]


def test_an_existing_hub_can_be_loaded(fake_konstruktor, tmp_path: Path) -> None:
    created = create_hub(tmp_path / "lab", wait=False)
    loaded = Hub.load(tmp_path / "lab")
    assert loaded.gateway_url == created.gateway_url
    assert loaded.redeem_tokens == created.redeem_tokens


def test_a_folder_without_a_self_contained_hub_is_refused(fake_konstruktor, tmp_path: Path) -> None:
    with pytest.raises(FileNotFoundError, match="--server local"):
        Hub.load(tmp_path)


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
