//! 内存扫描：特征码扫描与数值扫描。
//!
//! 设计要点：
//! - 按可读区域遍历，不碰空闲页，避免大量无谓的失败调用
//! - 分块读取（默认 4 MB），内存占用可控，扫描 GB 级进程也不会一次吃掉几个 G
//! - 块与块之间保留 `模式长度 - 1` 字节的重叠，跨块的特征码不会漏
//! - 每个命中只报告一次（用「本块起始范围」做去重）

use std::mem;

use crate::error::Result;
use crate::pattern::Pattern;
use crate::process::Process;

/// 单次读取的块大小。4 MB 在速度和内存占用之间比较平衡。
const CHUNK_SIZE: usize = 4 * 1024 * 1024;

impl Process {
    /// 在目标进程所有可读内存中扫描特征码。
    ///
    /// `limit` 是命中数量上限，防止在大内存上扫出海量结果把内存吃满。
    pub fn scan_pattern(&self, pattern: &Pattern, limit: usize) -> Result<Vec<usize>> {
        let mut hits = Vec::new();
        if pattern.is_empty() || limit == 0 {
            return Ok(hits);
        }

        for region in self.readable_regions()? {
            let remaining = limit.saturating_sub(hits.len());
            if remaining == 0 {
                break;
            }
            let mut region_hits =
                self.scan_pattern_range(pattern, region.base, region.size, remaining)?;
            hits.append(&mut region_hits);
        }

        Ok(hits)
    }

    /// 在指定地址范围内扫描特征码。
    pub fn scan_pattern_in(
        &self,
        pattern: &Pattern,
        base: usize,
        size: usize,
        limit: usize,
    ) -> Result<Vec<usize>> {
        self.scan_pattern_range(pattern, base, size, limit)
    }

    /// 在指定模块内扫描特征码（最常用的姿势：找模块里的某个函数特征）。
    pub fn scan_pattern_in_module(
        &self,
        pattern: &Pattern,
        module_name: &str,
        limit: usize,
    ) -> Result<Vec<usize>> {
        let module = self
            .find_module(module_name)
            .ok_or_else(|| crate::error::MemError::ModuleNotFound {
                name: module_name.to_string(),
            })?;
        self.scan_pattern_range(pattern, module.base, module.size, limit)
    }

    /// 扫描一个数值（`u32`、`f32` 等），按类型宽度对齐匹配。
    ///
    /// 对齐是必要的：不对齐会扫出大量无意义的「跨字段命中」，
    /// 而 CE 默认也是 4 字节对齐。
    pub fn scan_value<T: Copy + PartialEq>(&self, value: &T, limit: usize) -> Result<Vec<usize>> {
        let size = mem::size_of::<T>();
        if size == 0 {
            return Ok(Vec::new());
        }
        let needle =
            unsafe { std::slice::from_raw_parts(value as *const T as *const u8, size) };
        self.scan_bytes(needle, size, limit)
    }

    /// 扫描任意字节序列，`align` 为 1 时表示逐字节匹配。
    pub fn scan_bytes(&self, needle: &[u8], align: usize, limit: usize) -> Result<Vec<usize>> {
        let mut hits = Vec::new();
        if needle.is_empty() || limit == 0 {
            return Ok(hits);
        }

        for region in self.readable_regions()? {
            let remaining = limit.saturating_sub(hits.len());
            if remaining == 0 {
                break;
            }
            let mut region_hits =
                self.scan_aligned_range(needle, align, region.base, region.size, remaining)?;
            hits.append(&mut region_hits);
        }

        Ok(hits)
    }

    /// 扫描字节序列并限制在指定范围。
    pub fn scan_bytes_in(
        &self,
        needle: &[u8],
        align: usize,
        base: usize,
        size: usize,
        limit: usize,
    ) -> Result<Vec<usize>> {
        self.scan_aligned_range(needle, align, base, size, limit)
    }

    /// 特征码扫描的核心实现。
    fn scan_pattern_range(
        &self,
        pattern: &Pattern,
        base: usize,
        size: usize,
        limit: usize,
    ) -> Result<Vec<usize>> {
        let mut hits = Vec::new();
        let pattern_len = pattern.len();

        if pattern_len == 0 || size < pattern_len || limit == 0 {
            return Ok(hits);
        }

        let overlap = pattern_len - 1;
        let mut offset = 0usize;

        while offset < size {
            let chunk = CHUNK_SIZE.min(size - offset);
            let address = base + offset;
            // 多读一点，让跨越块边界的特征码也能被完整看到。
            let read_len = (chunk + overlap).min(size - offset);

            if let Ok(data) = self.read_bytes(address, read_len) {
                let mut cursor = 0usize;
                while cursor + pattern_len <= data.len() {
                    match pattern.find_in(&data[cursor..]) {
                        Some(relative) => {
                            let local = cursor + relative;
                            // 只报告落在本块起始范围内的命中，重叠区交给下一块处理，
                            // 这样同一个位置不会被报两次。
                            if local < chunk {
                                hits.push(address + local);
                                if hits.len() >= limit {
                                    return Ok(hits);
                                }
                            }
                            cursor = local + 1;
                        }
                        None => break,
                    }
                }
            }

            offset += chunk.max(1);
        }

        Ok(hits)
    }

    /// 对齐字节扫描的核心实现。
    fn scan_aligned_range(
        &self,
        needle: &[u8],
        align: usize,
        base: usize,
        size: usize,
        limit: usize,
    ) -> Result<Vec<usize>> {
        let mut hits = Vec::new();
        let needle_len = needle.len();
        let align = align.max(1);

        if needle_len == 0 || size < needle_len || limit == 0 {
            return Ok(hits);
        }

        let overlap = needle_len - 1;
        let mut offset = 0usize;

        while offset < size {
            let chunk = CHUNK_SIZE.min(size - offset);
            let address = base + offset;
            let read_len = (chunk + overlap).min(size - offset);

            if let Ok(data) = self.read_bytes(address, read_len) {
                // 对齐到绝对地址，而不是块内偏移 —— 否则分块会打乱对齐关系。
                let misalign = address % align;
                let mut i = if misalign == 0 { 0 } else { align - misalign };

                while i + needle_len <= data.len() {
                    if i < chunk && &data[i..i + needle_len] == needle {
                        hits.push(address + i);
                        if hits.len() >= limit {
                            return Ok(hits);
                        }
                    }
                    i += align;
                }
            }

            offset += chunk.max(1);
        }

        Ok(hits)
    }
}
