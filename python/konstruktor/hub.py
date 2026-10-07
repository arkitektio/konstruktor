"""A self-contained Arkitekt hub, from Python.

Everything here runs the ``konstruktor`` executable and reads back what it wrote.
Nothing about what a deployment looks like is decided on this side: which services
exist, how they are wired, what the coordination server is seeded with -- that is
the generator's, and the generator is the binary.
"""

import json
import os
import socket
import subprocess
from collections.abc import Mapping, Sequence
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any

from konstruktor._binary import find_konstruktor_bin

#: Where ``hub create --server local`` writes how to reach the hub.
ACCESS_FILE = "secrets/access.json"
#: Where this package writes down which app was handed which redeem token. Beside the
#: access document, because it is as secret, and in the hub's folder, because it is the
#: hub's: a token is pinned to its app by the coordination server, for as long as the hub
#: lives and whichever process asks.
TAKEN_FILE = "secrets/redeem-tokens-taken.json"


class KonstruktorError(RuntimeError):
    """A ``konstruktor`` command failed.

    The message carries what the command wrote to stderr, which is where it says
    what went wrong and what to do about it.
    """

    def __init__(self, command: Sequence[str], returncode: int, stderr: str) -> None:
        self.command = list(command)
        self.returncode = returncode
        self.stderr = stderr
        super().__init__(
            f"`{' '.join(self.command)}` failed with exit code {returncode}:\n{stderr.strip()}"
        )


class NoRedeemTokenLeftError(LookupError):
    """Every redeem token of the hub is already taken by another app."""


@dataclass(frozen=True)
class Account:
    """A username and its password."""

    username: str
    password: str


@dataclass(frozen=True)
class Service:
    """One service of a hub, as it is reached through the gateway."""

    name: str
    #: The identifier an app requires it by, e.g. ``live.arkitekt.mikro``.
    identifier: str
    url: str
    health_url: str


@dataclass(frozen=True)
class Endpoint:
    """One endpoint a client opens, and what it last answered."""

    name: str
    url: str
    #: The last HTTP status, or ``None`` when it did not answer at all.
    status: int | None
    ready: bool
    #: Why it is not ready although it answered, when that needs saying.
    detail: str | None = None


def free_port() -> int:
    """A TCP port nothing is listening on right now.

    A hub's address is part of every token it issues, so the port is decided
    before the hub exists rather than assigned by Docker afterwards.
    """
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as probe:
        probe.bind(("127.0.0.1", 0))
        return int(probe.getsockname()[1])


def _run(
    binary: Path,
    args: Sequence[str],
    *,
    data_dir: Path | None,
    timeout: float | None = None,
) -> subprocess.CompletedProcess[str]:
    """Run one ``konstruktor`` command to completion.

    Raises:
        KonstruktorError: If it exits with anything but zero.
    """
    env = dict(os.environ)
    if data_dir is not None:
        env["KONSTRUKTOR_DATA_DIR"] = os.fspath(data_dir)
    command = [os.fspath(binary), *args]
    result = subprocess.run(
        command,
        env=env,
        capture_output=True,
        text=True,
        stdin=subprocess.DEVNULL,
        timeout=timeout,
        check=False,
    )
    if result.returncode != 0:
        raise KonstruktorError(command, result.returncode, result.stderr)
    return result


def _read_taken(directory: Path) -> dict[str, str]:
    """Which app was handed which redeem token, as written down in the hub's folder."""
    try:
        taken = json.loads((directory / TAKEN_FILE).read_text())
    except (OSError, ValueError):
        return {}
    return {str(app): str(token) for app, token in taken.items()} if isinstance(taken, dict) else {}


def _write_taken(directory: Path, taken: Mapping[str, str]) -> None:
    """Write that down, readable by its owner alone like the access document beside it."""
    path = directory / TAKEN_FILE
    path.write_text(json.dumps(dict(sorted(taken.items())), indent=2) + "\n")
    path.chmod(0o600)


@dataclass
class Hub:
    """A self-contained hub on this machine: where it is, and how to get in.

    Made by :func:`create_hub`, or by :meth:`Hub.load` for one that already exists.
    The fields are what the hub's own ``secrets/access.json`` says; the methods run
    the ``konstruktor`` command of the same name on its folder.
    """

    directory: Path
    #: The hub's name inside its organization.
    identifier: str
    #: Where the gateway answers, e.g. ``http://localhost:7190``.
    gateway_url: str
    #: What an app is pointed at (``FAKTS_URL``).
    fakts_url: str
    #: The ``iss`` of every token the hub's coordination server signs: a name, not
    #: an address. The hub can be logged into at any address it answers on.
    issuer: str
    organization: str
    #: The Django administrator of every service.
    admin: Account
    #: The accounts apps act as. The first one owns the organization.
    users: tuple[Account, ...]
    #: Every redeem token there is. Prefer :meth:`redeem_token`, which hands each
    #: app its own.
    redeem_tokens: tuple[str, ...]
    services: Mapping[str, Service]
    #: The registry the hub is recorded in, when it is not the user's own.
    data_dir: Path | None = None
    binary: Path = field(default_factory=find_konstruktor_bin)
    _taken: dict[str, str] = field(default_factory=dict, repr=False, compare=False)

    @classmethod
    def load(
        cls,
        directory: str | os.PathLike[str],
        *,
        data_dir: str | os.PathLike[str] | None = None,
        binary: str | os.PathLike[str] | None = None,
    ) -> "Hub":
        """The hub in ``directory``, read from the access document it wrote.

        Raises:
            FileNotFoundError: If the folder holds no self-contained hub.
        """
        directory = Path(directory)
        access_file = directory / ACCESS_FILE
        if not access_file.is_file():
            raise FileNotFoundError(
                f"{directory} holds no self-contained hub: there is no {ACCESS_FILE}. "
                "Only a hub created with `--server local` writes one."
            )
        access: dict[str, Any] = json.loads(access_file.read_text())
        return cls(
            directory=directory,
            identifier=access["hub"],
            gateway_url=access["gateway_url"],
            fakts_url=access["fakts_url"],
            issuer=access["issuer"],
            organization=access["organization"],
            admin=Account(**access["admin"]),
            users=tuple(Account(**user) for user in access["users"]),
            redeem_tokens=tuple(access["redeem_tokens"]),
            services={
                name: Service(name=name, **service)
                for name, service in access["services"].items()
            },
            data_dir=Path(data_dir) if data_dir is not None else None,
            binary=find_konstruktor_bin(binary),
            _taken=_read_taken(directory),
        )

    def __enter__(self) -> "Hub":
        """Use the hub for the length of a ``with`` block, and destroy it afterwards."""
        return self

    def __exit__(self, *exc: object) -> None:
        """Destroy the hub, whether or not the block raised."""
        self.destroy()

    def _konstruktor(
        self, *args: str, timeout: float | None = None
    ) -> subprocess.CompletedProcess[str]:
        return _run(self.binary, args, data_dir=self.data_dir, timeout=timeout)

    # -- getting in -----------------------------------------------------------

    def redeem_token(self, app: str) -> str:
        """The redeem token of the app called ``app``: the same one every time.

        A redeem token serves one app -- the coordination server pins it to the
        first manifest it is redeemed with and refuses another -- so each app
        that connects needs its own. ``app`` is only a name to remember the
        choice by; it need not be the app's identifier.

        Raises:
            NoRedeemTokenLeftError: If every token is taken. Create the hub with
                more ``redeem_tokens``.
        """
        # Read again: another process may have taken one since this hub was loaded.
        self._taken = {**_read_taken(self.directory), **self._taken}
        if app not in self._taken:
            free = [token for token in self.redeem_tokens if token not in self._taken.values()]
            if not free:
                raise NoRedeemTokenLeftError(
                    f"All {len(self.redeem_tokens)} redeem token(s) of this hub are taken "
                    f"(by {', '.join(sorted(self._taken))}), and one token serves one app. "
                    "Create the hub with more `redeem_tokens`."
                )
            self._taken[app] = free[0]
            _write_taken(self.directory, self._taken)
        return self._taken[app]

    def env(self, app: str) -> dict[str, str]:
        """What to put in an app's environment so it connects to this hub unattended.

        ``FAKTS_URL`` and ``FAKTS_REDEEM_TOKEN``, the latter :meth:`redeem_token`
        of ``app``.
        """
        return {"FAKTS_URL": self.fakts_url, "FAKTS_REDEEM_TOKEN": self.redeem_token(app)}

    # -- lifecycle ------------------------------------------------------------

    def up(self) -> None:
        """Start the hub (``konstruktor up``). Returns once the containers exist,
        which is before anything answers -- see :meth:`wait`."""
        self._konstruktor("up", os.fspath(self.directory))

    def wait(self, timeout: float = 600.0) -> tuple[Endpoint, ...]:
        """Block until every endpoint a client opens answers (``konstruktor wait``).

        Raises:
            KonstruktorError: If something still does not answer after ``timeout``
                seconds. :meth:`logs` says why.
        """
        result = self._konstruktor(
            "wait",
            os.fspath(self.directory),
            "--timeout",
            str(int(timeout)),
            "--json",
            # The command enforces the timeout itself; this is only a backstop.
            timeout=timeout + 60,
        )
        return tuple(Endpoint(**endpoint) for endpoint in json.loads(result.stdout))

    def stop(self) -> None:
        """Stop the containers, leaving them and their data in place."""
        self._konstruktor("stop", os.fspath(self.directory))

    def down(self, *, volumes: bool = False) -> None:
        """Remove the containers and networks, and with ``volumes`` the data too."""
        args = ["down", os.fspath(self.directory)]
        if volumes:
            args += ["--volumes", "--yes"]
        self._konstruktor(*args)

    def destroy(self) -> None:
        """Remove the hub completely: containers, data, folder and registry entry."""
        self._konstruktor("destroy", os.fspath(self.directory), "--yes")

    def job(self, service: str, job: str, *extra: str, timeout: float | None = 600.0) -> str:
        """Run one of a service's declared jobs, in a container of its own (``konstruktor job run``).

        ``extra`` is passed on to the job. Returns what it printed.

        Raises:
            KonstruktorError: If the service offers no such job, or the job failed.
        """
        args = ["job", "run", "--in", os.fspath(self.directory), service, job]
        if extra:
            args += ["--", *extra]
        result = self._konstruktor(*args, timeout=timeout)
        return result.stdout + result.stderr

    def superuser(self, service: str, username: str, password: str) -> None:
        """Create an account for one running service's admin site (``konstruktor superuser``).

        Each service keeps its own accounts. For a hub made for a test: the password is on
        the command line of the process that runs this.
        """
        self._konstruktor(
            "superuser", service, "--in", os.fspath(self.directory), "--username", username, "--password", password
        )

    def logs(self, service: str | None = None, *, tail: int = 200) -> str:
        """The last ``tail`` log lines, of one service or of all of them."""
        args = ["logs", os.fspath(self.directory), "--tail", str(tail)]
        if service is not None:
            args += ["--service", service]
        result = self._konstruktor(*args)
        return result.stdout + result.stderr


def _which_services(services: Sequence[str], service_images: Sequence[str]) -> list[str]:
    """The flags saying what a hub runs: by image when images are given, else by name."""
    if service_images:
        return [flag for image in service_images for flag in ("--service-image", image)]
    return ["--services", ",".join(services)]


def create_hub(
    directory: str | os.PathLike[str],
    *,
    services: Sequence[str] = ("rekuest",),
    service_images: Sequence[str] = (),
    http_port: int | None = None,
    identifier: str | None = None,
    organization: str = "demo",
    user: str = "demo",
    user_password: str | None = None,
    redeem_tokens: int = 1,
    images: Mapping[str, str] | None = None,
    debug: Sequence[str] = (),
    mounts: Mapping[str, str | os.PathLike[str]] | None = None,
    start: bool = True,
    wait: bool = True,
    timeout: float = 600.0,
    data_dir: str | os.PathLike[str] | None = None,
    binary: str | os.PathLike[str] | None = None,
) -> Hub:
    """Create a self-contained hub in ``directory``: services and their own
    coordination server, behind one gateway on one port.

    This is ``konstruktor hub create --server local``. Nobody has to accept the
    hub and nothing leaves the machine, so it runs unattended -- in a test suite,
    in CI, in a script.

    Args:
        directory: Where the hub lives. Created if it does not exist; must not
            already hold a deployment. Its name is the Docker Compose project,
            and those share one namespace on a machine -- so it has to differ
            from the folder name of every other deployment on it.
        services: The services to run, by name (``"rekuest"``, ``"mikro"``,
            ``"fluss"``, ...).
        service_images: The services to run, by image instead of by name:
            ``["jhnnsrs/mikro:7"]``. Each image is asked which service it is, so
            a client library only has to know the image that hosts it. Given,
            it replaces ``services``, and Rekuest runs only if one of the images
            is Rekuest's.
        http_port: The port the gateway is published on. Defaults to a free one.
        identifier: The hub's name inside its organization. Defaults to the
            folder's name.
        organization: The organization the hub's coordination server starts with.
        user: The account in it that apps act as.
        user_password: Its password. Generated when left out.
        redeem_tokens: How many redeem tokens to mint. One token serves one app,
            so this is how many apps can connect -- see :meth:`Hub.redeem_token`.
        images: Images to run instead of the ones a new hub gets, by compose
            service: ``{"rekuest": "jhnnsrs/rekuest:1.2.3"}``. For pinning what a
            suite runs against.
        debug: Services to run with their development server and Django's debug
            mode (``--debug``). Debug shows internals to anyone who can reach the
            service: for a hub on this machine.
        mounts: Source trees on this machine to run instead of what an image
            holds, by service: ``{"mikro": "~/Code/mikro-server"}``. The tree is
            mounted over the image's own code, so the image supplies the
            dependencies and the tree the service; with ``debug`` the server
            reloads when a file in it changes. For developing a service against
            a real hub.
        start: Start the hub once it is written.
        wait: After starting, block until everything a client opens answers.
        timeout: How long ``wait`` waits, in seconds.
        data_dir: The registry to record the hub in (``KONSTRUKTOR_DATA_DIR``).
            Defaults to the user's own, where the desktop app also sees it.
        binary: The ``konstruktor`` executable. See :func:`find_konstruktor_bin`.

    Returns:
        The hub, started and answering unless told otherwise.

    Raises:
        KonstruktorError: If the hub could not be created, started, or did not
            answer in time.
    """
    directory = Path(directory)
    executable = find_konstruktor_bin(binary)
    registry = Path(data_dir) if data_dir is not None else None

    args = [
        "hub",
        "create",
        os.fspath(directory),
        "--server",
        "local",
        *_which_services(services, service_images),
        "--http-port",
        str(http_port if http_port is not None else free_port()),
        "--org",
        organization,
        "--user",
        user,
        "--redeem-tokens",
        str(redeem_tokens),
        "--no-open",
        "--yes",
    ]
    for service, image in (images or {}).items():
        args += ["--image", f"{service}={image}"]
    for service in debug:
        args += ["--debug", service]
    for service, tree in (mounts or {}).items():
        # A checkout the hub is asked to run from source is left exactly as it is when
        # it is already there: so the tree is put where the checkout would go, as a
        # link, and nothing is cloned.
        source = Path(tree).expanduser().resolve()
        if not source.is_dir():
            raise FileNotFoundError(f"{source} is not a directory: nothing to mount as {service}")
        checkout = directory / "mounts" / service
        checkout.parent.mkdir(parents=True, exist_ok=True)
        if not checkout.exists():
            checkout.symlink_to(source, target_is_directory=True)
        args += ["--from-source", service]
    if identifier is not None:
        args += ["--identifier", identifier]
    if user_password is not None:
        args += ["--user-password", user_password]
    if not start:
        args.append("--no-start")
    _run(executable, args, data_dir=registry)

    hub = Hub.load(directory, data_dir=registry, binary=executable)
    if start and wait:
        hub.wait(timeout)
    return hub
