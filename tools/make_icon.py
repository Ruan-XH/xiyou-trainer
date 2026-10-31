# -*- coding: utf-8 -*-
"""把一张方形 PNG 加工成程序图标的三件套。

用法：
    python tools/make_icon.py 源图.png

产出（都在 assets/ 下）：
    logo.png        256×256，留档用，也是以后换图时的唯一源文件
    logo.ico        16/24/32/48/64/128/256 多尺寸，给 exe 当文件图标
    icon64.rgba     64×64 裸 RGBA，编译期 include_bytes! 进去当窗口图标
    icon32.rgba     32×32 同上（任务栏小图标）

只依赖 Pillow。源图建议 ≥512×512 且背景透明；不透明也能用，
只是深色任务栏上会顶着个方块。
"""

import io
import os
import struct
import sys

from PIL import Image

HERE = os.path.dirname(os.path.abspath(__file__))
ASSETS = os.path.join(os.path.dirname(HERE), "assets")

ICO_SIZES = [16, 24, 32, 48, 64, 128, 256]


def squarify(img: Image.Image) -> Image.Image:
    """裁成正方形：不缩放内容，只把四周多余的部分切掉（居中）。"""
    w, h = img.size
    if w == h:
        return img
    side = min(w, h)
    left = (w - side) // 2
    top = (h - side) // 2
    return img.crop((left, top, left + side, top + side))


def dib_frame(img: Image.Image) -> bytes:
    """生成 ICO 里的 BMP 帧（BITMAPINFOHEADER + BGRA 倒序 + AND 掩码）。

    为什么不用 Pillow 的 `save(..., format="ICO")`：它把所有尺寸都存成 PNG 压缩帧，
    而 PNG 帧只有 Vista 以后的外壳认，GDI+（System.Drawing、不少资源编辑器、
    一些安装器）读到 256 会直接报「不是可用的 Icon 图片」。小尺寸用 DIB 帧最稳。
    """
    w, h = img.size
    px = img.convert("RGBA").load()

    xor = bytearray()
    for y in range(h - 1, -1, -1):
        for x in range(w):
            r, g, b, a = px[x, y]
            xor += bytes((b, g, r, a))

    # AND 掩码：32bpp 时外壳不看，但格式要求它在，按行补到 4 字节。
    stride = (w + 31) // 32 * 4
    mask = bytes(stride * h)

    header = struct.pack(
        "<IiiHHIIiiII",
        40,          # biSize
        w,           # biWidth
        h * 2,       # biHeight = 高 ×2（XOR + AND 两张图）
        1,           # biPlanes
        32,          # biBitCount
        0,           # biCompression = BI_RGB
        len(xor) + len(mask),
        0, 0, 0, 0,
    )
    return header + bytes(xor) + mask


def build_ico(big: Image.Image) -> bytes:
    """自己拼 ICO：≤128 用 DIB 帧，256 用 PNG（体积能小一个数量级）。"""
    frames = []
    for side in ICO_SIZES:
        frame = big.resize((side, side), Image.LANCZOS)
        if side >= 256:
            buf = io.BytesIO()
            frame.save(buf, format="PNG", optimize=True)
            frames.append((side, buf.getvalue()))
        else:
            frames.append((side, dib_frame(frame)))

    out = bytearray(struct.pack("<HHH", 0, 1, len(frames)))
    offset = 6 + 16 * len(frames)
    for side, data in frames:
        out += struct.pack(
            "<BBBBHHII",
            0 if side >= 256 else side,   # 宽（0 表示 256）
            0 if side >= 256 else side,   # 高
            0,                            # 调色板色数
            0,                            # 保留
            1,                            # planes
            32,                           # bpp
            len(data),
            offset,
        )
        offset += len(data)
    for _, data in frames:
        out += data
    return bytes(out)


def main() -> int:
    if len(sys.argv) < 2:
        print(__doc__)
        return 2

    src = sys.argv[1]
    img = Image.open(src)
    if img.mode != "RGBA":
        img = img.convert("RGBA")
    img = squarify(img)

    os.makedirs(ASSETS, exist_ok=True)

    big = img.resize((256, 256), Image.LANCZOS)
    big.save(os.path.join(ASSETS, "logo.png"))

    # ICO：自己拼，小尺寸 DIB、256 走 PNG（理由见 build_ico）。
    with open(os.path.join(ASSETS, "logo.ico"), "wb") as fh:
        fh.write(build_ico(big))

    for side in (64, 32):
        raw = img.resize((side, side), Image.LANCZOS).tobytes()
        with open(os.path.join(ASSETS, f"icon{side}.rgba"), "wb") as fh:
            fh.write(raw)

    print("源图 %s → %dx%d" % (os.path.basename(src), img.size[0], img.size[1]))
    for name in ["logo.png", "logo.ico", "icon64.rgba", "icon32.rgba"]:
        path = os.path.join(ASSETS, name)
        print("  %-12s %7d B" % (name, os.path.getsize(path)))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
