# winmem

轻量级 Windows 跨进程内存读写库。纯 Rust，基于微软官方 `windows` crate，**不依赖任何 C 库**。

```rust
let process = Process::by_name("notepad.exe")?;
let base = process.module_base("notepad.exe")?;
let value: u32 = process.read(base + 0x1000)?;
process.write(base + 0x1000, &12345u32)?;
```

## 为什么不用 libmem

`libmem` 功能确实全，但它是 C 库的 binding：编译要 C 工具链，分发要带 native 依赖，换台机器就得重新折腾一遍。

这个库的选择是：**纯 Rust + 零外部依赖**，`cargo add` 完就能用，复制粘贴一个目录就能带走。功能覆盖日常逆向/调试需要的八成，剩下两成（注入、hook）用别的专用 crate 补更合适。

## 功能

| 能力 | API |
|---|---|
| 按 PID / 进程名附加 | `Process::by_pid` / `Process::by_name` |
| 类型化读写（自动算大小） | `read::<T>` / `write::<T>` |
| 字节块读写 | `read_bytes` / `write_bytes` |
| 数组 / 结构体读取 | `read_array::<T>` |
| C 字符串 / UTF-16 字符串 | `read_cstring` / `read_wstring` |
| 模块枚举与基址查询 | `modules` / `module_base` / `find_module` |
| 多级指针链解析 | `resolve_pointer_chain` |
| 内存区域枚举 | `regions` / `readable_regions` / `region_at` |
| 保护属性修改（RAII 式） | `protect` / `with_writable` |
| 特征码扫描（AOB，支持半字节通配） | `scan_pattern` / `scan_pattern_in_module` |
| 数值扫描（带对齐） | `scan_value::<T>` / `scan_bytes` |

## 引入

本地 path 依赖：

```toml
[dependencies]
winmem = { path = "D:/WorkBuddyOutput/2026-10-29-19-23-19/winmem" }
```

## 快速开始

```rust
use winmem::{Pattern, Process};

fn main() -> winmem::Result<()> {
    let process = Process::by_name("notepad.exe")?;
    println!("pid = {}", process.pid());

    // 模块基址
    let base = process.module_base("notepad.exe")?;

    // 读一个 u32（类型自动推导，不用手写 4）
    let health: u32 = process.read(base + 0x1234)?;
    println!("health = {health}");

    // 写回去
    process.write(base + 0x1234, &999u32)?;

    // 多级指针：[[base+0x10]+0x20]+0x8
    if let Some(address) = process.resolve_pointer_chain(base, &[0x10, 0x20, 0x8]) {
        let value: u32 = process.read(address)?;
        println!("pointer chain -> {value}");
    }

    // 特征码扫描（? 是通配，?B 是半字节通配）
    let pattern = Pattern::parse("48 8B 05 ?? ?? ?? ?? 48 85 C0")?;
    let hits = process.scan_pattern_in_module(&pattern, "notepad.exe", 5)?;
    for hit in hits {
        println!("命中：0x{hit:X}");
    }

    // 扫数值
    let hits = process.scan_value(&15000u32, 20)?;
    println!("值为 15000 的地址有 {} 个", hits.len());

    Ok(())
}
```

## 示例

```bash
# 自我测试：用自己的进程验证读写、扫描、指针链是否正常
cargo run --example self_test

# 查看任意进程的模块和内存区域
cargo run --example inspect -- explorer.exe
cargo run --example inspect -- --pid 1234 --regions
```

`self_test` 会断言每一步的结果，跑通就说明库在本机工作正常。

## 设计说明

**类型化读写。** `read::<T>()` 内部用 `size_of::<T>()` 算长度，再 `ptr::copy_nonoverlapping` 拷进 `MaybeUninit`，不经过任何中间转换。代价是只接受 `Copy` 类型——这是故意的，避免把目标进程里的指针/所有权误搬进当前进程。

**RAII 句柄。** `Process` 持有 `HANDLE`，`Drop` 里自动 `CloseHandle`。`modules()` 用的临时快照句柄也有独立 guard，异常路径同样会释放。

**部分读容忍。** 跨页读取时 `ReadProcessMemory` 返回 `FALSE` 且错误码是 `ERROR_PARTIAL_COPY`，但实际已经读到了一部分。库在这种情况下返回**读到的部分**而不是直接报错，扫描器才不会因为一个坏页就中断整片区域。

**分块扫描。** 默认 4 MB 一块，块间保留 `模式长度 - 1` 字节重叠，跨块的特征码不会漏。去重方式是「只报告落在本块起始范围内的命中」，重叠区留给下一块，所以同一个位置不会被报两次。

**对齐扫描。** 数值扫描按元素宽度对齐（`u32` 就是 4 字节对齐），和 CE 的默认行为一致。不对齐会扫出海量跨字段的假命中。对齐基准用的是**绝对地址**而不是块内偏移，否则分块会打乱对齐关系。

**特征码用位掩码表示。** 每个位置是 `(值, 掩码)`，所以 `??`、`?B`、`4?` 都是原生支持，不需要为半字节通配写特判分支。

## 注意事项

**权限。** 目标进程权限更高时 `OpenProcess` 会失败并返回 `MemError::OpenProcess { code: 5 }`。需要以管理员身份运行，或启用 `SeDebugPrivilege`。

**扫描代价。** `scan_pattern` / `scan_value` 遍历全部可读区域，在大进程（几百 MB 到几 GB）上是秒级到十几秒级操作。能限定范围就用 `scan_pattern_in` / `scan_pattern_in_module`，快一个数量级。

**`resolve_pointer_chain` 的语义。** 它按「读指针 → 加偏移」的顺序走完所有 offset，返回最后一层的地址。中间读到 0 就返回 `None`。注意它**不会**在最后一层再解引用一次——这是 CE 里「指针路径」的语义，不是「取值」。

**别对受保护进程硬来。** 有内核级反作弊的进程（比如带 ACE、EAC、BattlEye 的）会主动检测甚至拒绝外部读写。绕过它既不稳定也不合适，这个库不打算帮这个忙。

## 许可

MIT
