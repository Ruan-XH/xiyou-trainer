//! 西游释厄传内存看板。
//!
//! 界面是 egui 画的：250×650 的小竖条，敌兵六行、玩家四行，纯色块 + 文字。
//! 数据层在 `game/` 里，和界面完全解耦；地址全在 `config/offsets.yaml`。

// 发布版是图形子系统，双击 exe 不会再弹一个黑框跟着。
// debug 版保留控制台，方便看字体加载和配置报错的日志。
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod game;

use app::Trainer;

/// 把父进程（cmd / PowerShell）的控制台接回来。
///
/// 图形子系统的进程默认不带控制台，`println!` 会静默丢掉。`--dump-offsets`
/// 这些命令行用法需要能看到输出，所以带参数启动时先 Attach 一次。
/// 没有父控制台（双击、从资源管理器启动）时调用会失败，忽略即可。
#[cfg(windows)]
fn attach_parent_console() {
    const ATTACH_PARENT_PROCESS: u32 = u32::MAX;

    unsafe extern "system" {
        fn AttachConsole(process_id: u32) -> i32;
    }

    // SAFETY: 只是把当前进程挂到父进程的控制台上，没有指针进来。
    unsafe {
        AttachConsole(ATTACH_PARENT_PROCESS);
    }
}

#[cfg(not(windows))]
fn attach_parent_console() {}

/// 弹一个系统错误框。
///
/// 没有控制台以后，`eprintln!` 是静默丢弃的，界面起不来必须让人看得见。
#[cfg(windows)]
fn alert(text: &str) {
    unsafe extern "system" {
        fn MessageBoxW(
            hwnd: *mut core::ffi::c_void,
            text: *const u16,
            caption: *const u16,
            flags: u32,
        ) -> i32;
    }

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    const MB_ICONERROR: u32 = 0x0000_0010;

    let body = wide(text);
    let caption = wide("西游 · 内存看板");
    // SAFETY: 两个指针指向的 Vec 在调用期间都还活着，且以 0 结尾。
    unsafe {
        MessageBoxW(std::ptr::null_mut(), body.as_ptr(), caption.as_ptr(), MB_ICONERROR);
    }
}

#[cfg(not(windows))]
fn alert(_text: &str) {}

/// 窗口 / 任务栏图标。
///
/// 素材由 `tools/make_icon.py` 从 `assets/logo_src.png` 生成，这里是 64×64 的
/// 裸 RGBA（不是 PNG），编译期直接进代码段，运行时不用解码、也不用带图片文件。
/// 换图标：重新跑一次脚本，`cargo build` 就会带上新的。
fn window_icon() -> eframe::egui::IconData {
    eframe::egui::IconData {
        rgba: include_bytes!("../assets/icon64.rgba").to_vec(),
        width: 64,
        height: 64,
    }
}

fn main() {
    // 带参数 = 命令行用法，先把控制台接回来，否则输出会被丢掉。
    if std::env::args_os().len() > 1 {
        attach_parent_console();
    }

    if let Some(code) = cli() {
        std::process::exit(code);
    }

    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([250.0, 650.0])
            .with_min_inner_size([250.0, 650.0])
            .with_max_inner_size([250.0, 650.0])
            .with_resizable(false)
            .with_maximize_button(false)
            .with_icon(window_icon())
            .with_title("西游 · V126"),
        ..Default::default()
    };

    if let Err(err) = eframe::run_native(
        "xiyou-trainer",
        options,
        Box::new(|cc| Ok(Box::new(Trainer::new(cc)))),
    ) {
        eprintln!("界面启动失败：{err}");
        alert(&format!("界面启动失败：\n\n{err}"));
        std::process::exit(1);
    }
}

/// 命令行入口。返回 `Some(退出码)` 表示不进界面。
fn cli() -> Option<i32> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let first = args.first()?;

    match first.as_str() {
        "--dump-offsets" | "-d" => {
            let loaded = game::config::load();
            println!("{}", loaded.describe_source());
            let ram = args
                .get(1)
                .and_then(|text| parse_hex_arg(text))
                .unwrap_or(0x0400_0000);
            println!();
            print!("{}", loaded.config.describe(ram, 0x0040_0000));
            if let Some(err) = &loaded.error {
                eprintln!("\n配置有误：{err}");
                Some(1)
            } else {
                Some(0)
            }
        }
        "--addresses" | "-a" => {
            let loaded = game::config::load();
            println!("{}", loaded.describe_source());
            let live = match game::addresses::locate(&loaded.config) {
                Ok(live) => live,
                Err(err) => {
                    eprintln!("\n{err}");
                    return Some(1);
                }
            };
            let text = game::addresses::yaml(&loaded.config, &live);

            if args.get(1).map(String::as_str) == Some("-") {
                print!("{text}");
                return Some(0);
            }

            let out = args
                .get(1)
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|| std::path::PathBuf::from("config/addresses.yaml"));
            if let Some(dir) = out.parent().filter(|dir| !dir.as_os_str().is_empty()) {
                let _ = std::fs::create_dir_all(dir);
            }
            match std::fs::write(&out, &text) {
                Ok(()) => {
                    println!("\n已写入 {}（{} 行）", out.display(), text.lines().count());
                    Some(0)
                }
                Err(err) => {
                    eprintln!("\n写 {} 失败：{err}", out.display());
                    Some(1)
                }
            }
        }
        "--help" | "-h" => {
            print_help();
            Some(0)
        }
        other => {
            eprintln!("不认识的参数：{other}\n");
            print_help();
            Some(2)
        }
    }
}

fn print_help() {
    println!(
        "西游释厄传 · 内存看板

用法：
  xiyou-trainer                     直接启动界面
  xiyou-trainer --dump-offsets      打印全部地址（校验 config/offsets.yaml）
  xiyou-trainer --dump-offsets 0x04000000   指定一个假想的 RAM 基址来预览
  xiyou-trainer --addresses         连上在跑的模拟器，算出绝对地址写入 config/addresses.yaml
  xiyou-trainer --addresses -       同上，但打到标准输出

环境变量：
  XIYOU_OFFSETS   指定 offsets.yaml 的路径
  XIYOU_DEMO=1    演示模式，不读内存，用假数据渲染界面"
    );
}

fn parse_hex_arg(text: &str) -> Option<usize> {
    let s = text.trim();
    let (digits, radix) = match s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        Some(rest) => (rest, 16),
        None => (s, 10),
    };
    usize::from_str_radix(digits, radix).ok()
}
