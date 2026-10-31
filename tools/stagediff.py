"""关卡号定位：用多份「第 N 关」快照取交集。

用法：
    python tools/stagediff.py save 1
    python tools/stagediff.py save 2
    python tools/stagediff.py save 3
    python tools/stagediff.py find 1 2 3        # 值随关卡号同步递增

同时尝试两种编码：
    · 1 基（第 1 关 = 1）
    · 0 基（第 1 关 = 0）
只保留在这几关里「每次刚好 +1」的字节，其余全排除。
"""
import os
import sys

sys.path.insert(0, __file__.rsplit("\\", 1)[0].rsplit("/", 1)[0])
from mem import Mem  # noqa: E402

PROBE = ".probe"


def path(name):
    return os.path.join(PROBE, f"stage_{name}.bin")


def save(label):
    m = Mem()
    d = m.ram_read(0, 0x40000)
    os.makedirs(PROBE, exist_ok=True)
    open(path(label), "wb").write(d)
    print(f"已保存 stage_{label}.bin（{len(d)} 字节）")


def find(labels):
    bufs = [open(path(x), "rb").read() for x in labels]
    nums = [int(x) for x in labels]
    size = min(len(b) for b in bufs)
    for name, offset in (("1 基（关1=1）", 0), ("0 基（关1=0）", -1)):
        hits = []
        for i in range(size):
            want = nums[0] + offset
            if want < 0 or want > 0x20:
                continue
            ok = True
            for b, n in zip(bufs, nums):
                if b[i] != n + offset:
                    ok = False
                    break
            if ok:
                hits.append(i)
        print(f"\n=== {name}：命中 {len(hits)} 个 ===")
        for i in hits:
            ctx = " ".join(f"{x:02X}" for x in bufs[-1][max(0, i - 6):i + 7])
            print(f"  0x{i:06X}   末关上下文: {ctx}")


def main():
    if len(sys.argv) < 3:
        print(__doc__)
        return
    mode = sys.argv[1]
    if mode == "save":
        save(sys.argv[2])
    elif mode == "find":
        find(sys.argv[2:])
    else:
        print(__doc__)


if __name__ == "__main__":
    main()
