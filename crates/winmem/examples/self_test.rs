//! 自测示例：对自己的进程做一次完整的读写 / 扫描 / 区域枚举。
//!
//! 用自己的进程做目标，既不需要管理员权限，也不会影响别的程序，
//! 适合用来确认库在你这台机器上工作正常。
//!
//! 运行：`cargo run --example self_test`

use winmem::{Pattern, Process};

fn main() -> winmem::Result<()> {
    let pid = std::process::id();
    let process = Process::by_pid(pid)?;

    println!("=== 附加 ===");
    println!("进程: {} (pid={})", process.name(), process.pid());

    // ---------- 1. 模块枚举 ----------
    println!("\n=== 模块 ===");
    let modules = process.modules()?;
    println!("已加载模块数: {}", modules.len());
    for module in modules.iter().take(5) {
        println!(
            "  {:<28} base=0x{:012X} size=0x{:X}",
            module.name, module.base, module.size
        );
    }

    let exe_name = process.name().to_string();
    match process.module_base(&exe_name) {
        Ok(base) => println!("\n自身模块基址: 0x{base:012X}"),
        Err(e) => println!("\n查询自身模块基址失败: {e}"),
    }

    // ---------- 2. 类型化读写 ----------
    println!("\n=== 读写 ===");

    let mut number: u32 = 0xDEAD_BEEF;
    let number_addr = &mut number as *mut u32 as usize;

    let read_back: u32 = process.read(number_addr)?;
    println!("读 u32 : 0x{read_back:08X}（原始 0x{number:08X}）");
    assert_eq!(read_back, 0xDEAD_BEEF, "读出的值应与原始值一致");

    process.write(number_addr, &0x1234_5678u32)?;
    println!("写 u32 : 写入 0x12345678 后，变量实际值 = 0x{number:08X}");
    assert_eq!(number, 0x1234_5678, "写入应真正改变了目标内存");

    let mut float_value: f32 = 1.5;
    let float_addr = &mut float_value as *mut f32 as usize;
    let float_back: f32 = process.read(float_addr)?;
    println!("读 f32 : {float_back}");
    assert_eq!(float_back, 1.5);

    // 字节块读写
    let mut buffer = [0u8; 16];
    let buffer_addr = buffer.as_mut_ptr() as usize;
    process.write_bytes(buffer_addr, &[1, 2, 3, 4, 5])?;
    println!("写字节: 写 5 字节后 buffer 前 6 位 = {:?}", &buffer[..6]);

    let bytes = process.read_bytes(buffer_addr, 5)?;
    println!("读字节: {bytes:?}");
    assert_eq!(bytes, vec![1, 2, 3, 4, 5]);

    // ---------- 3. 字符串 ----------
    println!("\n=== 字符串 ===");
    let text = b"hello winmem\0";
    let text_addr = text.as_ptr() as usize;
    println!("读 C 字符串: {:?}", process.read_cstring(text_addr, text.len())?);

    let wide: Vec<u16> = "宽字符测试\0".encode_utf16().collect();
    let wide_addr = wide.as_ptr() as usize;
    println!("读 UTF-16  : {:?}", process.read_wstring(wide_addr, wide.len())?);

    // ---------- 4. 指针链 ----------
    println!("\n=== 指针链 ===");
    let target_value: u64 = 0x1122_3344_5566_7788;
    let first_hop = &target_value as *const u64 as usize;
    let second_hop: usize = first_hop; // 一个「指针的指针」

    // [[second_hop] + 0] -> first_hop，再读它得到 target_value 的地址
    let resolved = process.resolve_pointer_chain(&second_hop as *const usize as usize, &[0]);
    match resolved {
        Some(address) => {
            println!("解析 [[&second_hop]+0] = 0x{address:X}（期望 0x{first_hop:X}）");
            assert_eq!(address, first_hop, "指针链应解析回 target_value 的地址");
        }
        None => panic!("指针链解析返回 None"),
    }

    // ---------- 5. 内存区域 ----------
    println!("\n=== 内存区域 ===");
    let regions = process.readable_regions()?;
    let total: usize = regions.iter().map(|r| r.size).sum();
    println!("可读区域数: {}，总大小: {:.1} MB", regions.len(), total as f64 / 1024.0 / 1024.0);
    for region in regions.iter().take(5) {
        println!("  {region}");
    }

    let self_region = process.region_at(&number as *const u32 as usize)?;
    println!("\n局部变量所在区域: base=0x{:X} size=0x{:X} protect={}", self_region.base, self_region.size, self_region.protection_string());

    // ---------- 6. 特征码扫描 ----------
    println!("\n=== 特征码扫描 ===");
    let marker: [u8; 8] = [0xDE, 0xAD, 0xBE, 0xEF, 0xCA, 0xFE, 0xBA, 0xBE];
    let marker_addr = marker.as_ptr() as usize;

    let pattern = Pattern::parse("DE AD BE EF CA FE BA BE")?;
    let hits = process.scan_pattern_in(&pattern, marker_addr, marker.len(), 10)?;
    println!("精确特征码命中: {} 个 -> {hits:?}", hits.len());
    assert_eq!(hits.len(), 1, "应恰好命中 1 处");
    assert_eq!(hits[0], marker_addr);

    // 带通配符的版本
    let wildcard = Pattern::parse("DE AD ?? EF ?A FE BA BE")?;
    let wild_hits = process.scan_pattern_in(&wildcard, marker_addr, marker.len(), 10)?;
    println!("通配特征码命中: {} 个 -> {wild_hits:?}", wild_hits.len());
    assert_eq!(wild_hits.len(), 1);

    // 在自身 exe 模块里找 MZ 头（真实场景：在模块内定位特征）
    if let Ok(mz) = process.scan_pattern_in_module(&Pattern::parse("4D 5A")?, &exe_name, 3) {
        println!("在 {exe_name} 模块内找到 MZ 头: {mz:?}");
    }

    // ---------- 7. 数值扫描 ----------
    println!("\n=== 数值扫描 ===");
    let lucky = 0x5A5A_1234u32;
    let lucky_addr = &lucky as *const u32 as usize;
    let found = process.scan_bytes_in(
        &lucky.to_le_bytes(),
        4,
        lucky_addr.saturating_sub(0x1000),
        0x2000,
        10,
    )?;
    println!("在局部范围扫 0x{lucky:X} 命中 {} 处: {found:?}", found.len());
    assert!(found.contains(&lucky_addr), "应扫到变量自身地址");

    println!("\n全部检查通过 ✓");
    Ok(())
}
