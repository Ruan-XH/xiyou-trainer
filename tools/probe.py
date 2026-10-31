"""玩家区快照对比：确认「某个字节是不是玩家在场标志」。

用法：
    python tools/probe.py save base      # 存一份基线（投币前）
    python tools/probe.py cmp base       # 比较（投币后），列出变化的字节
    python tools/probe.py cmp base --all # 连没变化的也列出来

比较范围默认是四个玩家槽（0x1BE98 起，含 0x603 处的角色字段），
可以用 --off / --len 覆盖。
"""

import json
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from mem import Mem  # noqa: E402

DIR = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", ".probe")
PLAYER_BASE = 0x1BE98
PLAYER_STRIDE = 0x13E
SPAN = 0x9D0  # 覆盖四槽 + 每个槽 0x603 处的角色字段


def key_offsets():
    """四槽里我们关心的字段相对各自槽首的偏移。"""
    return [
        ("hp", 0x00, 2),
        ("qi", 0xB5, 1),
        ("active", 0x61, 1),
        ("char", 0x603, 1),
    ]


def slot_of(off):
    rel = off - PLAYER_BASE
    if rel < 0:
        return None
    idx = rel // PLAYER_STRIDE
    return (idx, rel) if idx < 4 else None


def save(m, name):
    os.makedirs(DIR, exist_ok=True)
    data = m.ram_read(PLAYER_BASE, SPAN)
    path = os.path.join(DIR, name + ".bin")
    with open(path, "wb") as f:
        f.write(data)
    print(f"已存 {path}（{len(data)} 字节，RAM 基址 {m.ram:#x}）")


def cmp(m, name, show_all=False):
    path = os.path.join(DIR, name + ".bin")
    if not os.path.exists(path):
        raise SystemExit(f"没有基线 {path}，先 save")
    old = open(path, "rb").read()
    new = m.ram_read(PLAYER_BASE, SPAN)
    if len(old) != len(new):
        raise SystemExit("长度不一致")

    rows = []
    for i, (a, b) in enumerate(zip(old, new)):
        if a != b:
            off = PLAYER_BASE + i
            s = slot_of(off)
            tag = f"{s[0]+1}P+{s[1]:#05x}" if s else "-"
            rows.append((off, tag, a, b))

    print(f"变化字节 {len(rows)} 个（范围 0x{PLAYER_BASE:X} … 0x{PLAYER_BASE+SPAN:X}）")
    print(f"{'绝对偏移':>9} {'定位':>10} {'旧':>4} {'新':>4}")
    for off, tag, a, b in rows:
        star = ""
        if slot_of(off) and slot_of(off)[1] == 0x61:
            star = "   ← active 候选"
        print(f"0x{off:07X} {tag:>10} {a:>4} {b:>4}{star}")

    if show_all:
        print("\n四槽关键字段：")
        print(f"{'槽':>3} {'hp':>5} {'qi':>4} {'active':>7} {'char':>5}")
        for idx in range(4):
            b = idx * PLAYER_STRIDE
            vals = []
            for _, rel, size in key_offsets():
                if size == 2:
                    vals.append(int.from_bytes(new[b + rel : b + rel + 2], "little"))
                else:
                    vals.append(new[b + rel])
            print(f"{idx+1}P  {vals[0]:>5} {vals[1]:>4} {vals[2]:>7} {vals[3]:>5}")


def main():
    m = Mem()
    cmd = sys.argv[1] if len(sys.argv) > 1 else "cmp"
    name = sys.argv[2] if len(sys.argv) > 2 else "base"
    if cmd == "save":
        save(m, name)
    elif cmd == "cmp":
        cmp(m, name, "--all" in sys.argv)
    else:
        print(__doc__)


if __name__ == "__main__":
    main()
