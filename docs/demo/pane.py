"""Draw `git loom` status in the right part of the terminal: pane.py <cols> <rows> <x>.

loom only colors a terminal, so it runs in a pseudo-console; ConPTY passes its
escapes through, and everything but SGR is stripped.
"""
import os
import re
import shutil
import sys
import threading
import time

from winpty import PtyProcess

cols, rows, x = map(int, sys.argv[1:4])
width = cols - x - 2

exe = shutil.which("git-loom.exe") or shutil.which("git-loom")
p = PtyProcess.spawn([exe, "--theme", "dark"], dimensions=(200, 300), cwd=os.getcwd())
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
out = "".join(chunks)
out = re.sub(r"\x1b\][^\x07]*\x07", "", out)
out = re.sub(r"\x1b\[[0-9;?]*[A-Za-ln-z]", "", out)
lines = out.replace("\r", "").split("\n")
while lines and not lines[-1].strip():
    lines.pop()

def clip(line, n):
    """Cut `line` to `n` visible characters, keeping its SGR escapes."""
    res, seen = [], 0
    for tok in re.split(r"(\x1b\[[0-9;]*m)", line):
        if tok.startswith("\x1b"):
            res.append(tok)
        else:
            res.append(tok[: max(0, n - seen)])
            seen += len(tok)
    return "".join(res)


lines = [clip(line, width) for line in lines]
DIM, RESET = "\x1b[38;5;240m", "\x1b[0m"
body = [f"{DIM}git loom status{RESET}", ""] + lines
buf = ["\x1b7"]
for r in range(1, rows + 1):
    buf.append(f"\x1b[{r};{x}H\x1b[K{DIM}│{RESET} ")
    if r - 1 < len(body):
        buf.append(body[r - 1] + RESET)
buf.append("\x1b8")
sys.stdout.buffer.write("".join(buf).encode("utf-8"))
sys.stdout.flush()
os._exit(0)  # skip the PTY teardown, which can block as long as its EOF
