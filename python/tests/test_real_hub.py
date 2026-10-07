"""The real executable, and a real stack: an app's side of a self-contained hub, and a look inside it."""

import json
import urllib.parse
import urllib.request

import pytest

from konstruktor import Hub

pytestmark = pytest.mark.konstruktor


@pytest.fixture(scope="session")
def hub(konstruktor_hub) -> Hub:
    return konstruktor_hub(services=["rekuest"], redeem_tokens=1)


def _json(request: urllib.request.Request | str) -> dict:
    with urllib.request.urlopen(request, timeout=30) as response:
        return json.load(response)


def test_an_app_redeems_a_token_and_rekuest_accepts_it(hub: Hub) -> None:
    well_known = _json(f"{hub.fakts_url}/.well-known/fakts")
    # Reachable from here, which the compose-internal name of the coordination
    # server would not be.
    assert well_known["token_endpoint"].startswith(hub.gateway_url)

    manifest = {
        "identifier": "live.arkitekt.konstruktor.pytest",
        "version": "1.0.0",
        "scopes": ["openid"],
        "requirements": [{"key": "rekuest", "service": "live.arkitekt.rekuest"}],
    }
    grant = _json(
        urllib.request.Request(
            well_known["token_endpoint"],
            data=urllib.parse.urlencode(
                {
                    "grant_type": "urn:fakts:grant-type:redeem",
                    "redeem_token": hub.redeem_token("pytest"),
                    "manifest": json.dumps(manifest),
                }
            ).encode(),
        )
    )
    assert grant["statuses"] == {"rekuest": "granted"}

    answer = _json(
        urllib.request.Request(
            f"{hub.services['rekuest'].url}/graphql",
            data=json.dumps({"query": "{ agents { id } }"}).encode(),
            headers={
                "Content-Type": "application/json",
                "Authorization": f"Bearer {grant['access_token']}",
            },
        )
    )
    assert "errors" not in answer, answer
    assert isinstance(answer["data"]["agents"], list)


def test_the_hubs_containers_can_be_listed(hub: Hub) -> None:
    running = {container.service: container for container in hub.ps()}
    assert {"lok", "rekuest"} <= set(running)
    assert running["rekuest"].is_running
    # The same stack konstruktor started, not a second one beside it.
    assert running["rekuest"].name.startswith(hub.directory.name)


def test_a_service_can_be_asked_and_read(hub: Hub) -> None:
    said = hub.exec("rekuest", ["python", "-c", "print('from inside')"])
    assert "from inside" in said.stdout
    assert hub.logs("rekuest", tail=20)


def test_every_service_is_healthy_by_its_own_account(hub: Hub) -> None:
    assert {check.service for check in hub.health_checks} == set(hub.services)
    hub.check_health()


def test_a_service_can_be_restarted_and_watched_coming_back(hub: Hub) -> None:
    with hub.create_watcher("rekuest", wait_for_first_log=False) as watcher:
        hub.restart("rekuest", await_health=False)
        hub.wait_ready(120)
    assert watcher.collected_logs
    hub.check_health(services=["rekuest"])
