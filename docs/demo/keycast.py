"""Burn the keys a tape presses into its video, as a keycap in the bottom-right corner.

Usage: keycast.py <tape> <raw.mp4> <out.mp4>

Key times come from the tape: its sleeps and typing delays, from `Show` on, scaled to
the recorded duration (vhs runs slightly faster than the sum). Single-letter `Type`s
are TUI shortcuts, labeled with their action; longer ones are text typed into a field.
The first `Type` and its `Enter` launch the TUI at the prompt, where they already show.
"""
import os
import re
import subprocess
import sys

TYPING = 0.06
HOLD = 1.0  # how long a keycap stays up when no other key replaces it
NAMES = {"Down": "↓", "Up": "↑", "Left": "←", "Right": "→", "Enter": "Enter",
         "Tab": "Tab", "Space": "Space", "Escape": "Esc", "Backspace": "⌫"}
ACTIONS = {"b": "new branch", "m": "move", "f": "fold", "F": "fold hunks",
           "c": "commit", "C": "commit hunks", "s": "split", "r": "reword",
           "a": "absorb", "d": "drop", "u": "update", "p": "push",
           "L": "log", "?": "help", "q": "quit"}
KEY = r"{\c&H8BE2F9&}"    # Catppuccin yellow, as BGR
LABEL = r"{\c&HC8ADA6&}"  # Catppuccin subtext


def seconds(s):
    m = re.fullmatch(r"([\d.]+)(ms|s)?", s)
    return float(m.group(1)) / (1000 if m.group(2) == "ms" else 1)


def presses(tape):
    """(time, kind, text) for each key or typed string after `Show`."""
    typing, t, shown, out = TYPING, 0.0, False, []
    for line in open(tape, encoding="utf-8"):
        line = line.strip()
        if m := re.fullmatch(r"Set TypingSpeed (\S+)", line):
            typing = seconds(m.group(1))
        if not line or line.startswith(("#", "Set", "Output", "Hide")):
            continue
        if line == "Show":
            shown, t = True, 0.0
        elif not shown:
            continue
        elif line.startswith("Sleep"):
            t += seconds(line.split()[1])
        elif m := re.fullmatch(r'Type(?:@(\S+))? "(.*)"', line):
            speed = seconds(m.group(1)) if m.group(1) else typing
            out.append((t, "type", m.group(2)))
            t += len(m.group(2)) * speed
        else:
            out.append((t, "key", line.split()[0]))
            t += typing
    return out[2:], t


def caption(kind, text):
    if kind == "key":
        return KEY + NAMES.get(text, text)
    if len(text) == 1:
        action = ACTIONS.get(text)
        return KEY + text + (LABEL + "  " + action if action else "")
    return LABEL + "type  " + KEY + text


def stamp(t):
    return f"{int(t // 3600)}:{int(t // 60 % 60):02}:{t % 60:05.2f}"


def main(tape, raw, out):
    events, total = presses(tape)
    length = float(subprocess.check_output(
        ["ffprobe", "-v", "error", "-show_entries", "format=duration", "-of", "csv=p=0", raw]))
    scale = length / total
    shown = []  # (start, text), a repeated key counted: "↓ ×3"
    for t, kind, text in events:
        cap = caption(kind, text)
        if shown and shown[-1][2] == cap and t * scale - shown[-1][3] < HOLD:
            count = shown[-1][4] + 1
            shown.append((t * scale, cap + f"{LABEL}  ×{count}", cap, t * scale, count))
        else:
            shown.append((t * scale, cap, cap, t * scale, 1))
    lines = []
    for i, (start, text, *_) in enumerate(shown):
        end = start + HOLD
        if i + 1 < len(shown):
            end = min(end, shown[i + 1][0])
        lines.append(f"Dialogue: 0,{stamp(start)},{stamp(end)},Key,,0,0,0,,{text}")
    ass = os.path.splitext(out)[0] + ".keys.ass"
    with open(ass, "w", encoding="utf-8") as f:
        f.write("""[Script Info]
ScriptType: v4.00+
PlayResX: 1920
PlayResY: 1080

[V4+ Styles]
Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding
Style: Key,Segoe UI Semibold,52,&H00F4D6CD,&H00F4D6CD,&H105A4745,&H105A4745,0,0,0,0,100,100,0,0,3,18,0,3,0,70,110,1

[Events]
Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text
""" + "\n".join(lines) + "\n")
    # The ass filter takes a path whose drive colon it would read as an option separator.
    subprocess.run(["ffmpeg", "-v", "error", "-y", "-i", os.path.abspath(raw),
                    "-vf", "ass=" + os.path.basename(ass),
                    "-c:v", "libx264", "-preset", "slow", "-crf", "20", "-pix_fmt", "yuv420p",
                    "-movflags", "+faststart", os.path.abspath(out)],
                   cwd=os.path.dirname(os.path.abspath(ass)), check=True)
    os.remove(ass)


if __name__ == "__main__":
    main(*sys.argv[1:4])
