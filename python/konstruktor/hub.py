"""A self-contained Arkitekt hub, from Python.

A :class:`Hub` is a dokker ``Deployment`` whose stack ``konstruktor`` writes and
runs. Nothing about what a deployment looks like is decided on this side: which
services exist, how they are wired, what the coordination server is seeded with --
that is the generator's, and the generator is the binary. What this side adds is
the state around it: a hub that exists for the length of a ``with`` block, and a
way to look at its services while it does.
"""

import json
import os
from collections.abc import Mapping, Sequence
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Self

from dokker import CLI, Deployment, HealthCheck, PolicyName
from koil import unkoil
from pydantic import PrivateAttr

from konstruktor._binary import find_konstruktor_bin
from konstruktor.project import ACCESS_FILE, KonstruktorError, KonstruktorProject
from konstruktor.runs import hub_dir, registry_dir, runs_dir

#: Where this package writes down which app was handed which redeem token. Beside the
#: access document, because it is as secret, and in the hub's folder, because it is the
#: hub's: a token is pinned to its app by the coordination server, for as long as the hub
#: lives and whichever process asks.
TAKEN_FILE = "secrets/redeem-tokens-taken.json"

#: How long a test hub's teardown may take before leaving the block gives up on it.
#: A hub is a dozen containers and their volumes, not the one or two of a compose file
#: written for a test.
TEARDOWN_TIMEOUT = 300.0


class NoRedeemTokenLeftError(LookupError):
    """Every redeem token of the hub is already taken by another app."""


class HubNotCreatedError(RuntimeError):
    """The hub's folder is not written yet, so nothing is known about how to reach it."""


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


@dataclass(frozen=True)
class Access:
    """How to get into a hub: what its ``secrets/access.json`` says."""

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
    #: Every redeem token there is. Prefer :meth:`Hub.redeem_token`, which hands
    #: each app its own.
    redeem_tokens: tuple[str, ...]
    services: Mapping[str, Service]

    @classmethod
    def read(cls, directory: Path) -> "Access":
        """The access document of the hub in ``directory``."""
        access: dict[str, Any] = json.loads((directory / ACCESS_FILE).read_text())
        return cls(
            identifier=access["hub"],
            gateway_url=access["gateway_url"],
            fakts_url=access["fakts_url"],
            issuer=access["issuer"],
            organization=access["organization"],
            admin=Account(**access["admin"]),
            users=tuple(Account(**user) for user in access["users"]),
            redeem_tokens=tuple(access["redeem_tokens"]),
            services={name: Service(name=name, **service) for name, service in access["services"].items()},
        )


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


class Hub(Deployment):
    """A self-contained hub on this machine, for as long as a ``with`` block.

    Made by :func:`testing_hub` or :func:`local_hub`, or by :meth:`Hub.load` for
    one that already exists. Entering does nothing; the block says what happens::

        with testing_hub(services=["mikro", "rekuest"]) as hub:
            hub.up()            # written, then started
            hub.wait_ready()    # until every endpoint a client opens answers
            hub.fakts_url, hub.redeem_token("my-app")
            hub.ps(); hub.logs("mikro"); hub.exec("mikro", "python manage.py check")

    Starting, stopping and removing are ``konstruktor``'s commands of the same
    name. Everything that looks at a running hub -- ``ps``, ``logs``, ``exec``,
    ``run``, ``restart``, ``get_port``, ``create_watcher``, ``check_health`` -- is
    dokker's, on the compose project in the hub's folder.

    The methods run on a loop the block owns, so they need the block (or
    :meth:`enter`). How to get in -- :attr:`fakts_url`, :meth:`redeem_token`,
    :attr:`services` and the rest of the access document -- is read from the
    folder and works anywhere.
    """

    project: KonstruktorProject

    _access: Access | None = PrivateAttr(default=None)
    _taken: dict[str, str] = PrivateAttr(default_factory=dict)

    @classmethod
    def load(
        cls,
        directory: str | os.PathLike[str],
        *,
        data_dir: str | os.PathLike[str] | None = None,
        binary: str | os.PathLike[str] | None = None,
        policy: PolicyName = "manual",
    ) -> Self:
        """The hub in ``directory``, as it is. Leaving the block leaves it alone.

        Raises:
            FileNotFoundError: If the folder holds no self-contained hub.
        """
        directory = Path(directory)
        if not (directory / ACCESS_FILE).is_file():
            raise FileNotFoundError(
                f"{directory} holds no self-contained hub: there is no {ACCESS_FILE}. "
                "Only a hub created with `--server local` writes one."
            )
        return cls(
            project=KonstruktorProject(
                directory=directory,
                data_dir=Path(data_dir) if data_dir is not None else None,
                binary=find_konstruktor_bin(binary),
            ),
            policy=policy,
            # A hub somebody keeps: `down()` leaves its data unless told otherwise.
            remove_volumes_on_down=False,
        )

    # -- getting in -----------------------------------------------------------

    @property
    def directory(self) -> Path:
        """The hub's folder."""
        return self.project.directory

    @property
    def access(self) -> Access:
        """How to get into the hub, as its folder says.

        Raises:
            HubNotCreatedError: If the folder is not written yet.
        """
        if self._access is None:
            if not self.project.exists:
                raise HubNotCreatedError(
                    f"The hub in {self.directory} is not written yet, so it has no address and "
                    "no tokens. Call `hub.create()` (or `hub.up()`, which creates it first) "
                    "inside its `with` block."
                )
            self._access = Access.read(self.directory)
        return self._access

    @property
    def identifier(self) -> str:
        """The hub's name inside its organization."""
        return self.access.identifier

    @property
    def gateway_url(self) -> str:
        """Where the gateway answers, e.g. ``http://localhost:7190``."""
        return self.access.gateway_url

    @property
    def fakts_url(self) -> str:
        """What an app is pointed at (``FAKTS_URL``)."""
        return self.access.fakts_url

    @property
    def issuer(self) -> str:
        """The ``iss`` of every token the hub's coordination server signs."""
        return self.access.issuer

    @property
    def organization(self) -> str:
        """The organization the hub's coordination server started with."""
        return self.access.organization

    @property
    def admin(self) -> Account:
        """The Django administrator of every service."""
        return self.access.admin

    @property
    def users(self) -> tuple[Account, ...]:
        """The accounts apps act as. The first one owns the organization."""
        return self.access.users

    @property
    def redeem_tokens(self) -> tuple[str, ...]:
        """Every redeem token there is. Prefer :meth:`redeem_token`."""
        return self.access.redeem_tokens

    @property
    def services(self) -> Mapping[str, Service]:
        """The hub's services as they are reached through the gateway, by name."""
        return self.access.services

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

    async def ainitialize(self) -> CLI:
        """Write the hub's folder if it is not there, and learn its services.

        Each service the hub says it has gets a health check on the address it
        gives for one, so ``check_health(services=[...])`` and
        ``restart(..., await_health=True)`` know what healthy means here.
        """
        cli = await super().ainitialize()
        checked = {check.service for check in self.health_checks}
        for name, service in self.services.items():
            if name not in checked:
                self.health_checks.append(HealthCheck(url=service.health_url, service=name))
        return cli

    async def acreate(self) -> None:
        """Write the hub's folder without starting anything. A no-op once it is there."""
        await self.aretrieve_cli()

    def create(self) -> None:
        """Write the hub's folder without starting anything. See :meth:`acreate`."""
        unkoil(self.acreate)

    async def await_ready(self, timeout: float = 600.0) -> tuple[Endpoint, ...]:
        """Block until every endpoint a client opens answers (``konstruktor wait``).

        Raises:
            KonstruktorError: If something still does not answer after ``timeout``
                seconds. :meth:`logs` says why.
        """
        await self.aretrieve_cli()
        said = await self.project.arun("wait", os.fspath(self.directory), "--timeout", str(int(timeout)), "--json")
        return tuple(Endpoint(**endpoint) for endpoint in json.loads(said))

    def wait_ready(self, timeout: float = 600.0) -> tuple[Endpoint, ...]:
        """Block until every endpoint a client opens answers. See :meth:`await_ready`."""
        return unkoil(self.await_ready, timeout)

    async def adestroy(self) -> None:
        """Remove the hub completely: containers, data, folder and registry entry.

        What leaving the block does to a hub made by :func:`testing_hub`.
        """
        await self.project.adestroy()
        self._access = None

    def destroy(self) -> None:
        """Remove the hub completely. See :meth:`adestroy`."""
        unkoil(self.adestroy)

    # -- what only a hub has --------------------------------------------------

    async def ajob(self, service: str, job: str, *extra: str) -> str:
        """Run one of a service's declared jobs, in a container of its own (``konstruktor job run``).

        ``extra`` is passed on to the job. Returns what it printed. For a command
        the image does not declare as a job, use :meth:`run` or :meth:`exec`.

        Raises:
            KonstruktorError: If the service offers no such job, or the job failed.
        """
        await self.aretrieve_cli()
        args = ["job", "run", "--in", os.fspath(self.directory), service, job]
        if extra:
            args += ["--", *extra]
        return "\n".join([line async for _, line in self.project.astream(*args)])

    def job(self, service: str, job: str, *extra: str) -> str:
        """Run one of a service's declared jobs. See :meth:`ajob`."""
        return unkoil(self.ajob, service, job, *extra)

    async def asuperuser(self, service: str, username: str, password: str) -> None:
        """Create an account for one running service's admin site (``konstruktor superuser``).

        Each service keeps its own accounts. For a hub made for a test: the password is on
        the command line of the process that runs this.
        """
        await self.aretrieve_cli()
        await self.project.arun(
            "superuser", service, "--in", os.fspath(self.directory), "--username", username, "--password", password
        )

    def superuser(self, service: str, username: str, password: str) -> None:
        """Create an account for one running service's admin site. See :meth:`asuperuser`."""
        unkoil(self.asuperuser, service, username, password)


def _project(
    directory: Path,
    *,
    services: Sequence[str],
    service_images: Sequence[str],
    mounts: Mapping[str, str | os.PathLike[str]] | None,
    images: Mapping[str, str] | None,
    debug: Sequence[str],
    data_dir: str | os.PathLike[str] | None,
    binary: str | os.PathLike[str] | None,
    reap_runs: Path | None = None,
    **what: Any,
) -> KonstruktorProject:
    return KonstruktorProject(
        directory=directory,
        services=tuple(services),
        service_images=tuple(service_images),
        images=dict(images or {}),
        debug=tuple(debug),
        mounts={service: Path(tree) for service, tree in (mounts or {}).items()},
        data_dir=Path(data_dir) if data_dir is not None else None,
        binary=find_konstruktor_bin(binary),
        reap_runs=reap_runs,
        **what,
    )


def testing_hub(
    *,
    services: Sequence[str] = ("rekuest",),
    service_images: Sequence[str] = (),
    name: str = "hub",
    http_port: int | None = None,
    organization: str = "demo",
    user: str = "demo",
    user_password: str | None = None,
    redeem_tokens: int = 1,
    images: Mapping[str, str] | None = None,
    debug: Sequence[str] = (),
    mounts: Mapping[str, str | os.PathLike[str]] | None = None,
    binary: str | os.PathLike[str] | None = None,
    policy: PolicyName = "testing",
    teardown_timeout: float | None = TEARDOWN_TIMEOUT,
) -> Hub:
    """A hub for a test: gone, with its data and its folder, when the block is left.

    ::

        with testing_hub(service_images=["jhnnsrs/mikro:7"], redeem_tokens=2) as hub:
            hub.up()
            hub.wait_ready()
            ...

    It lives in a folder of this process's own under the temp directory, recorded
    in a registry of its own: it never shows up in the desktop app, and it cannot
    share a compose project with a hub somebody keeps. Should the process be
    killed before the block is left, the next test hub made on this machine
    destroys what it left behind.

    Args:
        services: The services to run, by name (``"rekuest"``, ``"mikro"``, ...).
        service_images: The services to run, by image instead of by name. Given,
            it replaces ``services``.
        name: The hub's name inside its organization.
        http_port: The port the gateway is published on. Defaults to a free one.
        organization: The organization the coordination server starts with.
        user: The account in it that apps act as.
        user_password: Its password. Generated when left out.
        redeem_tokens: How many apps can connect -- see :meth:`Hub.redeem_token`.
        images: Images to run instead of the ones a new hub gets, by service.
        debug: Services to run with their development server.
        mounts: Source trees to run instead of what an image holds, by service.
        binary: The ``konstruktor`` executable.
        policy: What leaving the block does. ``"testing"`` destroys the hub;
            ``"manual"`` leaves it running, to look at.
        teardown_timeout: Seconds the teardown may take.
    """
    return Hub(
        project=_project(
            hub_dir(name),
            services=services,
            service_images=service_images,
            identifier=name,
            http_port=http_port,
            organization=organization,
            user=user,
            user_password=user_password,
            redeem_tokens=redeem_tokens,
            images=images,
            debug=debug,
            mounts=mounts,
            data_dir=registry_dir(),
            binary=binary,
            reap_runs=runs_dir(),
        ),
        policy=policy,
        teardown_timeout=teardown_timeout,
    )


testing_hub.__test__ = False  # type: ignore[attr-defined]  # not a test, whatever pytest makes of the name


def local_hub(
    directory: str | os.PathLike[str],
    *,
    services: Sequence[str] = ("rekuest",),
    service_images: Sequence[str] = (),
    identifier: str | None = None,
    http_port: int | None = None,
    organization: str = "demo",
    user: str = "demo",
    user_password: str | None = None,
    redeem_tokens: int = 1,
    images: Mapping[str, str] | None = None,
    debug: Sequence[str] = (),
    mounts: Mapping[str, str | os.PathLike[str]] | None = None,
    data_dir: str | os.PathLike[str] | None = None,
    binary: str | os.PathLike[str] | None = None,
    policy: PolicyName = "local",
) -> Hub:
    """A hub in a folder of your choosing, that is kept: stopped, not removed, on leaving the block.

    Written the first time it is used and used as it is from then on -- what it
    is made of is only read when the folder does not hold a hub yet. For
    developing against a real hub::

        with local_hub("./hub", services=["mikro"], mounts={"mikro": "~/Code/mikro"}, debug=["mikro"]) as hub:
            hub.up()
            hub.wait_ready()

    Args:
        directory: Where the hub lives. Its name is the Docker Compose project,
            and those share one namespace on a machine -- so it has to differ from
            the folder name of every other deployment on it.
        identifier: The hub's name inside its organization. Defaults to the
            folder's name.
        data_dir: The registry to record the hub in. Defaults to the user's own,
            where the desktop app also sees it.

    The other arguments are :func:`testing_hub`'s.
    """
    return Hub(
        project=_project(
            Path(directory),
            services=services,
            service_images=service_images,
            identifier=identifier,
            http_port=http_port,
            organization=organization,
            user=user,
            user_password=user_password,
            redeem_tokens=redeem_tokens,
            images=images,
            debug=debug,
            mounts=mounts,
            data_dir=data_dir,
            binary=binary,
        ),
        policy=policy,
        # Kept means its data too: `down()` leaves the volumes unless told otherwise.
        remove_volumes_on_down=False,
    )


__all__ = [
    "ACCESS_FILE",
    "Access",
    "Account",
    "Endpoint",
    "Hub",
    "HubNotCreatedError",
    "KonstruktorError",
    "NoRedeemTokenLeftError",
    "Service",
    "local_hub",
    "testing_hub",
]
