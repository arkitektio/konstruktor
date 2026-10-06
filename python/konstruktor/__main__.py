"""``python -m konstruktor``: the CLI itself."""

import os
import subprocess
import sys

from konstruktor._binary import find_konstruktor_bin


def main() -> None:
    """Run the ``konstruktor`` executable with this process's arguments."""
    binary = os.fspath(find_konstruktor_bin())
    if sys.platform == "win32":
        sys.exit(subprocess.run([binary, *sys.argv[1:]]).returncode)
    os.execv(binary, [binary, *sys.argv[1:]])


if __name__ == "__main__":
    main()
