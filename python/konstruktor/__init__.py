"""Create and manage Arkitekt deployments from Python.

A thin layer over the ``konstruktor`` executable, which this package ships::

    from konstruktor import create_hub

    hub = create_hub("./hub", services=["rekuest", "mikro"], redeem_tokens=2)
    hub.fakts_url                  # where an app is pointed
    hub.redeem_token("my-app")     # what it trades for a client, unattended
    hub.destroy()

With pytest, the ``konstruktor_hub`` fixture does the same and cleans up after the
session -- see :mod:`konstruktor.pytest_plugin`.
"""

from importlib.metadata import PackageNotFoundError, version

from konstruktor._binary import KonstruktorNotFoundError, find_konstruktor_bin
from konstruktor.hub import (
    ACCESS_FILE,
    Account,
    Endpoint,
    Hub,
    KonstruktorError,
    NoRedeemTokenLeftError,
    Service,
    create_hub,
    free_port,
)

try:
    __version__ = version("konstruktor")
except PackageNotFoundError:  # a source checkout that was never installed
    __version__ = "0+unknown"

__all__ = [
    "ACCESS_FILE",
    "Account",
    "Endpoint",
    "Hub",
    "KonstruktorError",
    "KonstruktorNotFoundError",
    "NoRedeemTokenLeftError",
    "Service",
    "create_hub",
    "find_konstruktor_bin",
    "free_port",
]
