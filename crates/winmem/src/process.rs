//! 进程句柄与模块信息。

use std::ffi::c_void;
use std::mem::{self, MaybeUninit};
use std::ptr;

use windows::Win32::Foundation::{CloseHandle, GetLastError, HANDLE};
use windows::Win32::System::Diagnostics::Debug::{ReadProcessMemory, WriteProcessMemory};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Module32FirstW, Module32NextW, Process32FirstW, Process32NextW,
    MODULEENTRY32W, PROCESSENTRY32W, TH32CS_SNAPMODULE, TH32CS_SNAPMODULE32, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::Threading::{
    OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_VM_OPERATION, PROCESS_VM_READ, PROCESS_VM_WRITE,
};

use crate::error::{MemError, Result};

/// 一个已加载的模块（可执行文件或 DLL）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Module {
    /// 模块名，例如 `notepad.exe`、`kernel32.dll`。
    pub name: String,
    /// 完整磁盘路径。
    pub path: String,
    /// 模块在目标进程中的加载基址。
    pub base: usize,
    /// 模块大小（字节）。
    pub size: usize,
}

impl Module {
    /// 模块的末尾地址（不含）。
    pub fn end(&self) -> usize {
        self.base.saturating_add(self.size)
    }

    /// 判断某个地址是否落在该模块范围内。
    pub fn contains(&self, address: usize) -> bool {
        address >= self.base && address < self.end()
    }

    /// 把绝对地址换算成 `模块基址 + 偏移` 形式的偏移量。
    ///
    /// 地址不在模块内时返回 `None`。
    pub fn offset_of(&self, address: usize) -> Option<usize> {
        if self.contains(address) {
            Some(address - self.base)
        } else {
            None
        }
    }
}

/// 目标进程句柄。
///
/// 通过 [`Process::by_pid`] 或 [`Process::by_name`] 创建；`Drop` 时自动关闭句柄。
pub struct Process {
    handle: HANDLE,
    pid: u32,
    name: String,
}

// 句柄本身可以在线程间传递（Windows 句柄是进程级资源，不是线程私有的）。
unsafe impl Send for Process {}
unsafe impl Sync for Process {}

impl std::fmt::Debug for Process {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Process")
            .field("pid", &self.pid)
            .field("name", &self.name)
            .field("handle", &self.handle.0)
            .finish()
    }
}

impl Process {
    /// 按进程 ID 附加。
    ///
    /// 需要的权限：`PROCESS_VM_READ | PROCESS_VM_WRITE | PROCESS_VM_OPERATION | PROCESS_QUERY_INFORMATION`。
    pub fn by_pid(pid: u32) -> Result<Self> {
        let access = PROCESS_VM_READ | PROCESS_VM_WRITE | PROCESS_VM_OPERATION | PROCESS_QUERY_INFORMATION;
        let handle = unsafe { OpenProcess(access, false, pid) }
            .map_err(|_| MemError::OpenProcess { pid, code: last_error() })?;

        let name = process_name_of(pid).unwrap_or_else(|| format!("<pid {pid}>"));
        Ok(Self { handle, pid, name })
    }

    /// 按进程名附加（不含路径，大小写不敏感）。
    ///
    /// 有多个同名进程时取第一个。
    pub fn by_name(name: &str) -> Result<Self> {
        let pid = find_pid_by_name(name)
            .ok_or_else(|| MemError::ProcessNotFound { name: name.to_string() })?;
        Self::by_pid(pid)
    }

    /// 目标进程 ID。
    pub fn pid(&self) -> u32 {
        self.pid
    }

    /// 目标进程名（附加时解析到的名字）。
    pub fn name(&self) -> &str {
        &self.name
    }

    /// 原始 `HANDLE` 值，需要直接调用 Win32 API 时用。
    pub fn raw_handle(&self) -> HANDLE {
        self.handle
    }

    /// 读取 `size` 个字节。
    ///
    /// 如果跨越了不可读的页面，返回**实际读到**的部分数据（可能短于 `size`），
    /// 而不是整体失败。完全读不到时才返回错误。
    pub fn read_bytes(&self, address: usize, size: usize) -> Result<Vec<u8>> {
        if size == 0 {
            return Ok(Vec::new());
        }

        let mut buffer = vec![0u8; size];
        let mut bytes_read = 0usize;

        let result = unsafe {
            ReadProcessMemory(
                self.handle,
                address as *const c_void,
                buffer.as_mut_ptr() as *mut c_void,
                size,
                Some(&mut bytes_read),
            )
        };

        match result {
            Ok(()) => {
                buffer.truncate(bytes_read);
                Ok(buffer)
            }
            Err(_) => {
                // ERROR_PARTIAL_COPY：跨页读取，能读到多少算多少。
                if bytes_read > 0 {
                    buffer.truncate(bytes_read);
                    Ok(buffer)
                } else {
                    Err(MemError::ReadMemory { address, size, code: last_error() })
                }
            }
        }
    }

    /// 读取一个 `T` 类型的值。
    ///
    /// 只接受 `Copy` 类型，避免把目标内存里的指针/所有权语义误搬进当前进程。
    pub fn read<T: Copy>(&self, address: usize) -> Result<T> {
        let size = mem::size_of::<T>();
        let bytes = self.read_bytes(address, size)?;

        if bytes.len() < size {
            return Err(MemError::ShortRead { address, expected: size, got: bytes.len() });
        }

        let mut value = MaybeUninit::<T>::uninit();
        unsafe {
            ptr::copy_nonoverlapping(bytes.as_ptr(), value.as_mut_ptr() as *mut u8, size);
            Ok(value.assume_init())
        }
    }

    /// 按指定长度读取并解释为 `T`（用于数组、结构体等变长场景）。
    pub fn read_array<T: Copy>(&self, address: usize, count: usize) -> Result<Vec<T>> {
        let element = mem::size_of::<T>();
        let total = element.checked_mul(count).ok_or(MemError::ShortRead {
            address,
            expected: usize::MAX,
            got: 0,
        })?;

        let bytes = self.read_bytes(address, total)?;
        let complete = bytes.len() / element.max(1);

        let mut out = Vec::with_capacity(complete);
        for chunk in bytes.chunks_exact(element.max(1)).take(complete) {
            let mut value = MaybeUninit::<T>::uninit();
            unsafe {
                ptr::copy_nonoverlapping(chunk.as_ptr(), value.as_mut_ptr() as *mut u8, element);
                out.push(value.assume_init());
            }
        }
        Ok(out)
    }

    /// 写入 `size` 个字节。要求全部写入成功。
    pub fn write_bytes(&self, address: usize, data: &[u8]) -> Result<()> {
        if data.is_empty() {
            return Ok(());
        }

        let mut bytes_written = 0usize;
        let result = unsafe {
            WriteProcessMemory(
                self.handle,
                address as *const c_void,
                data.as_ptr() as *const c_void,
                data.len(),
                Some(&mut bytes_written),
            )
        };

        if result.is_err() || bytes_written != data.len() {
            return Err(MemError::WriteMemory { address, size: data.len(), code: last_error() });
        }
        Ok(())
    }

    /// 写入一个 `T` 类型的值。
    pub fn write<T: Copy>(&self, address: usize, value: &T) -> Result<()> {
        let size = mem::size_of::<T>();
        let bytes = unsafe { std::slice::from_raw_parts(value as *const T as *const u8, size) };
        self.write_bytes(address, bytes)
    }

    /// 读取以 NUL 结尾的 ANSI 字符串，最多读 `max_len` 字节。
    pub fn read_cstring(&self, address: usize, max_len: usize) -> Result<String> {
        let bytes = self.read_bytes(address, max_len)?;
        let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
        Ok(String::from_utf8_lossy(&bytes[..end]).into_owned())
    }

    /// 读取以 NUL 结尾的 UTF-16 字符串，最多读 `max_len` 个 UTF-16 码元。
    pub fn read_wstring(&self, address: usize, max_len: usize) -> Result<String> {
        let units = self.read_array::<u16>(address, max_len)?;
        let len = units.iter().position(|&u| u == 0).unwrap_or(units.len());
        Ok(String::from_utf16_lossy(&units[..len]))
    }

    /// 枚举目标进程已加载的全部模块。
    pub fn modules(&self) -> Result<Vec<Module>> {
        let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPMODULE | TH32CS_SNAPMODULE32, self.pid) }
            .map_err(|_| MemError::EnumModules { code: last_error() })?;

        let _guard = HandleGuard(snapshot);
        let mut list = Vec::new();
        let mut entry = MODULEENTRY32W {
            dwSize: mem::size_of::<MODULEENTRY32W>() as u32,
            ..Default::default()
        };

        if unsafe { Module32FirstW(snapshot, &mut entry) }.is_err() {
            return Ok(list);
        }

        loop {
            list.push(Module {
                name: wide_to_string(&entry.szModule),
                path: wide_to_string(&entry.szExePath),
                base: entry.modBaseAddr as usize,
                size: entry.modBaseSize as usize,
            });

            if unsafe { Module32NextW(snapshot, &mut entry) }.is_err() {
                break;
            }
        }

        Ok(list)
    }

    /// 查询指定模块的基址（大小写不敏感，可省略 `.dll` / `.exe` 后缀）。
    pub fn module_base(&self, name: &str) -> Result<usize> {
        self.find_module(name)
            .map(|m| m.base)
            .ok_or_else(|| MemError::ModuleNotFound { name: name.to_string() })
    }

    /// 查询指定模块的完整信息。
    pub fn find_module(&self, name: &str) -> Option<Module> {
        let wanted = name.trim();
        let wanted_lower = wanted.to_ascii_lowercase();

        self.modules().ok()?.into_iter().find(|m| {
            let actual = m.name.to_ascii_lowercase();
            actual == wanted_lower
                || actual.trim_end_matches(".dll") == wanted_lower
                || actual.trim_end_matches(".exe") == wanted_lower
        })
    }

    /// 解析多级指针链：`base -> [base+off0] -> ... -> 最终地址`。
    ///
    /// 例如 `resolve_pointer_chain(base, &[0x10, 0x20, 0x8])` 等价于
    /// `[[[base+0x10]+0x20]+0x8]`。
    ///
    /// 中间任一层读到 0（空指针）时返回 `None`。
    pub fn resolve_pointer_chain(&self, base: usize, offsets: &[usize]) -> Option<usize> {
        let mut current = base;
        for (index, offset) in offsets.iter().enumerate() {
            let pointer: usize = self.read(current).ok()?;
            if pointer == 0 {
                return None;
            }
            current = pointer.checked_add(*offset)?;

            // 最后一层是取值，不是解引用。
            if index == offsets.len() - 1 && offsets.len() > 1 {
                // 已按常规语义处理：循环结束时 current 就是目标地址。
            }
        }
        Some(current)
    }
}

impl Drop for Process {
    fn drop(&mut self) {
        if !self.handle.is_invalid() {
            unsafe {
                let _ = CloseHandle(self.handle);
            }
        }
    }
}

/// 保证临时的内核句柄在作用域结束时一定被关闭。
struct HandleGuard(HANDLE);

impl Drop for HandleGuard {
    fn drop(&mut self) {
        if !self.0.is_invalid() {
            unsafe {
                let _ = CloseHandle(self.0);
            }
        }
    }
}

/// 取当前线程最后一次 Win32 错误码。
fn last_error() -> u32 {
    unsafe { GetLastError().0 }
}

/// 把定长 UTF-16 缓冲区转成 `String`（遇到 NUL 截断）。
fn wide_to_string(buffer: &[u16]) -> String {
    let len = buffer.iter().position(|&c| c == 0).unwrap_or(buffer.len());
    String::from_utf16_lossy(&buffer[..len])
}

/// 遍历进程快照，按名字查 PID。
fn find_pid_by_name(name: &str) -> Option<u32> {
    let wanted = name.to_ascii_lowercase();

    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) }.ok()?;
    let _guard = HandleGuard(snapshot);

    let mut entry = PROCESSENTRY32W {
        dwSize: mem::size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };

    if unsafe { Process32FirstW(snapshot, &mut entry) }.is_err() {
        return None;
    }

    loop {
        if wide_to_string(&entry.szExeFile).to_ascii_lowercase() == wanted {
            return Some(entry.th32ProcessID);
        }
        if unsafe { Process32NextW(snapshot, &mut entry) }.is_err() {
            break;
        }
    }

    None
}

/// 由 PID 反查进程名。
fn process_name_of(pid: u32) -> Option<String> {
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) }.ok()?;
    let _guard = HandleGuard(snapshot);

    let mut entry = PROCESSENTRY32W {
        dwSize: mem::size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };

    if unsafe { Process32FirstW(snapshot, &mut entry) }.is_err() {
        return None;
    }

    loop {
        if entry.th32ProcessID == pid {
            return Some(wide_to_string(&entry.szExeFile));
        }
        if unsafe { Process32NextW(snapshot, &mut entry) }.is_err() {
            break;
        }
    }

    None
}
