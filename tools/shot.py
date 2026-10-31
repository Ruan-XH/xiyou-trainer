"""抓取窗口截图：按标题找窗口，先试 PrintWindow，抓不到再用屏幕 BitBlt。

用法：python shot.py "窗口标题" 输出.png

egui 走 OpenGL，PrintWindow 有时候拿不到 GL 画面（整张黑），
所以拿到图先看一眼是不是纯色，是就退回屏幕拷贝（窗口会被提到前台）。
"""
import ctypes
import sys
from ctypes import wintypes as wt

user32 = ctypes.WinDLL("user32", use_last_error=True)
gdi32 = ctypes.WinDLL("gdi32", use_last_error=True)

user32.GetWindowDC.restype = wt.HDC
user32.GetWindowDC.argtypes = [wt.HWND]
user32.GetDC.restype = wt.HDC
user32.GetDC.argtypes = [wt.HWND]
gdi32.CreateCompatibleDC.restype = wt.HDC
gdi32.CreateCompatibleDC.argtypes = [wt.HDC]
gdi32.CreateCompatibleBitmap.restype = wt.HBITMAP
gdi32.CreateCompatibleBitmap.argtypes = [wt.HDC, ctypes.c_int, ctypes.c_int]
gdi32.SelectObject.restype = wt.HGDIOBJ
gdi32.SelectObject.argtypes = [wt.HDC, wt.HGDIOBJ]
gdi32.BitBlt.argtypes = [wt.HDC, ctypes.c_int, ctypes.c_int, ctypes.c_int,
                         ctypes.c_int, wt.HDC, ctypes.c_int, ctypes.c_int, wt.DWORD]
user32.PrintWindow.restype = wt.BOOL
user32.PrintWindow.argtypes = [wt.HWND, wt.HDC, wt.UINT]
user32.GetClientRect.argtypes = [wt.HWND, ctypes.POINTER(wt.RECT)]
user32.GetWindowRect.argtypes = [wt.HWND, ctypes.POINTER(wt.RECT)]
gdi32.DeleteObject.argtypes = [wt.HGDIOBJ]
gdi32.DeleteDC.argtypes = [wt.HDC]
user32.ReleaseDC.argtypes = [wt.HWND, wt.HDC]
gdi32.GetDIBits.restype = ctypes.c_int
gdi32.GetDIBits.argtypes = [
    wt.HDC, wt.HBITMAP, wt.UINT, wt.UINT,
    ctypes.c_void_p, ctypes.c_void_p, wt.UINT,
]

PW_RENDERFULLCONTENT = 0x00000002
SRCCOPY = 0x00CC0020


class BITMAPINFOHEADER(ctypes.Structure):
    _fields_ = [
        ("biSize", wt.DWORD), ("biWidth", wt.LONG), ("biHeight", wt.LONG),
        ("biPlanes", wt.WORD), ("biBitCount", wt.WORD),
        ("biCompression", wt.DWORD), ("biSizeImage", wt.DWORD),
        ("biXPelsPerMeter", wt.LONG), ("biYPelsPerMeter", wt.LONG),
        ("biClrUsed", wt.DWORD), ("biClrImportant", wt.DWORD),
    ]


def find(title_part):
    found = []

    @ctypes.WINFUNCTYPE(wt.BOOL, wt.HWND, wt.LPARAM)
    def cb(hwnd, _):
        length = user32.GetWindowTextLengthW(hwnd)
        if length:
            buf = ctypes.create_unicode_buffer(length + 1)
            user32.GetWindowTextW(hwnd, buf, length + 1)
            if title_part in buf.value and user32.IsWindowVisible(hwnd):
                found.append((hwnd, buf.value))
        return True

    user32.EnumWindows(cb, 0)
    return found


def grab(src_dc, x, y, w, h, mode, hwnd):
    mdc = gdi32.CreateCompatibleDC(src_dc)
    bmp = gdi32.CreateCompatibleBitmap(src_dc, w, h)
    old = gdi32.SelectObject(mdc, bmp)

    if mode == "print":
        user32.PrintWindow(hwnd, mdc, PW_RENDERFULLCONTENT)
    else:
        gdi32.BitBlt(mdc, 0, 0, w, h, src_dc, x, y, SRCCOPY)

    bi = BITMAPINFOHEADER()
    bi.biSize = ctypes.sizeof(bi)
    bi.biWidth = w
    bi.biHeight = -h
    bi.biPlanes = 1
    bi.biBitCount = 32
    bi.biCompression = 0

    buf = ctypes.create_string_buffer(w * h * 4)
    gdi32.GetDIBits(mdc, bmp, 0, h, buf, ctypes.byref(bi), 0)

    from PIL import Image

    img = Image.frombuffer("RGBA", (w, h), buf, "raw", "BGRA", 0, 1).convert("RGB")

    gdi32.SelectObject(mdc, old)
    gdi32.DeleteObject(bmp)
    gdi32.DeleteDC(mdc)
    return img


def shot(title, out):
    hits = find(title)
    if not hits:
        print("没找到窗口:", title)
        return 1
    hwnd, name = hits[0]
    print(f"抓 {name!r} hwnd={hwnd}")

    rect = wt.RECT()
    user32.GetWindowRect(hwnd, ctypes.byref(rect))
    w, h = rect.right - rect.left, rect.bottom - rect.top

    wdc = user32.GetWindowDC(hwnd)
    img = grab(wdc, 0, 0, w, h, "print", hwnd)
    user32.ReleaseDC(hwnd, wdc)

    colors = len(img.getcolors(maxcolors=64) or [])
    if colors <= 2:
        print("PrintWindow 拿到的是纯色，改用屏幕拷贝")
        user32.SetForegroundWindow(hwnd)
        import time
        time.sleep(0.6)
        sdc = user32.GetDC(None)
        img = grab(sdc, rect.left, rect.top, w, h, "screen", hwnd)
        user32.ReleaseDC(None, sdc)

    img.save(out)
    print("saved", out, img.size, "colors", len(img.getcolors(maxcolors=100000) or []))
    return 0


if __name__ == "__main__":
    title = sys.argv[1] if len(sys.argv) > 1 else "西游释厄传"
    out = sys.argv[2] if len(sys.argv) > 2 else "shot.png"
    raise SystemExit(shot(title, out))
