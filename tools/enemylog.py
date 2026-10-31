"""盯住敌兵槽的在场标志，只在变化时打印一行。

用法：
    python tools/enemylog.py [秒数]

日志格式（每次标志向量或血量变化各打一行）：
    0.35s  flag 2 0 0 0 0 0  ->  2 2 0 0 0 0     hp 20 20 0 0 0 0
"""
import sys
import time

sys.path.insert(0, __file__.rsplit("\\", 1)[0].rsplit("/", 1)[0])
from mem import Mem  # noqa: E402

ANCHOR = 0x11A88
STRIDE = 0x13E
FLAG_REL = -0x57
SLOTS = 6


def read(m):
    flags = [m.ram_u8(ANCHOR + FLAG_REL + j * STRIDE) for j in range(SLOTS)]
    hps = [m.u16(m.ram + ANCHOR + j * STRIDE) for j in range(SLOTS)]
    return flags, hps


def fmt(v):
    return " ".join(f"{x:>2}" for x in v)


def main():
    seconds = float(sys.argv[1]) if len(sys.argv) > 1 else 60.0
    m = Mem()
    t0 = time.time()
    last = None
    print(f"监听 {seconds:.0f} 秒，只在变化时打印（flag = 0x11A31 起，间隔 0x13E）")
    print("   时间   flag                          hp")
    while time.time() - t0 < seconds:
        try:
            flags, hps = read(m)
        except Exception as e:  # 模拟器关了之类
            print(f"读取失败：{e}")
            return
        cur = (tuple(flags), tuple(hps))
        if cur != last:
            stamp = f"{time.time() - t0:5.2f}s"
            if last is None:
                print(f"{stamp}   {fmt(flags):<28}  {fmt(hps)}")
            else:
                pf, ph = last
                if pf != tuple(flags):
                    print(f"{stamp}  flag {fmt(pf):<24} ->  {fmt(flags)}")
                if ph != tuple(hps):
                    print(f"{stamp}  hp   {fmt(ph):<24} ->  {fmt(hps)}")
            last = cur
        time.sleep(0.1)


if __name__ == "__main__":
    main()
