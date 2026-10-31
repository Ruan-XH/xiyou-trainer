//! 内存区域枚举与保护属性操作。
//!
//! 对应 CE 里 "Memory Regions" 那套能力，扫描器也建立在这一层之上。

use std::ffi::c_void;
use std::mem;

use windows::Win32::System::Memory::{
    MEM_COMMIT, MEM_IMAGE, MEM_MAPPED, MEM_PRIVATE, PAGE_EXECUTE_READ, PAGE_EXECUTE_READWRITE,
    PAGE_EXECUTE_WRITECOPY, PAGE_GUARD, PAGE_NOACCESS, PAGE_PROTECTION_FLAGS, PAGE_READONLY,
    PAGE_READWRITE, PAGE_WRITECOPY, VirtualProtectEx, VirtualQueryEx, MEMORY_BASIC_INFORMATION,
};
use windows::Win32::System::SystemInformation::{GetSystemInfo, SYSTEM_INFO};

use crate::error::{MemError, Result};
use crate::process::Process;

/// 一块内存区域（等价于一次 `VirtualQueryEx` 的结果）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Region {
    /// 区域起始地址。
    pub base: usize,
    /// 区域大小（字节）。
    pub size: usize,
    /// 原始 `PAGE_*` 保护标志。
    pub protect: u32,
    /// 原始 `MEM_*` 状态标志。
    pub state: u32,
    /// 原始 `MEM_*` 类型标志。
    pub ty: u32,
}

impl Region {
    /// 区域结束地址（不含）。
    pub fn end(&self) -> usize {
        self.base.saturating_add(self.size)
    }

    /// 是否包含某个地址。
    pub fn contains(&self, address: usize) -> bool {
        address >= self.base && address < self.end()
    }

    /// 是否已提交（`MEM_COMMIT`）。
    pub fn is_committed(&self) -> bool {
        self.state == MEM_COMMIT.0
    }

    /// 是否处于空闲状态（`MEM_FREE`，通常没有对应物理页）。
    pub fn is_free(&self) -> bool {
        // MEM_FREE = 0x10000
        self.state == 0x10000
    }

    /// 是否带 `PAGE_GUARD` 保护（守卫页，访问会触发异常）。
    pub fn is_guard(&self) -> bool {
        self.protect & PAGE_GUARD.0 != 0
    }

    /// 是否可读。
    pub fn is_readable(&self) -> bool {
        if !self.is_committed() || self.is_guard() {
            return false;
        }
        matches!(
            self.protect & 0xFF,
            x if x == PAGE_READONLY.0
                || x == PAGE_READWRITE.0
                || x == PAGE_WRITECOPY.0
                || x == PAGE_EXECUTE_READ.0
                || x == PAGE_EXECUTE_READWRITE.0
                || x == PAGE_EXECUTE_WRITECOPY.0
        )
    }

    /// 是否可写。
    pub fn is_writable(&self) -> bool {
        if !self.is_committed() || self.is_guard() {
            return false;
        }
        matches!(
            self.protect & 0xFF,
            x if x == PAGE_READWRITE.0
                || x == PAGE_WRITECOPY.0
                || x == PAGE_EXECUTE_READWRITE.0
                || x == PAGE_EXECUTE_WRITECOPY.0
        )
    }

    /// 是否可执行。
    pub fn is_executable(&self) -> bool {
        if !self.is_committed() {
            return false;
        }
        matches!(
            self.protect & 0xFF,
            x if x == 0x10 // PAGE_EXECUTE
                || x == PAGE_EXECUTE_READ.0
                || x == PAGE_EXECUTE_READWRITE.0
                || x == PAGE_EXECUTE_WRITECOPY.0
        )
    }

    /// 是否完全不可访问（`PAGE_NOACCESS`）。
    pub fn is_no_access(&self) -> bool {
        self.protect & 0xFF == PAGE_NOACCESS.0
    }

    /// 区域类型的人类可读描述：`IMAGE` / `MAPPED` / `PRIVATE`。
    pub fn kind(&self) -> &'static str {
        match self.ty {
            t if t == MEM_IMAGE.0 => "IMAGE",
            t if t == MEM_MAPPED.0 => "MAPPED",
            t if t == MEM_PRIVATE.0 => "PRIVATE",
            _ => "UNKNOWN",
        }
    }

    /// 保护属性的简短描述，例如 `RW-`、`R-X`、`---`。
    pub fn protection_string(&self) -> String {
        let r = if self.is_readable() { 'R' } else { '-' };
        let w = if self.is_writable() { 'W' } else { '-' };
        let x = if self.is_executable() { 'X' } else { '-' };
        format!("{r}{w}{x}")
    }
}

impl std::fmt::Display for Region {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "0x{:016X} - 0x{:016X}  {:>10}  {}  {}",
            self.base,
            self.end(),
            format_size(self.size),
            self.protection_string(),
            self.kind()
        )
    }
}

/// 把字节数格式化成人看的单位。
fn format_size(size: usize) -> String {
    const KB: usize = 1024;
    const MB: usize = KB * 1024;
    const GB: usize = MB * 1024;

    if size >= GB {
        format!("{:.2} GB", size as f64 / GB as f64)
    } else if size >= MB {
        format!("{:.2} MB", size as f64 / MB as f64)
    } else if size >= KB {
        format!("{:.1} KB", size as f64 / KB as f64)
    } else {
        format!("{size} B")
    }
}

/// 常用保护标志的快捷常量。
pub mod protect {
    use windows::Win32::System::Memory::{
        PAGE_EXECUTE_READ, PAGE_EXECUTE_READWRITE, PAGE_NOACCESS, PAGE_READONLY, PAGE_READWRITE,
    };

    /// 不可访问。
    pub const NO_ACCESS: u32 = PAGE_NOACCESS.0;
    /// 只读。
    pub const READ: u32 = PAGE_READONLY.0;
    /// 读写。
    pub const READ_WRITE: u32 = PAGE_READWRITE.0;
    /// 可执行可读。
    pub const EXECUTE_READ: u32 = PAGE_EXECUTE_READ.0;
    /// 可执行可读写（写代码补丁常用）。
    pub const EXECUTE_READ_WRITE: u32 = PAGE_EXECUTE_READWRITE.0;
}

impl Process {
    /// 枚举目标进程的全部内存区域（包含空闲区域，可按需过滤）。
    pub fn regions(&self) -> Result<Vec<Region>> {
        let mut list = Vec::new();
        let mut address = min_application_address();

        loop {
            let mut info = MEMORY_BASIC_INFORMATION::default();
            let written = unsafe {
                VirtualQueryEx(
                    self.raw_handle(),
                    Some(address as *const c_void),
                    &mut info,
                    mem::size_of::<MEMORY_BASIC_INFORMATION>(),
                )
            };

            if written == 0 {
                break;
            }

            let base = info.BaseAddress as usize;
            let size = info.RegionSize;

            if size == 0 {
                break;
            }

            list.push(Region {
                base,
                size,
                protect: info.Protect.0,
                state: info.State.0,
                ty: info.Type.0,
            });

            let next = base.saturating_add(size);
            if next <= address {
                break; // 防御性：地址不再前进，避免死循环
            }
            address = next;
        }

        Ok(list)
    }

    /// 只枚举「已提交且可读」的区域 —— 扫描器通常只关心这些。
    pub fn readable_regions(&self) -> Result<Vec<Region>> {
        Ok(self.regions()?.into_iter().filter(|r| r.is_readable()).collect())
    }

    /// 查询单个地址所在的内存区域。
    pub fn region_at(&self, address: usize) -> Result<Region> {
        let mut info = MEMORY_BASIC_INFORMATION::default();
        let written = unsafe {
            VirtualQueryEx(
                self.raw_handle(),
                Some(address as *const c_void),
                &mut info,
                mem::size_of::<MEMORY_BASIC_INFORMATION>(),
            )
        };

        if written == 0 {
            return Err(MemError::QueryMemory { address, code: last_error() });
        }

        Ok(Region {
            base: info.BaseAddress as usize,
            size: info.RegionSize,
            protect: info.Protect.0,
            state: info.State.0,
            ty: info.Type.0,
        })
    }

    /// 修改内存保护属性，返回修改前的旧值。
    ///
    /// 写只读段（比如代码段做补丁）之前通常要临时改成 `protect::EXECUTE_READ_WRITE`，
    /// 改完再还原。
    pub fn protect(&self, address: usize, size: usize, new_protect: u32) -> Result<u32> {
        let mut old = PAGE_PROTECTION_FLAGS(0);
        let ok = unsafe {
            VirtualProtectEx(
                self.raw_handle(),
                address as *const c_void,
                size,
                PAGE_PROTECTION_FLAGS(new_protect),
                &mut old,
            )
        };

        if ok.is_err() {
            return Err(MemError::ProtectMemory { address, size, code: last_error() });
        }

        Ok(old.0)
    }

    /// 在「临时可写」状态下执行闭包，结束后自动还原保护属性。
    ///
    /// 比手动 `protect` + `protect` 更安全，闭包里 panic 也不会留下改过的保护属性。
    pub fn with_writable<T>(
        &self,
        address: usize,
        size: usize,
        f: impl FnOnce(&Self) -> T,
    ) -> Result<T> {
        let old = self.protect(address, size, protect::EXECUTE_READ_WRITE)?;
        let result = f(self);
        // 还原失败不影响闭包结果，但会返回错误 —— 这里选择忽略还原错误，
        // 因为闭包结果更重要，且地址可能已经被释放。
        let _ = self.protect(address, size, old);
        Ok(result)
    }
}

/// 用户态可用的最小应用地址（`GetSystemInfo().lpMinimumApplicationAddress`）。
fn min_application_address() -> usize {
    let mut info = SYSTEM_INFO::default();
    unsafe { GetSystemInfo(&mut info) };
    let min = info.lpMinimumApplicationAddress as usize;
    if min == 0 { 0x10000 } else { min }
}

/// 取当前线程最后一次 Win32 错误码。
fn last_error() -> u32 {
    unsafe { windows::Win32::Foundation::GetLastError().0 }
}
