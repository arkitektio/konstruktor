"""Where the ``konstruktor`` executable is."""

import os
import shutil
import sys
import sysconfig
from pathlib import Path

_NAME = "konstruktor.exe" if sys.platform == "win32" else "konstruktor"


class KonstruktorNotFoundError(FileNotFoundError):
    """The ``konstruktor`` executable is nowhere this package looks for it."""


def find_konstruktor_bin(binary: str | os.PathLike[str] | None = None) -> Path:
    """The ``konstruktor`` executable to run.

    Looked for, in order: ``binary`` when given, ``$KONSTRUKTOR_BIN``, the scripts
    directory the wheel installed it into (this environment's, then the user
    scheme's), and finally ``PATH`` -- which is where a source checkout or the
    release installer puts it.

    Raises:
        KonstruktorNotFoundError: If it is in none of those places.
    """
    explicit = binary or os.environ.get("KONSTRUKTOR_BIN")
    if explicit:
        path = Path(explicit)
        if not path.is_file():
            raise KonstruktorNotFoundError(f"{path} is not a konstruktor executable.")
        return path

    candidates = [Path(sysconfig.get_path("scripts")) / _NAME]
    user_scheme = sysconfig.get_preferred_scheme("user")
    candidates.append(Path(sysconfig.get_path("scripts", scheme=user_scheme)) / _NAME)
    for candidate in candidates:
        if candidate.is_file():
            return candidate

    on_path = shutil.which("konstruktor")
    if on_path:
        return Path(on_path)

    raise KonstruktorNotFoundError(
        "The konstruktor executable was not found. It ships in this package's wheels; "
        "from a source checkout build it with `cargo build -p konstruktor-cli` and "
        "point $KONSTRUKTOR_BIN at it."
    )
