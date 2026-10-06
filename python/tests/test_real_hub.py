"""The real executable, and a real stack: an app's side of a self-contained hub."""

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
