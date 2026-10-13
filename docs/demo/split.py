"""Split-screen terminal for the CLI demo: `git loom status` on the left, bash on the right.

ttyd's ConPTY repaints whole rows, which blanks anything drawn beside a shell, so bash
runs in its own pseudo-console here, emulated with pyte, and this process owns every
cell: long lines wrap inside the shell pane. Bash's PROMPT_COMMAND (split.bashrc)
writes `<on|off> <counter> <cwd>` to $PANE_STATE; each new line redraws the pane.

Usage: split.py <pane columns> <bash.exe> <rcfile>
"""
import copy
import ctypes
import os
import shutil
import sys
import threading
import time

import pyte
from winpty import PtyProcess

PANE_W = int(sys.argv[1])
GAP = 3  # " │ " between the pane and the shell
STATE = os.environ["PANE_STATE"]
GIT_LOOM = shutil.which("git-loom.exe") or shutil.which("git-loom")

k32 = ctypes.windll.kernel32
h_in, h_out = k32.GetStdHandle(-10), k32.GetStdHandle(-11)
saved_in = ctypes.c_uint32()
k32.GetConsoleMode(h_in, ctypes.byref(saved_in))
# Raw input as VT sequences, so arrows and Ctrl keys reach bash untouched.
k32.SetConsoleMode(h_in, (saved_in.value & ~0x7) | 0x200)
out_mode = ctypes.c_uint32()
k32.GetConsoleMode(h_out, ctypes.byref(out_mode))
k32.SetConsoleMode(h_out, out_mode.value | 0x4 | 0x8)
k32.SetConsoleCP(65001)
k32.SetConsoleOutputCP(65001)

COLS, ROWS = shutil.get_terminal_size()
lock = threading.Lock()
dirty = threading.Event()


ALT_MODES = {47, 1047, 1049}


class Screen(pyte.Screen):
    """pyte screen that answers the pseudo-console's queries, keeps faint text, and
    has an alternate screen, so the shell comes back after a full-screen program."""

    def __init__(self, cols, rows, pty=None):
        super().__init__(cols, rows)
        self.pty = pty
        self.main = None

    def set_mode(self, *modes, **kwargs):
        if kwargs.get("private") and ALT_MODES & set(modes) and self.main is None:
            self.main = copy.deepcopy(self.buffer), copy.copy(self.cursor)
            self.erase_in_display(2)
        super().set_mode(*modes, **kwargs)

    def reset_mode(self, *modes, **kwargs):
        if kwargs.get("private") and ALT_MODES & set(modes) and self.main is not None:
            buffer, self.cursor = self.main
            self.buffer.clear()
            self.buffer.update(buffer)
            self.dirty.update(range(self.lines))
            self.main = None
        super().reset_mode(*modes, **kwargs)

    def write_process_input(self, data):
        if self.pty:
            self.pty.write(data)

    def select_graphic_rendition(self, *attrs, **kwargs):
        # pyte drops SGR 2 (faint); carry it as blink, which loom never uses.
        out, i = [], 0
        while i < len(attrs):
            a = attrs[i]
            if a in (38, 48):
                n = 3 if attrs[i + 1 : i + 2] == (5,) else 5
                # ConPTY sends the 16 basic colors as 38;5;n, which pyte pins to
                # xterm's RGB: keep them basic, so the terminal theme colors them.
                if n == 3 and len(attrs) > i + 2 and attrs[i + 2] < 16:
                    c = attrs[i + 2]
                    out.append((30 if a == 38 else 40) + c if c < 8 else (90 if a == 38 else 100) + c - 8)
                else:
                    out += attrs[i : i + n]
                i += n
                continue
            out += [5] if a == 2 else [22, 25] if a == 22 else [a]
            i += 1
        super().select_graphic_rendition(*out, **kwargs)


NAMES = ["black", "red", "green", "brown", "blue", "magenta", "cyan", "white"]


def color(c, base):
    if c == "default":
        return []
    if c in NAMES:
        return [str(base + NAMES.index(c))]
    if c.startswith("bright") and c[6:] in NAMES:
        return [str(base + 60 + NAMES.index(c[6:]))]
    return [f"{base + 8};2;{int(c[0:2], 16)};{int(c[2:4], 16)};{int(c[4:6], 16)}"]


def sgr(ch):
    codes = ["0"]
    codes += ["1"] * ch.bold + ["2"] * ch.blink + ["3"] * ch.italics
    codes += ["4"] * ch.underscore + ["7"] * ch.reverse + ["9"] * ch.strikethrough
    codes += color(ch.fg, 30) + color(ch.bg, 40)
    return "\x1b[" + ";".join(codes) + "m"


def row_text(line, width):
    """`line` (a pyte row) as text with SGR escapes, padded to `width` cells."""
    res, cur = [], None
    for x in range(width):
        ch = line[x]
        if ch.data == "":  # right half of a wide character
            continue
        s = sgr(ch)
        if s != cur:
            res.append(s)
            cur = s
        res.append(ch.data)
    return "".join(res)


pane = Screen(PANE_W, ROWS)
pane_on = False
shell_x = 0
shell = PtyProcess.spawn(
    [sys.argv[2], "--rcfile", sys.argv[3], "-i"], dimensions=(ROWS, COLS)
)
screen = Screen(COLS, ROWS, shell)
stream = pyte.Stream(screen)
shown = [None] * ROWS


def render():
    with lock:
        buf = ["\x1b[?25l"]
        sep = sgr(pane.default_char)[:-1] + ";38;5;240m │ "
        for y in range(ROWS):
            text = row_text(screen.buffer[y], screen.columns)
            if pane_on:
                text = row_text(pane.buffer[y], PANE_W) + sep + text
            if text != shown[y]:
                shown[y] = text
                buf.append(f"\x1b[{y + 1};1H{text}\x1b[0m")
        c = screen.cursor
        buf.append(f"\x1b[{c.y + 1};{c.x + shell_x + 1}H")
        if not c.hidden:
            buf.append("\x1b[?25h")
    sys.stdout.buffer.write("".join(buf).encode("utf-8"))
    sys.stdout.flush()


def run_status(cwd):
    """`git loom status` as a terminal of the pane's size shows it, colors included."""
    p = PtyProcess.spawn([GIT_LOOM, "--theme", "dark"], cwd=cwd, dimensions=(300, PANE_W))
    chunks = []

    def reader():
        answered = False
        while True:
            try:
                chunks.append(p.read(65536))
            except EOFError:
                return
            # ConPTY asks for device attributes at startup and stalls until answered.
            if not answered and "\x1b[c" in "".join(chunks):
                p.write("\x1b[?61;4c")
                answered = True

    # ConPTY takes seconds to report EOF after the child exits: stop at its exit instead.
    threading.Thread(target=reader, daemon=True).start()
    while p.isalive():
        time.sleep(0.02)
    time.sleep(0.15)
    out = Screen(PANE_W, 300)
    pyte.Stream(out).feed("".join(chunks))
    title = Screen(PANE_W, 2)
    pyte.Stream(title).feed("\x1b[38;5;240mgit loom status\x1b[0m")
    with lock:
        pane.reset()
        for y in range(1, ROWS):
            pane.buffer[y] = out.buffer[y - 1]
        pane.buffer[0] = title.buffer[0]
    dirty.set()


def watch_state():
    global pane_on, shell_x
    last = None
    while True:
        time.sleep(0.05)
        try:
            with open(STATE, encoding="utf-8") as f:
                state = f.read().strip()
        except OSError:
            continue
        if state == last or not state:
            continue
        last = state
        mode, _, cwd = state.split(" ", 2)
        on = mode == "on"
        if on != pane_on:
            with lock:
                pane_on, shell_x = on, PANE_W + GAP if on else 0
                width = COLS - shell_x
                screen.resize(ROWS, width)
                shell.setwinsize(ROWS, width)
                shown[:] = [None] * ROWS
            # The pseudo-console redraws its old screen after a resize, the line
            # that switched the pane included: wait for it, then start clean.
            time.sleep(0.5)
            with lock:
                screen.reset()
            shell.write("")
            dirty.set()
        if on:
            threading.Thread(target=run_status, args=(cwd,), daemon=True).start()


def read_shell():
    while True:
        try:
            data = shell.read(65536)
        except EOFError:
            break
        with lock:
            stream.feed(data)
        dirty.set()
    dirty.set()


def read_keys():
    while True:
        data = os.read(0, 1024)
        if data:
            shell.write(data.decode("utf-8", "replace"))


for target in (watch_state, read_shell, read_keys):
    threading.Thread(target=target, daemon=True).start()
sys.stdout.write("\x1b[2J")
while shell.isalive():
    if dirty.wait(0.5):
        dirty.clear()
        time.sleep(0.01)  # coalesce bursts into one frame
        render()
k32.SetConsoleMode(h_in, saved_in.value)
sys.stdout.write("\x1b[0m\x1b[2J\x1b[H")
os._exit(0)  # skip the PTY teardown, which can block as long as its EOF
