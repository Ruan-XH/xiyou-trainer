//! 统一错误类型。
//!
//! 所有 API 都返回 [`Result`]，错误里尽量带上地址、大小、Windows 错误码，
//! 方便直接定位问题。

use std::fmt;

/// 本库统一的结果类型。
pub type Result<T> = std::result::Result<T, MemError>;

/// 本库可能产生的所有错误。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MemError {
    /// 打开进程失败（通常是权限不足，需要管理员权限或目标以更高权限运行）。
    OpenProcess {
        /// 目标进程 ID。
        pid: u32,
        /// `GetLastError()` 返回的错误码。
        code: u32,
    },

    /// 按名称查找进程失败。
    ProcessNotFound {
        /// 查找的名称。
        name: String,
    },

    /// 读取内存失败。
    ReadMemory {
        /// 起始地址。
        address: usize,
        /// 请求读取的字节数。
        size: usize,
        /// `GetLastError()` 返回的错误码。
        code: u32,
    },

    /// 写入内存失败。
    WriteMemory {
        /// 起始地址。
        address: usize,
        /// 请求写入的字节数。
        size: usize,
        /// `GetLastError()` 返回的错误码。
        code: u32,
    },

    /// 查询内存区域信息失败。
    QueryMemory {
        /// 查询的地址。
        address: usize,
        /// `GetLastError()` 返回的错误码。
        code: u32,
    },

    /// 修改内存保护属性失败。
    ProtectMemory {
        /// 地址。
        address: usize,
        /// 大小。
        size: usize,
        /// `GetLastError()` 返回的错误码。
        code: u32,
    },

    /// 枚举模块失败。
    EnumModules {
        /// `GetLastError()` 返回的错误码。
        code: u32,
    },

    /// 在模块列表里找不到指定模块。
    ModuleNotFound {
        /// 模块名（不含路径）。
        name: String,
    },

    /// 读到的字节数少于请求长度（跨页被截断，或目标内存不可读）。
    ShortRead {
        /// 起始地址。
        address: usize,
        /// 期望读取的字节数。
        expected: usize,
        /// 实际读到的字节数。
        got: usize,
    },

    /// 特征码（AOB pattern）格式非法。
    InvalidPattern {
        /// 出错的原始字符串。
        pattern: String,
        /// 具体原因。
        reason: String,
    },
}

impl fmt::Display for MemError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MemError::OpenProcess { pid, code } => {
                write!(f, "打开进程 {pid} 失败（GetLastError={code}），可能需要管理员权限")
            }
            MemError::ProcessNotFound { name } => write!(f, "找不到进程：{name}"),
            MemError::ReadMemory { address, size, code } => {
                write!(f, "读取内存失败：地址 0x{address:X}，大小 {size} 字节（GetLastError={code}）")
            }
            MemError::WriteMemory { address, size, code } => {
                write!(f, "写入内存失败：地址 0x{address:X}，大小 {size} 字节（GetLastError={code}）")
            }
            MemError::QueryMemory { address, code } => {
                write!(f, "查询内存区域失败：地址 0x{address:X}（GetLastError={code}）")
            }
            MemError::ProtectMemory { address, size, code } => {
                write!(f, "修改内存保护失败：地址 0x{address:X}，大小 {size}（GetLastError={code}）")
            }
            MemError::EnumModules { code } => {
                write!(f, "枚举模块失败（GetLastError={code}）")
            }
            MemError::ModuleNotFound { name } => write!(f, "找不到模块：{name}"),
            MemError::ShortRead { address, expected, got } => write!(
                f,
                "读取长度不足：地址 0x{address:X} 期望 {expected} 字节，实际只读到 {got} 字节（可能跨出可读区域）"
            ),
            MemError::InvalidPattern { pattern, reason } => {
                write!(f, "特征码格式非法「{pattern}」：{reason}")
            }
        }
    }
}

impl std::error::Error for MemError {}
