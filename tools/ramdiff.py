"""全 RAM 快照对比：找「某件事发生前后」变化的字节。

用法：
    python tools/ramdiff.py save a     # 存一份全 RAM 基线
    python tools/ramdiff.py cmp a      # 对比，列出变化的字节
    python tools/ramdiff.py cmp a --to b   # 两份已存的快照互相比

典型流程：存基线 → 在游戏里做一件事（投币加入 2P）→ 对比，
只关心「从 0 变非 0」和「值 +1」的地址。
"""

import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from mem import Mem  # noqa: E402

DIR = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", ".probe")
SPAN = 0x40000


def path_of(name):
    return os.path.join(DIR, name + ".bin")


def save(m, name):
    os.makedirs(DIR, exist_ok=True)
    data = m.ram_read(0, SPAN)
    with open(path_of(name), "wb") as f:
        f.write(data)
    print(f"已存 {path_of(name)}（{len(data)} 字节）")


def cmp(old, new, label):
    rows = []
    for i, (a, b) in enumerate(zip(old, new)):
        if a != b:
            rows.append((i, a, b))
    from0 = [r for r in rows if r[1] == 0 and r[2] != 0]
    to0 = [r for r in rows if r[1] != 0 and r[2] == 0]
    print(f"{label}")
    print(f"变化 {len(rows)} 个：0→非0 {len(from0)} 个，非0→0 {len(to0)} 个")
    print(f"\n【0 → 非0】{'（最可能是「启用」标志）' if from0 else ''}")
    for off, a, b in from0[:80]:
        print(f"  0x{off:06X}  {a:02X} → {b:02X}  ({b})")
    if len(from0) > 80:
        print(f"  …还有 {len(from0) - 80} 个")
    print(f"\n【非0 → 0】")
    for off, a, b in to0[:40]:
        print(f"  0x{off:06X}  {a:02X} → {b:02X}")
    if len(to0) > 40:
        print(f"  …还有 {len(to0) - 40} 个")


def main():
    args = sys.argv[1:]
    if not args:
        print(__doc__)
        return
    cmd = args[0]
    if cmd == "save":
        save(Mem(), args[1] if len(args) > 1 else "a")
    elif cmd == "cmp":
        name = args[1] if len(args) > 1 else "a"
        old = open(path_of(name), "rb").read()
        if "--to" in args:
            other = args[args.index("--to") + 1]
            new = open(path_of(other), "rb").read()
            label = f"{name} → {other}"
        else:
            new = Mem().ram_read(0, SPAN)
            label = f"{name} → 当前"
        cmp(old, new, label)
    else:
        print(__doc__)


if __name__ == "__main__":
    main()
