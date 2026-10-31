//! 特征码（Array of Bytes / AOB / signature）解析与匹配。
//!
//! 支持 CE / x64dbg 的全部常见写法，包括**半字节通配**：
//!
//! ```
//! # use winmem::pattern::Pattern;
//! // 空格分隔
//! Pattern::parse("48 8B 05 ?? ?? ?? ?? 48 85 C0").unwrap();
//! // 紧凑写法
//! Pattern::parse("488B05????????4885C0").unwrap();
//! // 用 * 表示整字节通配
//! Pattern::parse("48 8B * * 0D").unwrap();
//! // 半字节通配：只固定高半字节 / 只固定低半字节
//! Pattern::parse("48 ?B 0? 0D").unwrap();
//! ```
//!
//! 内部用 `(值, 掩码)` 表示每个位置，因此 `?B` 和 `4?` 这类写法是原生支持的，
//! 不需要额外特判。

use crate::error::{MemError, Result};

/// 单字节匹配规则：`(期望值, 有效位掩码)`。
///
/// 匹配条件为 `(实际字节 & mask) == (value & mask)`。
/// - `(0x48, 0xFF)`：整字节精确匹配 `0x48`
/// - `(0x0B, 0x0F)`：低半字节匹配 `B`
/// - `(0x40, 0xF0)`：高半字节匹配 `4`
/// - `(0x00, 0x00)`：整字节通配
type Token = (u8, u8);

/// 一条已解析的特征码。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pattern {
    tokens: Vec<Token>,
    source: String,
}

impl Pattern {
    /// 解析特征码字符串。
    ///
    /// 规则：
    /// - 十六进制字节用空格分隔（`48 8B 05`）或连写（`488B05`）
    /// - `?`、`??`、`*` 表示整字节通配
    /// - `?B`、`4?` 这类表示半字节通配
    pub fn parse(input: &str) -> Result<Self> {
        let invalid = |reason: &str| MemError::InvalidPattern {
            pattern: input.to_string(),
            reason: reason.to_string(),
        };

        let trimmed = input.trim();
        if trimmed.is_empty() {
            return Err(invalid("特征码为空"));
        }

        // 先按空白切分；若只有一段且长度 > 2，则视为连写形式，按 2 字符拆分。
        let mut pieces: Vec<String> =
            trimmed.split_whitespace().map(|s| s.to_string()).collect();

        if pieces.len() == 1 && pieces[0].chars().count() > 2 {
            let compact: Vec<char> = pieces.remove(0).chars().collect();
            if !compact.len().is_multiple_of(2) {
                return Err(invalid("连写模式下十六进制位数必须为偶数"));
            }
            pieces = compact
                .chunks(2)
                .map(|c| c.iter().collect::<String>())
                .collect();
        }

        let mut tokens = Vec::with_capacity(pieces.len());
        for piece in &pieces {
            let token = parse_token(piece)
                .ok_or_else(|| invalid(&format!("「{piece}」不是合法的字节（应为两位十六进制或含 ? 通配）")))?;
            tokens.push(token);
        }

        if tokens.is_empty() {
            return Err(invalid("特征码为空"));
        }

        Ok(Self { tokens, source: trimmed.to_string() })
    }

    /// 特征码长度（含通配位）。
    pub fn len(&self) -> usize {
        self.tokens.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.tokens.is_empty()
    }

    /// 完全通配（没有任何确定位）的数量。
    pub fn wildcard_count(&self) -> usize {
        self.tokens.iter().filter(|(_, mask)| *mask == 0).count()
    }

    /// 完全固定的字节数量。
    pub fn fixed_count(&self) -> usize {
        self.tokens.iter().filter(|(_, mask)| *mask == 0xFF).count()
    }

    /// 原始字符串（去掉首尾空白）。
    pub fn source(&self) -> &str {
        &self.source
    }

    /// 在 `haystack` 中查找**第一个**匹配位置。
    pub fn find_in(&self, haystack: &[u8]) -> Option<usize> {
        let n = self.tokens.len();
        if n == 0 || haystack.len() < n {
            return None;
        }

        // 优先用完全确定的字节做锚点，跳过速度更快。
        let anchor = self
            .tokens
            .iter()
            .position(|(_, mask)| *mask == 0xFF)
            .or_else(|| self.tokens.iter().position(|(_, mask)| *mask != 0));

        match anchor {
            Some(index) => {
                let (expected, mask) = self.tokens[index];
                let mut i = 0usize;
                while i + n <= haystack.len() {
                    if haystack[i + index] & mask != expected & mask {
                        i += 1;
                        continue;
                    }
                    if self.matches_at(haystack, i) {
                        return Some(i);
                    }
                    i += 1;
                }
                None
            }
            // 全是通配：永远匹配开头。
            None => Some(0),
        }
    }

    /// 查找所有匹配位置（最多 `limit` 个）。
    pub fn find_all_in(&self, haystack: &[u8], limit: usize) -> Vec<usize> {
        let mut hits = Vec::new();
        let mut cursor = 0usize;

        while cursor <= haystack.len() {
            match self.find_in(&haystack[cursor..]) {
                Some(relative) => {
                    let absolute = cursor + relative;
                    hits.push(absolute);
                    if hits.len() >= limit {
                        break;
                    }
                    cursor = absolute + 1;
                }
                None => break,
            }
        }

        hits
    }

    /// 精确比对某个位置是否匹配。
    fn matches_at(&self, haystack: &[u8], position: usize) -> bool {
        if position + self.tokens.len() > haystack.len() {
            return false;
        }
        self.tokens
            .iter()
            .enumerate()
            .all(|(offset, (value, mask))| haystack[position + offset] & mask == value & mask)
    }
}

impl std::fmt::Display for Pattern {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.source)
    }
}

/// 把单个 token（1~2 个字符）解析成 `(值, 掩码)`。
fn parse_token(piece: &str) -> Option<Token> {
    // 单个 ? 或 * 表示整字节通配，必须在这里拦掉：
    // 否则会被下面「单字符视为低半字节」的规则误解成 0?（高半字节固定）。
    if matches!(piece, "?" | "*" | "??" | "**") {
        return Some((0, 0));
    }

    let chars: Vec<char> = piece.chars().collect();

    // 单字符视为低半字节（"4" == "04"）。
    let padded: Vec<char> = match chars.len() {
        0 => return None,
        1 => vec!['0', chars[0]],
        2 => chars,
        _ => return None,
    };

    let mut value = 0u8;
    let mut mask = 0u8;

    for (index, ch) in padded.iter().enumerate() {
        let shift = if index == 0 { 4 } else { 0 };
        match ch {
            '?' | '*' => {} // 通配半字节：不动 mask
            other => {
                let digit = other.to_digit(16)? as u8;
                value |= digit << shift;
                mask |= 0x0F << shift;
            }
        }
    }

    Some((value, mask))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 解析空格分隔() {
        let p = Pattern::parse("48 8B 05").unwrap();
        assert_eq!(p.len(), 3);
        assert_eq!(p.wildcard_count(), 0);
        assert_eq!(p.fixed_count(), 3);
        assert_eq!(p.tokens[0], (0x48, 0xFF));
        assert_eq!(p.tokens[2], (0x05, 0xFF));
    }

    #[test]
    fn 解析连写() {
        let p = Pattern::parse("488B05").unwrap();
        assert_eq!(p.len(), 3);
        assert_eq!(p.tokens[2], (0x05, 0xFF));
    }

    #[test]
    fn 整字节通配() {
        for text in ["48 ?? 05", "48 * 05", "48 ? 05"] {
            let p = Pattern::parse(text).unwrap();
            assert_eq!(p.len(), 3, "{text}");
            assert_eq!(p.tokens[1], (0x00, 0x00), "{text}");
        }
    }

    #[test]
    fn 半字节通配() {
        // ?B：高半字节任意，低半字节固定为 B
        let p = Pattern::parse("?B").unwrap();
        assert_eq!(p.tokens[0], (0x0B, 0x0F));

        // 0?：高半字节固定 0，低半字节任意
        let p = Pattern::parse("0?").unwrap();
        assert_eq!(p.tokens[0], (0x00, 0xF0));

        // 连写模式下同样生效
        let p = Pattern::parse("48?B0D").unwrap();
        assert_eq!(p.len(), 3);
        assert_eq!(p.tokens[1], (0x0B, 0x0F));
        assert_eq!(p.tokens[2], (0x0D, 0xFF));
    }

    #[test]
    fn 匹配逻辑() {
        let needle = Pattern::parse("48 8B ?? 0D").unwrap();
        assert_eq!(needle.find_in(&[0x00, 0x48, 0x8B, 0xFF, 0x0D, 0x00]), Some(1));
        assert_eq!(needle.find_in(&[0x00, 0x48, 0x8C, 0xFF, 0x0D]), None);
    }

    #[test]
    fn 半字节通配匹配() {
        let needle = Pattern::parse("?B").unwrap();
        assert_eq!(needle.find_in(&[0x11, 0x3B, 0x22]), Some(1));

        let needle = Pattern::parse("4?").unwrap();
        assert_eq!(needle.find_in(&[0x11, 0x4F, 0x22]), Some(1));
        assert_eq!(needle.find_in(&[0x11, 0x5F, 0x22]), None);
    }

    #[test]
    fn 查找全部匹配() {
        let needle = Pattern::parse("AA BB").unwrap();
        assert_eq!(
            needle.find_all_in(&[0xAA, 0xBB, 0x00, 0xAA, 0xBB, 0xAA, 0xBB], 10),
            vec![0, 3, 5]
        );
    }

    #[test]
    fn 全通配退化() {
        let p = Pattern::parse("?? ??").unwrap();
        assert_eq!(p.find_in(&[1, 2, 3]), Some(0));
        assert_eq!(p.wildcard_count(), 2);
    }

    #[test]
    fn 非法输入报错() {
        assert!(Pattern::parse("").is_err());
        assert!(Pattern::parse("ZZ").is_err());
        assert!(Pattern::parse("488B0").is_err());
        assert!(Pattern::parse("123").is_err());
    }
}
