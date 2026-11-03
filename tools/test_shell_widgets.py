#!/usr/bin/env python3
"""Drive the `upyr shell-init` widgets in a real interactive shell.

Types each case at the prompt through a pseudo-terminal, presses the widget
keys, runs the line and checks what it printed. The cursor cases catch a
widget that leaves the cursor in the wrong place (bash counts it in bytes).

    tools/test_shell_widgets.py zsh "$(mktemp)"   # after: upyr shell-init zsh > that file

Set UPYR_BIN to run against a stand-in for `upyr` (see --fake).
"""

from __future__ import annotations

import argparse
import os
import pty
import re
import select
import sys
import time
from pathlib import Path

LINE = "\x18\x0c"  # Ctrl+X Ctrl+L
WORD = "\x18l"  # Ctrl+X L
LEFT = "\x02"  # Ctrl+B
HOME, END, MARK = "\x01", "\x05", "\x00"  # Ctrl+A, Ctrl+E, Ctrl+Space

# What a Ukrainian-layout `echo upyr-ok` looks like, and its parts.
TYPED = "усрщ гзнк-щл"
FAKE_TABLE = {TYPED: "echo upyr-ok", "гзнк-щл": "upyr-ok"}

# (name, typed, keys, expected output, shells; None means every shell)
CASES = [
    ("whole line", TYPED, [LINE], "upyr-ok", None),
    ("whole line keeps the cursor", TYPED, [LEFT * 3, LINE, "Z"], "upyrZ-ok", None),
    ("word before the cursor", "echo ABC гзнк-щл", [WORD], "ABC upyr-ok", None),
    ("word before trailing spaces", "echo гзнк-щл ", [WORD, "end"], "upyr-ok end", None),
    (
        "word in the middle of the line",
        "echo гзнк-щл tail",
        [LEFT * 5, WORD, "X"],
        "upyr-okX tail",
        None,
    ),
    ("the active region", TYPED, [HOME, MARK, END, WORD], "upyr-ok", {"zsh"}),
]

SHELLS = {
    "zsh": (["zsh", "-f", "-i"], "bindkey -e; source {script}"),
    "bash": (["bash", "--norc", "--noprofile", "-i"], "source {script}"),
    "fish": (["fish", "--no-config", "-i"], "source {script}"),
}

ESCAPES = re.compile(r"\x1b(\[[0-9;?<=>]*[ -/]*[@-~]|\][^\x07\x1b]*(\x07|\x1b\\)|[()][A-Z0-9]|[=>78M])")


def fake_upyr(path: Path) -> None:
    """A stand-in `upyr` whose `--installed` fails, to exercise the fallback."""
    path.write_text(
        "#!/usr/bin/env python3\n"
        "import sys\n"
        f"TABLE = {FAKE_TABLE!r}\n"
        "args = sys.argv[1:]\n"
        "if '--installed' in args: sys.exit(1)\n"
        "text = args[args.index('--') + 1]\n"
        "if text not in TABLE: sys.exit(1)\n"
        "print(TABLE[text])\n",
        encoding="utf-8",
    )
    path.chmod(0o755)


# Queries a shell sends its terminal, and a terminal's answers. fish 4 asks
# for the cursor position and the device attributes at startup and loses
# what is typed before the answers come.
ANSWERS = (
    (b"\x1b[6n", b"\x1b[1;1R"),
    (b"\x1b[0c", b"\x1b[?62;22c"),
    (b"\x1b[c", b"\x1b[?62;22c"),
)


def read_for(fd: int, seconds: float) -> str:
    chunks = []
    deadline = time.monotonic() + seconds
    while (remaining := deadline - time.monotonic()) > 0:
        ready, _, _ = select.select([fd], [], [], remaining)
        if not ready:
            break
        try:
            data = os.read(fd, 65536)
        except OSError:
            break
        if not data:
            break
        for query, answer in ANSWERS:
            for _ in range(data.count(query)):
                os.write(fd, answer)
        chunks.append(data)
    return b"".join(chunks).decode("utf-8", "replace")


def output_lines(text: str) -> list[str]:
    plain = ESCAPES.sub("", text)
    return [line.strip() for line in re.split(r"\r?\n|\r", plain)]


def run(shell: str, script: Path) -> int:
    command, load = SHELLS[shell]
    locale = os.environ.get("UPYR_TEST_LOCALE", "en_US.UTF-8")
    env = dict(os.environ, TERM="xterm", LANG=locale, LC_ALL=locale, PS1="$ ")
    pid, fd = pty.fork()
    if pid == 0:
        os.execvpe(command[0], command, env)
    read_for(fd, 1.5)
    os.write(fd, (load.format(script=script) + "\r").encode())
    read_for(fd, 1.0)
    failures = 0
    cases = [case for case in CASES if case[4] is None or shell in case[4]]
    for name, typed, actions, expected, _ in cases:
        os.write(fd, typed.encode())
        read_for(fd, 0.3)
        for action in actions:
            os.write(fd, action.encode())
            read_for(fd, 0.8)
        os.write(fd, b"\r")
        lines = output_lines(read_for(fd, 1.0))
        if expected in lines:
            print(f"ok    {shell}: {name}")
        else:
            failures += 1
            shown = [line for line in lines if line][-4:]
            print(f"FAIL  {shell}: {name}: expected {expected!r}, last lines {shown!r}")
        # Clear anything left on the line before the next case.
        os.write(fd, b"\x03")
        read_for(fd, 0.3)
    os.write(fd, b"exit\r")
    read_for(fd, 0.5)
    os.close(fd)
    os.waitpid(pid, 0)
    print(f"{len(cases) - failures}/{len(cases)} passed")
    return failures


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("shell", choices=sorted(SHELLS))
    parser.add_argument("script", type=Path, help="output of `upyr shell-init SHELL`")
    parser.add_argument(
        "--fake",
        type=Path,
        help="write a stand-in upyr here and point UPYR_BIN at it",
    )
    args = parser.parse_args()
    if args.fake:
        fake_upyr(args.fake)
        os.environ["UPYR_BIN"] = str(args.fake)
    return 1 if run(args.shell, args.script) else 0


if __name__ == "__main__":
    sys.exit(main())
