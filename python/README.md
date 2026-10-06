# konstruktor

Create and manage [Arkitekt](https://arkitekt.live) deployments — the `konstruktor` CLI in a
wheel, and a Python interface to it.

```
pip install konstruktor
```

The wheel carries the CLI itself, so that is the whole install: `konstruktor` is on your
`PATH` (and `python -m konstruktor` runs it), and the Python interface below always drives
the generator it was released with. Docker has to be running.

Everything that decides what a deployment looks like lives in the CLI. This package runs
it and reads back what it wrote — nothing more.

## A deployment from Python

```python
from konstruktor import create_hub

hub = create_hub("./hub", services=["rekuest", "mikro"], redeem_tokens=2)

hub.fakts_url                   # http://localhost:<port> — where an app is pointed
hub.redeem_token("my-app")      # what the app trades for a client, with no browser
hub.services["mikro"].url       # http://localhost:<port>/mikro
hub.logs("mikro")
hub.destroy()
```
A hub can also be made from images instead of names. Each image is asked which
service it is, so a client library only has to know the image that hosts it:

```python
hub = create_hub("./hub", service_images=["jhnnsrs/mikro:7"])
```

Rekuest then runs only if one of the images is Rekuest's.


`create_hub` builds a **self-contained hub**: the services you name, and a coordination
server of their own, behind one gateway on one port (`konstruktor hub create --server
local`). Nobody has to accept it and nothing leaves the machine. It returns once everything
a client opens answers.

Connecting an app to it takes the address and a redeem token:

```python
from arkitekt import App, connect

app = App("my-app", "0.1.0", services=[...])
with connect(app, url=hub.fakts_url, redeem_token=hub.redeem_token("my-app")) as runtime:
    ...
```

or, for an app in another process, `hub.env("my-app")` — `FAKTS_URL` and
`FAKTS_REDEEM_TOKEN`. An app in a container on the hub's own docker network points at
`http://gateway` instead: the hub can be logged into at any address it answers on.

**One redeem token serves one app.** The coordination server pins a token to the first app
that redeems it, so `redeem_tokens=` is how many apps can connect, and
`hub.redeem_token(name)` hands each name its own.

To run a particular build of a service rather than the one a new hub gets, pin it by its
name in the stack: `create_hub(..., images={"rekuest": "jhnnsrs/rekuest:1.2.3"})`.

## With pytest

Installing the package registers a pytest plugin:

```python
import pytest


@pytest.fixture(scope="session")
def hub(konstruktor_hub):
    return konstruktor_hub(services=["rekuest", "mikro"], redeem_tokens=2)


@pytest.mark.konstruktor
def test_my_app(hub):
    ...
```

- `konstruktor_hub` is a session-scoped factory. Every hub it makes is destroyed —
  containers, volumes and folder — when the session ends.
- Tests marked `konstruktor` are skipped where Docker is not running.
- A failed test gets the hub's container logs attached to its report.
- Hubs live in a registry of their own, so they never show up beside your real ones.
- A session that is killed leaves containers behind; the next session removes them.

Which build of a service a run tests against is usually the job's to say, not the suite's.
Set `KONSTRUKTOR_IMAGES` where the suite runs and every hub created there uses those images,
for the services it has:

```
KONSTRUKTOR_IMAGES=rekuest=jhnnsrs/rekuest:1.2.3,mikro=jhnnsrs/mikro:6.1.0 pytest -m konstruktor
```

| option | |
|---|---|
| `--konstruktor-keep` | leave the hubs running after the session, and print where they are |
| `--konstruktor-timeout` | seconds a hub gets to answer after it is started (600) |
| `--konstruktor-bin` | the executable to use, instead of `$KONSTRUKTOR_BIN` or the one in the wheel |

## From a source checkout

```
cargo build -p konstruktor-cli
KONSTRUKTOR_BIN=target/debug/konstruktor PYTHONPATH=python python -m pytest python/tests
```
