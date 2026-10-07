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
it, reads back what it wrote, and hands you the result as a
[dokker](https://github.com/jhnnsrs/dokker) deployment to look into.

## A deployment from Python

```python
from konstruktor import testing_hub

with testing_hub(services=["rekuest", "mikro"], redeem_tokens=2) as hub:
    hub.up()                        # written, then started
    hub.wait_ready()                # until everything a client opens answers

    hub.fakts_url                   # http://localhost:<port> — where an app is pointed
    hub.redeem_token("my-app")      # what the app trades for a client, with no browser
    hub.services["mikro"].url       # http://localhost:<port>/mikro
# gone: containers, data and folder
```

A hub is a **self-contained hub**: the services you name, and a coordination server of
their own, behind one gateway on one port (`konstruktor hub create --server local`).
Nobody has to accept it and nothing leaves the machine.

Entering the block does nothing. The block says what happens, and leaving it undoes
exactly that:

| | made by | on leaving the block |
|---|---|---|
| a hub for a test | `testing_hub(...)` | destroyed, with its data and its folder |
| a hub you keep | `local_hub("./hub", ...)` | stopped; its folder and data stay |
| a hub that exists | `Hub.load("./hub")` | left as it is |

A hub can also be made from images instead of names. Each image is asked which
service it is, so a client library only has to know the image that hosts it:

```python
testing_hub(service_images=["jhnnsrs/mikro:7"])
```

Rekuest then runs only if one of the images is Rekuest's.

### Looking into it

Inside the block the hub answers for its services:

```python
hub.ps()                                        # its containers, and whether they run
hub.logs("mikro", tail=50).stdout
hub.exec("mikro", "python manage.py check")     # in the running container
hub.run("mikro", "python manage.py shell -c ...")  # in a container of its own
hub.restart("mikro")                            # and wait until it is healthy again
hub.check_health()                              # every service, on its own health address
hub.job("mikro", "plan")                        # a job the image declares

with hub.create_watcher("mikro") as logs:       # what a service writes while you act
    ...
logs.collected_logs
```

Starting, stopping and removing (`up`, `stop`, `down`, `pull`, `destroy`) are the
`konstruktor` commands of the same name, on the whole hub: which containers a hub runs,
and from which builds, is the generator's to say, so `up(services=[...])` is refused.
Everything else above is dokker's, on the compose project in the hub's folder. Each has
an `a`-prefixed async twin (`await hub.aup()`, `async with hub:`).

These run on a loop the block owns, so they need the block. Outside of a `with` — in a
fixture, a notebook — `hub.enter()` and `hub.exit()` are the two halves of it. How to get
in (`fakts_url`, `redeem_token`, `services`, ...) is read from the hub's folder and works
anywhere.

### Developing a service against a hub

```python
from konstruktor import local_hub

with local_hub("./hub", services=["mikro"], mounts={"mikro": "~/Code/mikro-server"}, debug=["mikro"]) as hub:
    hub.up()
    hub.wait_ready()
    input("running — enter to stop")
```

The tree is mounted over the image's own code and the server reloads when a file in it
changes. The folder is written the first time and used as it is from then on.

### Connecting an app

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
name in the stack: `testing_hub(..., images={"rekuest": "jhnnsrs/rekuest:1.2.3"})`.

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

- `konstruktor_hub` is a session-scoped factory taking what `testing_hub` takes. The hub
  it returns is entered, started and answering, so a test can ask it anything above.
  Every hub it makes is destroyed — containers, volumes and folder — when the session ends.
- Tests marked `konstruktor` are skipped where Docker is not running.
- A failed test gets the hub's container logs attached to its report.
- Hubs live in a registry of their own, so they never show up beside your real ones.
- A session that is killed leaves containers behind; the next test hub made on the machine removes them.

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
KONSTRUKTOR_BIN=$PWD/target/debug/konstruktor PYTHONPATH=$PWD/python python -m pytest python/tests
```
