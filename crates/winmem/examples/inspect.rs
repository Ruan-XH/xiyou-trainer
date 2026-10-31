//! 附加到任意进程，打印模块与内存区域信息。
//!
//! 用法：
//! ```text
//! cargo run --example inspect -- notepad.exe
//! cargo run --example inspect -- --pid 1234
//! cargo run --example inspect -- --pid 1234 --regions
//! ```

use std::env;
use winmem::Process;

fn main() -> winmem::Result<()> {
    let args: Vec<String> = env::args().skip(1).collect();

    if args.is_empty() {
        eprintln!("用法：");
        eprintln!("  inspect <进程名>            例如: inspect notepad.exe");
        eprintln!("  inspect --pid <PID>        例如: inspect --pid 1234");
        eprintln!("  --regions                  额外打印全部可读内存区域");
        return Ok(());
    }

    let show_regions = args.iter().any(|a| a == "--regions");

    let process = if args[0] == "--pid" {
        let pid: u32 = args
            .get(1)
            .and_then(|s| s.parse().ok())
            .unwrap_or_else(|| {
                eprintln!("--pid 后面需要跟一个数字");
                std::process::exit(1);
            });
        Process::by_pid(pid)?
    } else {
        Process::by_name(&args[0])?
    };

    println!("进程: {} (pid={})", process.name(), process.pid());

    // ---- 模块 ----
    let modules = process.modules()?;
    println!("\n模块（共 {} 个）：", modules.len());
    for module in modules.iter().take(20) {
        println!(
            "  0x{:012X} - 0x{:012X}  {:<30} {}",
            module.base,
            module.end(),
            module.name,
            module.path
        );
    }
    if modules.len() > 20 {
        println!("  ... 还有 {} 个未显示", modules.len() - 20);
    }

    // ---- 内存区域统计 ----
    let regions = process.readable_regions()?;
    let total: usize = regions.iter().map(|r| r.size).sum();

    println!(
        "\n可读区域: {} 个，合计 {:.2} MB",
        regions.len(),
        total as f64 / 1024.0 / 1024.0
    );

    let mut private_count = 0;
    let mut image_count = 0;
    let mut mapped_count = 0;
    let mut writable_total = 0usize;

    for region in &regions {
        match region.kind() {
            "PRIVATE" => private_count += 1,
            "IMAGE" => image_count += 1,
            "MAPPED" => mapped_count += 1,
            _ => {}
        }
        if region.is_writable() {
            writable_total += region.size;
        }
    }

    println!("  PRIVATE: {private_count}   IMAGE: {image_count}   MAPPED: {mapped_count}");
    println!(
        "  其中可写: {:.2} MB",
        writable_total as f64 / 1024.0 / 1024.0
    );

    if show_regions {
        println!("\n全部可读区域：");
        for region in &regions {
            println!("  {region}");
        }
    }

    Ok(())
}
