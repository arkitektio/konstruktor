# Konstruktor

This is the repository for the Konstruktor project, which primarily serves as an entrypoint and installer for
the [Arkitekt](http://arkitekt.live) platform.

![Screenshots of the Konstruktor Platform](demo.gif)


# About Docker!

Konstruktor writes the deployment itself and hands it to Docker Compose to run, so Docker has to be
installed on the host machine — but nothing else does. There is no Python, no `arkitekt-next` CLI and
no helper container involved. The app checks for the presence of Docker and Docker Compose, but will
**not** install them.

The check runs as the app starts and again as the first step of the wizard, and it answers three
questions separately, because each "no" has its own remedy: the `docker` binary is missing (install
Docker), the CLI is there without its compose plugin (install a current Docker), or the daemon is not
answering (start Docker). The wizard will not go past that step until all three are satisfied.

 Please refer to the [Docker documentation](https://docs.docker.com/get-docker/) for
instructions on how to install Docker on your machine.


## Installation

### The command line

```
curl -fsSL https://raw.githubusercontent.com/arkitektio/konstruktor/master/install.sh | sh
```

Detects your platform, downloads the matching binary, verifies it against the release's
published `SHA256SUMS`, installs it to `~/.local/bin`, and — when there is a terminal
attached — asks for a hub identifier and creates that hub in `~/MyHubs/<identifier>`.
Pass `--hub-dir <path>` to put it somewhere else, or `--no-run` to just install it.

`~/MyHubs` is only a default. A hub folder holds the database and the object store, so it
can live wherever you want it — `hub create` takes a directory, the way `git init` does,
and defaults to the one you are standing in. Wherever hubs land, konstruktor keeps its own
index of them, so `konstruktor list` finds them all and every command takes a name:

```
konstruktor hub create          # the wizard, here
konstruktor hub create /mnt/data/lab-hub
konstruktor list                # what this machine knows about
konstruktor status [target]     # what a hub is, and what is running
konstruktor up|stop|down|pull|ps|logs [target]
konstruktor doctor              # is Docker ready?
```

Every answer has a flag, so a hub can be created unattended:

```
konstruktor hub create ~/MyHubs/lab-hub --server go.arkitekt.live \
  --identifier lab-hub --services rekuest,mikro,fluss --yes
```

With a terminal, missing answers are prompted for; without one they are an error naming
the flag that would have supplied it, so CI never hangs on a prompt. `[target]` is a path
or the name of a registered deployment — the CLI and the desktop app share one registry,
so a hub created in either shows up in the other.

The one interactive step is the authorization itself: the CLI prints the URL and the short
code, and waits while somebody with an account accepts the hub in a browser.

### The desktop app



Konstruktor is an executable app that can be installed with the installer found in the releases section of this
repository. The installer is available for Windows, Mac and Linux. The installation should be prettry straigthforward.

## Usage

Konstruktor creates and manages **hubs**: the data and compute services (rekuest, mikro, fluss, …)
that make up an Arkitekt deployment. A hub manages no accounts of its own — users, organizations and
permissions live on a *coordination server* such as [go.arkitekt.live](https://go.arkitekt.live), and
the hub is authorized against one before it exists on disk.

Creating a hub walks through a folder, the coordination server and the hub's name there, which
services to run, which ports to publish, which of this machine's addresses to advertise, and whether
the hub joins a mesh. The last step is the authorization itself: Konstruktor sends the hub's manifest
to the coordination server, shows you a short code, and opens the page where somebody with an account
accepts the hub into an organization. Only once that comes back does anything get written.

The wizard opens on answers that already work — Docker is checked before the first question, the
folder defaults to `MyHub` in your home directory, the hub identifier is the folder's name slugified,
and `go.arkitekt.live` is offered as the coordination server alongside any you have used before.
Everything that has a working default sits under an "Advanced" disclosure on its step, so each step
asks the one question it is actually there for.

### The mesh

A hub advertised only at LAN addresses is only reachable from that LAN. The mesh step joins it to the
organization's tailnet: a `tailscale/tailscale` sidecar runs alongside the gateway, and the gateway is
published inside that container's network namespace, so the hub is on the tailnet under a name of its
own.

Joining and *being advertised* are two steps, not one. The manifest sent at authorization time carries
the addresses picked on the Addresses step, and the tailnet address does not exist until the hub has
actually joined — so once the stack is up, add that address on the dashboard and authorize again. Only
then do clients off this network get told where the hub lives.

The credential is a single-use pre-authorized key, and it can come from either end. "Join the
organization's mesh" sets `request_auth_key` on the hub manifest, so the coordination server mints one
while it is accepting the hub and returns it in the grant envelope — no second trip. Whoever approves
the hub decides whether to grant it, so an approval can come back without a key. Alternatively a key
from a tailnet you run yourself can be pasted, together with the control server it belongs to.

A hub without a mesh generates exactly what it generated before the mesh existed: no `mesh` block in
`hub_config.yaml`, no sidecar, no extra volume.

What lands in the folder is an ordinary Docker Compose project — `hub_config.yaml`,
`docker-compose.yaml`, a `configs/` directory and `hub_credentials.json` — which you can start, stop
and inspect from the app, or drive with `docker compose` yourself. Nothing about a deployment is
locked to Konstruktor.

A hub can be authorized again later from its dashboard, which is how you add services, move it to a
different network, or point it at another coordination server.

### How it works

Konstruktor generates the whole deployment in-process. The profile, the `docker-compose.yaml`,
the Caddyfile and every service's configuration file are produced by `konstruktor-core`, a Rust
port of [`arkitekt-next`](https://github.com/arkitektio/arkitekt-next)'s own generator, checked
against that generator's real output by golden-file tests
(`crates/konstruktor-core/tests/generate.rs`). The only thing Docker is asked to do is run the
result.

The core is the whole product; the desktop app and the `konstruktor` command are two front ends
over it. Creating a hub — build the profile, authorize it, write the folder, start the stack — is
one function in the core that takes a progress callback, so the two front ends run the same code
rather than merely equivalent code. The desktop app renders those progress events through a Tauri
channel; the CLI prints them.

```
crates/konstruktor-core/   generation · authorization · docker · the registry
crates/konstruktor-cli/    the `konstruktor` binary
src-tauri/                 Tauri commands, and nothing else
src/                       React
```

Authorization is the canonical fakts device-code flow: the hub manifest is POSTed to the coordination
server's `hub_authorization_endpoint` (discovered from `/.well-known/fakts`), a human accepts it in
the browser, and Konstruktor polls the OAuth2 token endpoint until the grant comes back carrying the
hub's identity — the JWKS URL the generated services verify inbound tokens against.

## Disclaimer

Konstruktor is a work in progress and is not yet ready for production use. It is provided as-is, without any warranty.
While we do our best to ensure that Konstruktor is usable for non-technical users, we cannot guarantee that it will
work on all systems. If you encounter any issues, please report them in the issues section of this repository. We would
really appreciate it if you could provide as much information as possible about your system and the issue you are
encountering.

## License

Konstruktor is licensed under the MIT license. Please refer to the LICENSE file for more information.


## Additional information

Konstruktor deploys Arkitekt through the `arkitekt-next` CLI, so the deployments it produces are exactly
those the CLI produces — bug reports about a generated stack belong upstream, while anything about the
wizards, the dashboard or the CLI invocation belongs here.

### Development

```bash
pnpm install
pnpm tauri dev        # run the app
pnpm test             # unit tests, no Docker needed
KONSTRUKTOR_E2E=1 pnpm test   # additionally runs the real CLI in Docker
```
