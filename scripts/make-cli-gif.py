#!/usr/bin/env python3
"""Render README's `demo-cli.gif` from real `konstruktor` output.

    python3 scripts/make-cli-gif.py demo-cli.gif

One frame per scene, held a couple of seconds — a slideshow, like `demo.gif` beside it,
rather than a recording. There is no asciinema or vhs in the loop: ffmpeg draws the text
itself, which is why each line carries its own colour here.

Every line below was captured by actually running the command; this file only decides
where it sits and what colour it is. When the CLI's output changes, re-run the commands
in SCENES and paste the new output in rather than editing it by hand — the point of the
gif is that it shows what the tool really prints.
"""

import subprocess
import sys
from pathlib import Path

OUT = Path(sys.argv[1] if len(sys.argv) > 1 else "demo-cli.gif")
WORK = Path("/tmp/kdemo/gif")
WORK.mkdir(parents=True, exist_ok=True)

MONO = "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf"
MONO_B = "/usr/share/fonts/truetype/dejavu/DejaVuSansMono-Bold.ttf"

W, H = 1000, 430
PAD_X, TOP = 40, 34
LINE = 26
SIZE = 17

BG = "0x0d0f14"
CHROME = "0x171a21"
DOT = ["0xff5f57", "0xfebc2e", "0x28c840"]

FG = "0xd6dae0"      # ordinary output
DIM = "0x7f8792"     # values, secondary text
CMD = "0xffffff"     # the command being run
PROMPT = "0x5aa9f8"  # the $
OK = "0x53d769"
WARN = "0xf0b429"
ACCENT = "0x9d8cff"  # headings inside output

# (colour, bold, text). A leading None line is a blank spacer.
SCENES = [
    [
        (PROMPT, True, "$ konstruktor --help"),
        None,
        (ACCENT, True, "What you can create:"),
        (FG, False, "  hub      the services — rekuest, mikro, fluss and the rest — behind a gateway"),
        (FG, False, "  engine   a plugin engine: one deployer container running an organization's plugins"),
        (FG, False, "  coord    a coordination server: where users, organizations and permissions live,"),
        (FG, False, "           and what a hub or an engine authorizes against"),
        None,
        (DIM, False, "Everything else takes any of the three. `[target]` is a path or a registered"),
        (DIM, False, "name; left out, it is the deployment you are standing in."),
    ],
    [
        (PROMPT, True, "$ konstruktor doctor"),
        None,
        (DIM, False, "  docker   29.4.2"),
        (DIM, False, "  compose  5.1.3"),
        (DIM, False, "  daemon   API 1.54"),
        (DIM, False, "  git      2.53.0"),
        None,
        (OK, False, "  ✓ Docker is ready."),
    ],
    [
        (PROMPT, True, "$ konstruktor hub create ~/MyHub --identifier lab-hub --dry-run"),
        None,
        (DIM, False, "  services      rekuest, mikro, fluss, kabinet, kraph, alpaka"),
        (DIM, False, "  advertised    127.0.0.1, localhost"),
        None,
        (FG, True, "  Would write:"),
        (DIM, False, "  configs/Caddyfile"),
        (DIM, False, "  configs/rekuest.yaml"),
        (DIM, False, "  configs/mikro.yaml          … and four more"),
        (DIM, False, "  docker-compose.yaml"),
        None,
        (FG, False, "  Nothing was created. Drop --dry-run to do it for real."),
    ],
    [
        (PROMPT, True, "$ konstruktor status MyHub"),
        None,
        (DIM, False, "  folder        ~/MyHub"),
        (DIM, False, "  gateway       http://localhost:8080"),
        (DIM, False, "  coordination  go.arkitekt.live"),
        (DIM, False, "  services      rekuest, kabinet, mikro, fluss, kraph"),
        (DIM, False, "  channel       next"),
        (DIM, False, "  storage       deployment folder"),
        (DIM, False, "  mesh          not joined"),
        (DIM, False, "  authorized    not yet"),
        None,
        (FG, False, "  Nothing running."),
    ],
    [
        (PROMPT, True, "$ konstruktor update --check MyHub"),
        None,
        (DIM, False, "  Asking the registries what has moved…"),
        (DIM, False, "  rekuest     up to date"),
        (DIM, False, "  mikro       up to date"),
        (DIM, False, "  fluss       up to date"),
        (DIM, False, "  db          up to date"),
        (DIM, False, "  minio       up to date"),
        (DIM, False, "  gateway     up to date"),
        None,
        (OK, False, "  ✓ Everything is up to date."),
    ],
]


def esc(path: str) -> str:
    return path.replace("\\", "/").replace(":", r"\:")


def render(scene, index):
    filters = [
        f"drawbox=x=0:y=0:w={W}:h=44:color={CHROME}:t=fill",
    ]
    for i, colour in enumerate(DOT):
        filters.append(
            f"drawbox=x={22 + i * 22}:y=17:w=11:h=11:color={colour}:t=fill"
        )

    y = 44 + TOP
    for line in scene:
        if line is None:
            y += LINE
            continue
        colour, bold, text = line
        f = WORK / f"s{index}_{y}.txt"
        f.write_text(text + "\n", encoding="utf-8")
        font = MONO_B if bold else MONO
        filters.append(
            f"drawtext=textfile={esc(str(f))}:fontfile={esc(font)}:"
            f"fontcolor={colour}:fontsize={SIZE}:x={PAD_X}:y={y}"
        )
        y += LINE

    out = WORK / f"frame{index:02d}.png"
    cmd = [
        "ffmpeg", "-y", "-v", "error",
        "-f", "lavfi", "-i", f"color=c={BG}:s={W}x{H}",
        "-vf", ",".join(filters),
        "-frames:v", "1", str(out),
    ]
    subprocess.run(cmd, check=True)
    return out


frames = [render(scene, i) for i, scene in enumerate(SCENES)]
print(f"rendered {len(frames)} frames")

# Each frame held the same length, looping forever. Two passes so the palette is built
# from every frame rather than the first one, which otherwise loses the greens.
listing = WORK / "list.txt"
listing.write_text(
    "".join(f"file '{f}'\nduration 2.6\n" for f in frames)
    + f"file '{frames[-1]}'\n",
    encoding="utf-8",
)

subprocess.run(
    ["ffmpeg", "-y", "-v", "error", "-f", "concat", "-safe", "0", "-i", str(listing),
     "-vf", "palettegen=stats_mode=diff", str(WORK / "palette.png")],
    check=True,
)
subprocess.run(
    ["ffmpeg", "-y", "-v", "error",
     "-f", "concat", "-safe", "0", "-i", str(listing),
     "-i", str(WORK / "palette.png"),
     "-lavfi", "paletteuse=dither=bayer:bayer_scale=3",
     "-loop", "0", str(OUT)],
    check=True,
)
print(f"wrote {OUT} ({OUT.stat().st_size // 1024} KB)")
