"""FBA 内存读写小工具：给定位址探索用。

用法（在项目目录下）：
    python tools/mem.py info                     # 打印 PID / 模块基址 / RAM 首地址
    python tools/mem.py hex 0x1BE98 0x140        # dump 一段 RAM（相对偏移）
    python tools/mem.py diff                     # 对比四个玩家槽，列出 1P 与众不同的字节
    python tools/mem.py watch 0x1BEF9,0x1BEF8    # 观察若干字节（回车刷新，q 退出）
"""

import ctypes
import ctypes.wintypes as wt
import struct
import sys

PROCESS_QUERY_INFORMATION = 0x0400
PROCESS_VM_READ = 0x0010
PROCESS_VM_WRITE = 0x0020
PROCESS_VM_OPERATION = 0x0008

TH32CS_SNAPPROCESS = 0x00000002
TH32CS_SNAPMODULE = 0x00000008


class PROCESSENTRY32(ctypes.Structure):
    _fields_ = [
        ("dwSize", wt.DWORD),
        ("cntUsage", wt.DWORD),
        ("th32ProcessID", wt.DWORD),
        ("th32DefaultHeapID", ctypes.POINTER(ctypes.c_ulong)),
        ("th32ModuleID", wt.DWORD),
        ("cntThreads", wt.DWORD),
        ("th32ParentProcessID", wt.DWORD),
        ("pcPriClassBase", ctypes.c_long),
        ("dwFlags", wt.DWORD),
        ("szExeFile", ctypes.c_char * 260),
    ]


class MODULEENTRY32(ctypes.Structure):
    _fields_ = [
        ("dwSize", wt.DWORD),
        ("th32ModuleID", wt.DWORD),
        ("th32ProcessID", wt.DWORD),
        ("GlblcntUsage", wt.DWORD),
        ("ProccntUsage", wt.DWORD),
        ("modBaseAddr", ctypes.POINTER(ctypes.c_byte)),
        ("modBaseSize", wt.DWORD),
        ("hModule", wt.HMODULE),
        ("szModule", ctypes.c_char * 256),
        ("szExePath", ctypes.c_char * 260),
    ]


k32 = ctypes.WinDLL("kernel32", use_last_error=True)


def find_pid(name):
    snap = k32.CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0)
    if snap == -1:
        raise OSError("CreateToolhelp32Snapshot 失败")
    entry = PROCESSENTRY32()
    entry.dwSize = ctypes.sizeof(entry)
    ok = k32.Process32First(snap, ctypes.byref(entry))
    pid = None
    while ok:
        if entry.szExeFile.decode("mbcs", "ignore").lower() == name.lower():
            pid = entry.th32ProcessID
            break
        ok = k32.Process32Next(snap, ctypes.byref(entry))
    k32.CloseHandle(snap)
    return pid


def module_base(pid, exe):
    snap = k32.CreateToolhelp32Snapshot(TH32CS_SNAPMODULE | 0x10, pid)
    if snap == -1:
        raise OSError("模块快照失败")
    entry = MODULEENTRY32()
    entry.dwSize = ctypes.sizeof(entry)
    base = None
    ok = k32.Module32First(snap, ctypes.byref(entry))
    while ok:
        if entry.szModule.decode("mbcs", "ignore").lower() == exe.lower():
            base = ctypes.cast(entry.modBaseAddr, ctypes.c_void_p).value
            break
        ok = k32.Module32Next(snap, ctypes.byref(entry))
    k32.CloseHandle(snap)
    return base


RAM_PTR = 0x01120518  # fba.exe：RAM 指针相对模块基址
EXE = "fba.exe"


class Mem:
    def __init__(self, exe=EXE, ram_ptr=RAM_PTR):
        pid = find_pid(exe)
        if not pid:
            raise SystemExit(f"没找到 {exe}，先把模拟器开起来")
        self.pid = pid
        self.h = k32.OpenProcess(
            PROCESS_QUERY_INFORMATION | PROCESS_VM_READ | PROCESS_VM_WRITE | PROCESS_VM_OPERATION,
            False,
            pid,
        )
        if not self.h:
            raise SystemExit(f"OpenProcess 失败：{ctypes.get_last_error()}（试试用管理员权限）")
        self.module = module_base(pid, exe)
        self.ram = self.u32(self.module + ram_ptr)
        if not self.ram:
            raise SystemExit("RAM 指针是 0，游戏还没载入 ROM")

    def read(self, addr, n):
        buf = ctypes.create_string_buffer(n)
        got = ctypes.c_size_t(0)
        ok = k32.ReadProcessMemory(
            self.h, ctypes.c_void_p(addr), buf, n, ctypes.byref(got)
        )
        if not ok:
            raise OSError(f"读 {addr:#x} 失败（{ctypes.get_last_error()}）")
        return buf.raw[: got.value]

    def write(self, addr, data):
        got = ctypes.c_size_t(0)
        ok = k32.WriteProcessMemory(
            self.h, ctypes.c_void_p(addr), data, len(data), ctypes.byref(got)
        )
        if not ok:
            raise OSError(f"写 {addr:#x} 失败（{ctypes.get_last_error()}）")
        return got.value

    def u8(self, addr):
        return self.read(addr, 1)[0]

    def u16(self, addr):
        return struct.unpack("<H", self.read(addr, 2))[0]

    def u32(self, addr):
        return struct.unpack("<I", self.read(addr, 4))[0]

    def ram_read(self, off, n):
        return self.read(self.ram + off, n)

    def ram_u8(self, off):
        return self.u8(self.ram + off)

    def hexdump(self, off, n, base=None):
        base = off if base is None else base
        data = self.ram_read(off, n)
        lines = []
        for i in range(0, len(data), 16):
            chunk = data[i : i + 16]
            hexs = " ".join(f"{b:02X}" for b in chunk)
            text = "".join(chr(b) if 32 <= b < 127 else "." for b in chunk)
            lines.append(
                f"{base + i:08X}  {hexs:<47}  {text}"
            )
        return "\n".join(lines)


# 玩家槽位：1P 起点 0x1BE98，每人 0x13E
PLAYER_BASE = 0x1BE98
PLAYER_STRIDE = 0x13E
PLAYER_N = 4


def cmd_info(m):
    print(f"PID        {m.pid}")
    print(f"模块基址   {m.module:#010x}")
    print(f"RAM 首地址 {m.ram:#010x}")
    print(f"1P 血量    {m.ram_u8(0x1BE98):#04x} … {m.u16(m.ram + 0x1BE98)}")


def cmd_hex(m, args):
    off = int(args[0], 0)
    n = int(args[1], 0) if len(args) > 1 else 0x80
    print(m.hexdump(off, n))


def cmd_diff(m, _args):
    datas = [m.ram_read(PLAYER_BASE + i * PLAYER_STRIDE, PLAYER_STRIDE) for i in range(PLAYER_N)]
    print(f"{'偏移':>8}  1P  2P  3P  4P   说明")
    same_zero = 0
    for off in range(PLAYER_STRIDE):
        vals = [d[off] for d in datas]
        if vals[0] != 0 and all(v == 0 for v in vals[1:]):
            print(f"  +{off:#04x}   " + "  ".join(f"{v:02X}" for v in vals) + "   ← 1P 非零、其余全 0")
        elif all(v == 0 for v in vals):
            same_zero += 1
    print(f"\n四槽全 0 的字节数：{same_zero}/{PLAYER_STRIDE}")


def cmd_watch(m, args):
    offs = []
    for a in args[0].split(","):
        offs.append(int(a, 0))
    while True:
        line = "  ".join(f"{o:#07x}={m.ram_u8(o):02X}({m.ram_u8(o)})" for o in offs)
        print(line)
        s = input("回车刷新，q 退出：").strip()
        if s.lower() == "q":
            break


def main():
    m = Mem()
    cmd = sys.argv[1] if len(sys.argv) > 1 else "info"
    args = sys.argv[2:]
    if cmd == "info":
        cmd_info(m)
    elif cmd == "hex":
        cmd_hex(m, args)
    elif cmd == "diff":
        cmd_diff(m, args)
    elif cmd == "watch":
        cmd_watch(m, args)
    else:
        print(__doc__)


if __name__ == "__main__":
    main()
