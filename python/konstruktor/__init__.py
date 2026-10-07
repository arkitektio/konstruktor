"""Create and manage Arkitekt deployments from Python.

A hub is a dokker ``Deployment`` that the ``konstruktor`` executable, which this
package ships, writes and runs::

    from konstruktor import testing_hub

    with testing_hub(services=["rekuest", "mikro"], redeem_tokens=2) as hub:
        hub.up()
        hub.wait_ready()
        hub.fakts_url                  # where an app is pointed
        hub.redeem_token("my-app")     # what it trades for a client, unattended
        hub.ps(); hub.logs("mikro")    # and what its services are doing
    # gone, with its data and its folder

``local_hub("./hub", ...)`` is one that is kept, and ``Hub.load("./hub")`` one that
already exists. With pytest, the ``konstruktor_hub`` fixture does the same for a
session -- see :mod:`konstruktor.pytest_plugin`.
"""

from importlib.metadata import PackageNotFoundError, version

from konstruktor._binary import KonstruktorNotFoundError, find_konstruktor_bin
from konstruktor.hub import (
    ACCESS_FILE,
    Access,
    Account,
    Endpoint,
    Hub,
    HubNotCreatedError,
    KonstruktorError,
    NoRedeemTokenLeftError,
    Service,
    local_hub,
    testing_hub,
)
from konstruktor.project import KonstruktorProject, free_port

try:
    __version__ = version("konstruktor")
except PackageNotFoundError:  # a source checkout that was never installed
    __version__ = "0+unknown"

__all__ = [
    "ACCESS_FILE",
    "Access",
    "Account",
    "Endpoint",
    "Hub",
    "HubNotCreatedError",
    "KonstruktorError",
    "KonstruktorNotFoundError",
    "KonstruktorProject",
    "NoRedeemTokenLeftError",
    "Service",
    "find_konstruktor_bin",
    "free_port",
    "local_hub",
    "testing_hub",
]
