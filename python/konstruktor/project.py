"""A hub as a dokker project: generated, started and removed by ``konstruktor``.

dokker runs a compose project it is pointed at. A hub is one, but not one that
``docker compose up`` can start: before its containers exist, ``konstruktor up``
writes down the builds it runs, has each service's image write its own config and
prepares the databases. So this project hands dokker the generated compose file
for everything that looks at a running stack -- logs, exec, ps, ports -- and keeps
the verbs that change it for the binary.
"""

import os
import socket
from collections.abc import AsyncIterator, Sequence
from pathlib import Path

from dokker import CLI, CommandError, DownOptions, ProjectError, PullOptions, UpOptions
from dokker.command import astream_command
from pydantic import BaseModel, Field

from konstruktor._binary import find_konstruktor_bin
from konstruktor.runs import areap_dead_runs

#: Where ``hub create --server local`` writes how to reach the hub.
ACCESS_FILE = "secrets/access.json"
#: The compose file a hub's folder holds.
COMPOSE_FILE = "docker-compose.yaml"

LogLine = tuple[str, str]


class KonstruktorError(CommandError):
    """A ``konstruktor`` command failed.

    The message carries what the command wrote, which is where it says what went
    wrong and what to do about it; ``stderr`` and ``stdout`` hold it line by line.
    """


def free_port() -> int:
    """A TCP port nothing is listening on right now.

    A hub's address is part of every token it issues, so the port is decided
    before the hub exists rather than assigned by Docker afterwards.
    """
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as probe:
        probe.bind(("127.0.0.1", 0))
        return int(probe.getsockname()[1])


class KonstruktorProject(BaseModel):
    """A self-contained hub in a folder, written by ``konstruktor hub create --server local``.

    The fields say what the hub is made of. They are read once, when the folder is
    written; a folder that already holds a hub is used as it is.
    """

    #: Where the hub lives. Created if it does not exist. Its name is the Docker
    #: Compose project, and those share one namespace on a machine -- so it has to
    #: differ from the folder name of every other deployment on it.
    directory: Path
    #: The services to run, by name (``"rekuest"``, ``"mikro"``, ``"fluss"``, ...).
    services: tuple[str, ...] = ("rekuest",)
    #: The services to run, by image instead of by name: ``["jhnnsrs/mikro:7"]``.
    #: Each image is asked which service it is, so a client library only has to
    #: know the image that hosts it. Given, it replaces ``services``, and Rekuest
    #: runs only if one of the images is Rekuest's.
    service_images: tuple[str, ...] = ()
    #: The port the gateway is published on. Defaults to a free one.
    http_port: int | None = None
    #: The hub's name inside its organization. Defaults to the folder's name.
    identifier: str | None = None
    #: The organization the hub's coordination server starts with.
    organization: str = "demo"
    #: The account in it that apps act as.
    user: str = "demo"
    #: Its password. Generated when left out.
    user_password: str | None = None
    #: How many redeem tokens to mint. One token serves one app, so this is how
    #: many apps can connect.
    redeem_tokens: int = 1
    #: Images to run instead of the ones a new hub gets, by compose service:
    #: ``{"rekuest": "jhnnsrs/rekuest:1.2.3"}``.
    images: dict[str, str] = Field(default_factory=dict)
    #: Services to run with their development server and Django's debug mode.
    #: Debug shows internals to anyone who can reach the service.
    debug: tuple[str, ...] = ()
    #: Source trees on this machine to run instead of what an image holds, by
    #: service: ``{"mikro": "~/Code/mikro-server"}``. The tree is mounted over the
    #: image's own code, so the image supplies the dependencies and the tree the
    #: service; with ``debug`` the server reloads when a file in it changes.
    mounts: dict[str, Path] = Field(default_factory=dict)
    #: The registry the hub is recorded in (``KONSTRUKTOR_DATA_DIR``). Defaults to
    #: the user's own, where the desktop app also sees it.
    data_dir: Path | None = None
    #: The ``konstruktor`` executable. See :func:`konstruktor.find_konstruktor_bin`.
    binary: Path = Field(default_factory=find_konstruktor_bin)
    #: A folder of run folders whose dead owners' hubs are destroyed before this
    #: hub is written. See :mod:`konstruktor.runs`.
    reap_runs: Path | None = None

    @property
    def exists(self) -> bool:
        """Whether the folder already holds a self-contained hub."""
        return (self.directory / ACCESS_FILE).is_file()

    async def astream(self, *args: str) -> AsyncIterator[LogLine]:
        """Run one ``konstruktor`` command, yielding ``(stream, line)`` as it writes.

        Raises:
            KonstruktorError: If it exits with anything but zero.
        """
        command = [os.fspath(self.binary), *args]
        env = {"KONSTRUKTOR_DATA_DIR": os.fspath(self.data_dir)} if self.data_dir is not None else None
        try:
            async for line in astream_command(command, env=env):
                yield line
        except CommandError as error:
            raise KonstruktorError(
                str(error),
                command=error.command,
                returncode=error.returncode,
                stdout=error.stdout,
                stderr=error.stderr,
            ) from error

    async def arun(self, *args: str) -> str:
        """Run one ``konstruktor`` command to completion. Returns what it printed to stdout."""
        return "\n".join([line async for stream, line in self.astream(*args) if stream == "STDOUT"])

    def _target(self) -> str:
        return os.fspath(self.directory)

    def _create_args(self) -> list[str]:
        which = (
            [flag for image in self.service_images for flag in ("--service-image", image)]
            if self.service_images
            else ["--services", ",".join(self.services)]
        )
        args = [
            "hub",
            "create",
            self._target(),
            "--server",
            "local",
            *which,
            "--http-port",
            str(self.http_port if self.http_port is not None else free_port()),
            "--org",
            self.organization,
            "--user",
            self.user,
            "--redeem-tokens",
            str(self.redeem_tokens),
            "--no-open",
            "--yes",
            # Starting is `up`'s: the folder is written here and nothing runs yet.
            "--no-start",
        ]
        for service, image in self.images.items():
            args += ["--image", f"{service}={image}"]
        for service in self.debug:
            args += ["--debug", service]
        for service in self.mounts:
            args += ["--from-source", service]
        if self.identifier is not None:
            args += ["--identifier", self.identifier]
        if self.user_password is not None:
            args += ["--user-password", self.user_password]
        return args

    def _link_mounts(self) -> None:
        for service, tree in self.mounts.items():
            # A checkout the hub is asked to run from source is left exactly as it is when
            # it is already there: so the tree is put where the checkout would go, as a
            # link, and nothing is cloned.
            source = Path(tree).expanduser().resolve()
            if not source.is_dir():
                raise FileNotFoundError(f"{source} is not a directory: nothing to mount as {service}")
            checkout = self.directory / "mounts" / service
            checkout.parent.mkdir(parents=True, exist_ok=True)
            if not checkout.exists():
                checkout.symlink_to(source, target_is_directory=True)

    # -- dokker's Project -----------------------------------------------------

    async def ainititialize(self) -> CLI:
        """Write the hub's folder, unless it is there already. Nothing is started."""
        if not self.exists:
            if self.reap_runs is not None:
                await areap_dead_runs(self.reap_runs, self.binary)
            self._link_mounts()
            await self.arun(*self._create_args())
        # No project name: compose derives it from the folder, which is what every
        # `konstruktor` command on this folder does too. Named differently here, the
        # two would be looking at two different stacks.
        return CLI(
            compose_files=[self.directory / COMPOSE_FILE],
            compose_project_directory=self.directory,
        )

    async def atear_down(self, cli: CLI) -> None:
        """Remove the hub completely: containers, data, folder and registry entry."""
        await self.adestroy()

    async def adestroy(self) -> None:
        """``konstruktor destroy``, without asking a coordination server to forget the hub.

        The hub's coordination server is its own and goes with it -- and has usually
        been taken down a moment ago, by the ``down`` that runs before this.
        """
        await self.arun("destroy", self._target(), "--yes", "--local-only")

    async def abefore_pull(self) -> None:
        """Nothing to prepare."""

    async def abefore_up(self) -> None:
        """Nothing to prepare."""

    async def abefore_enter(self) -> None:
        """Nothing to prepare."""

    async def abefore_down(self) -> None:
        """Nothing to prepare."""

    async def abefore_stop(self) -> None:
        """Nothing to prepare."""

    # -- the verbs konstruktor keeps ------------------------------------------

    async def astream_up(self, cli: CLI, options: UpOptions) -> AsyncIterator[LogLine]:
        """``konstruktor up``: the whole hub, always detached.

        ``wait`` is honoured by ``konstruktor wait``, which knows what a client of
        this hub opens. A part of a hub cannot be started and its images are the
        generator's to choose, so every other option is refused.
        """
        _refuse(
            "up",
            {
                "detach=False": not options.detach,
                "services": options.services,
                "build": options.build,
                "force_recreate": options.force_recreate,
                "no_recreate": options.no_recreate,
                "no_build": options.no_build,
                "remove_orphans": options.remove_orphans,
                "renew_anon_volumes": options.renew_anon_volumes,
                "pull": options.pull,
                "scales": options.scales,
            },
        )
        async for line in self.astream("up", self._target()):
            yield line
        if options.wait:
            timeout = options.wait_timeout if options.wait_timeout is not None else 600
            async for line in self.astream("wait", self._target(), "--timeout", str(timeout)):
                yield line

    async def astream_stop(self, cli: CLI, timeout: int | None) -> AsyncIterator[LogLine]:
        """``konstruktor stop``. The grace period is the generator's."""
        async for line in self.astream("stop", self._target()):
            yield line

    async def astream_down(self, cli: CLI, options: DownOptions) -> AsyncIterator[LogLine]:
        """``konstruktor down``, with the data when ``volumes`` is set.

        The other options arrive filled in from the deployment's settings whether
        or not anybody asked for them, and have no meaning for a hub: they are
        left alone.
        """
        args = ["down", self._target()]
        if options.volumes:
            args += ["--volumes", "--yes"]
        async for line in self.astream(*args):
            yield line

    async def astream_pull(self, cli: CLI, options: PullOptions) -> AsyncIterator[LogLine]:
        """``konstruktor pull``: newer images for the whole hub."""
        _refuse(
            "pull",
            {
                "services": options.services,
                "ignore_pull_failures": options.ignore_pull_failures,
                "include_deps": options.include_deps,
            },
        )
        async for line in self.astream("pull", self._target()):
            yield line


def _refuse(verb: str, asked: dict[str, object]) -> None:
    """Refuse the options of ``verb`` that a hub has no way to honour."""
    given: Sequence[str] = [name for name, value in asked.items() if value]
    if given:
        raise ProjectError(
            f"A hub is started and stopped as a whole by konstruktor, so `{verb}` cannot take "
            f"{', '.join(given)}. Use the hub's compose commands (`exec`, `run`, `restart`) "
            "to act on one service."
        )
