//! # winmem
//!
//! 轻量级 Windows 跨进程内存读写库，纯 Rust 实现（基于微软官方 `windows` crate），
//! 不依赖任何 C 库，`cargo add` 即可使用。
//!
//! ## 快速开始
//!
//! ```no_run
//! use winmem::Process;
//!
//! # fn main() -> winmem::Result<()> {
//! // 按进程名附加（也可以用 Process::by_pid）
//! let process = Process::by_name("notepad.exe")?;
//!
//! // 拿模块基址
//! let base = process.module_base("notepad.exe")?;
//!
//! // 读一个 u32 —— 类型自动推导，不用手动算大小
//! let value: u32 = process.read(base + 0x1000)?;
//! println!("value = {value}");
//!
//! // 写回去
//! process.write(base + 0x1000, &12345u32)?;
//! # Ok(())
//! # }
//! ```
//!
//! ## 设计要点
//!
//! - **类型安全**：`read::<T>()` / `write::<T>()` 自动算大小，避免手写字节数
//! - **RAII**：`Process` 持有句柄，Drop 时自动 `CloseHandle`，不会泄漏
//! - **错误可诊断**：错误里带地址、大小、Windows 错误码
//! - **部分读容忍**：跨页读取被截断时返回实际读到的字节，而不是整体失败
//!
//! ## 权限说明
//!
//! 目标进程权限比当前进程高时（例如受保护进程、管理员进程），`OpenProcess` 会失败并返回
//! [`MemError::OpenProcess`]。此时需要以管理员身份运行。

#![cfg(windows)]

pub mod error;
pub mod pattern;
pub mod process;
pub mod region;
pub mod scan;

pub use error::{MemError, Result};
pub use pattern::Pattern;
pub use process::{Module, Process};
pub use region::{protect, Region};
