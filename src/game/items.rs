//! 道具表与道具环布局。
//!
//! 原易语言版用 `道具列表[24]`（1 基下标）做 ID → 名称映射，再用一大坨嵌套
//! `如果` 拼空格来摆位置。这里把两件事拆开：ID → 名称查表，位置 → 位置表。
//!
//! 位置的规矩不是「顺时针填满」这种能算出来的规律，而是游戏自己按总数给出的
//! 固定排布：道具数不同，同一件道具落点会变（2 件时 1 号在左中，3 件时 1 号
//! 又跑到正上）。所以照抄成一张 1..8 件的表，见 `LAYOUTS`。
//!
//! 三条约束是死的：
//!
//! * 第二排（中排）的正中永远空着；
//! * 正中一格也永远空着（八边形中间是人物）；
//! * 一行里只摆一件时算居中（4 件时就是上、下、左、右一个菱形）。

/// 道具 ID → 名称。下标 1 起有效（0 号是占位，方便直接按游戏 ID 取值）。
///
/// ID 与游戏内存里的值一一对应：1 表示空，2 起才是真道具。
pub const ITEM_NAMES: [&str; 24] = [
    "-",        // 0 占位
    "无",       // 1
    "刀",       // 2
    "火把",     // 3
    "炸弹",     // 4
    "火扇",     // 5
    "冰扇",     // 6
    "照妖镜",   // 7
    "符咒",     // 8
    "玲珑宝塔", // 9
    "回旋刀",   // 10
    "乾坤葫芦", // 11
    "无影刀",   // 12
    "突木桩",   // 13
    "寒冰掌",   // 14
    "天罡火",   // 15
    "地煞功",   // 16
    "超级???",  // 17
    "定风珠",   // 18
    "雷电戟",   // 19
    "玄冰斧",   // 20
    "火云刀",   // 21
    "释厄剑",   // 22
    "金刚咒",   // 23
];

/// 有效的道具 ID 区间（闭区间）。1 视为空，不算有效道具。
pub const ITEM_ID_RANGE: std::ops::RangeInclusive<u8> = 2..=23;

/// 道具环最多能放几件。
pub const RING_CAPACITY: u8 = 8;

/// 同上的 `usize` 版本，给数组长度用。界面是固定的 3×3 八边形槽位，
/// 所以这个值不能随配置变——配置里写成别的会在加载时报错。
pub const RING_SLOTS: usize = RING_CAPACITY as usize;

/// 按 ID 取道具名，越界返回 `None`。
pub fn item_name(id: u8) -> Option<&'static str> {
    if ITEM_ID_RANGE.contains(&id) {
        Some(ITEM_NAMES[id as usize])
    } else {
        None
    }
}

/// 道具环里的一格。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RingCell {
    /// 原始道具 ID，保留下来方便对内存值排查。
    pub id: u8,
    pub name: &'static str,
    pub count: u8,
}

impl RingCell {
    pub fn new(id: u8, count: u8) -> Self {
        Self {
            id,
            name: item_name(id).unwrap_or("无"),
            count,
        }
    }
}

/// 网格中心格（3×3 的 (1,1)），永远空着——那里是人物本身。
pub const RING_CELL_CENTER: usize = 4;

/// 3×3 网格的格子编号就是 `row * 3 + col`：
///
/// ```text
///   0  1  2
///   3  ·  5
///   6  7  8
/// ```
///
/// 每一行是 `(格子编号, 第几件道具)`，道具编号 1 基、对应 `道具列表[1..=n]`。
/// 这张表逐条抄自原易语言版的 `如果 (道具总数 ＝ n)` 分支——它没有可以算出来
/// 的规律，就是游戏自己的固定排布，所以只能硬记：
///
/// ```text
///   n=1              n=2              n=3
///   .  1  .          .  .  .          .  1  .
///   .  ·  .          1  ·  2          2  ·  3
///   .  .  .          .  .  .          .  .  .
///
///   n=4              n=5              n=6
///   .  1  .          5  1  2          5  1  2
///   3  ·  4          4  ·  3          4  ·  3
///   .  2  .          .  .  .          .  6  .
///
///   n=7              n=8
///   7  1  2          8  1  2
///   6  ·  3          7  ·  3
///   5  ·  4          6  5  4
/// ```
///
/// 空行（比如 3 / 5 件的下排）整行留空；中排正中始终留空。
const LAYOUTS: [&[(usize, u8)]; 9] = [
    // 0 件：全空
    &[],
    // 1 件：正上
    &[(1, 1)],
    // 2 件：左中 / 右中
    &[(3, 1), (5, 2)],
    // 3 件：上 / 左中 / 右中
    &[(1, 1), (3, 2), (5, 3)],
    // 4 件：上 / 下 / 左中 / 右中
    &[(1, 1), (7, 2), (3, 3), (5, 4)],
    // 5 件：上排 5 1 2，中排 4 · 3
    &[(0, 5), (1, 1), (2, 2), (3, 4), (5, 3)],
    // 6 件：再加下排正中的 6
    &[(0, 5), (1, 1), (2, 2), (3, 4), (5, 3), (7, 6)],
    // 7 件：左列补齐，下排正中空
    &[(0, 7), (1, 1), (2, 2), (3, 6), (5, 3), (6, 5), (8, 4)],
    // 8 件：满环
    &[(0, 8), (1, 1), (2, 2), (3, 7), (5, 3), (6, 6), (7, 5), (8, 4)],
];

/// 算出某个道具数量下的 9 个格子：`Some(k)` 表示这里摆第 k 件道具（1 基）。
///
/// 索引就是 `row * 3 + col`。数量超过 8 按 8 件算。
pub fn ring_slots(total: u8) -> [Option<u8>; 9] {
    let mut cells = [None; 9];
    let total = total.min(RING_CAPACITY);
    for &(cell, nth) in LAYOUTS[total as usize] {
        cells[cell] = Some(nth);
    }
    cells
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 名称表长度与游戏id对齐() {
        assert_eq!(ITEM_NAMES.len(), 24);
        assert_eq!(item_name(23), Some("金刚咒"));
        assert_eq!(item_name(2), Some("刀"));
        assert_eq!(item_name(1), None, "ID 1 是空，不该当成道具");
        assert_eq!(item_name(24), None);
        assert_eq!(item_name(0), None);
    }

    /// 各数量下的完整版面，逐行照抄易语言版的分支结果：0 表示空格子。
    const EXPECTED: [[[u8; 3]; 3]; 9] = [
        [[0, 0, 0], [0, 0, 0], [0, 0, 0]],
        [[0, 1, 0], [0, 0, 0], [0, 0, 0]],
        [[0, 0, 0], [1, 0, 2], [0, 0, 0]],
        [[0, 1, 0], [2, 0, 3], [0, 0, 0]],
        [[0, 1, 0], [3, 0, 4], [0, 2, 0]],
        [[5, 1, 2], [4, 0, 3], [0, 0, 0]],
        [[5, 1, 2], [4, 0, 3], [0, 6, 0]],
        [[7, 1, 2], [6, 0, 3], [5, 0, 4]],
        [[8, 1, 2], [7, 0, 3], [6, 5, 4]],
    ];

    #[test]
    fn 每个数量都与易语言版的排布一致() {
        for (total, rows) in EXPECTED.iter().enumerate() {
            let cells = ring_slots(total as u8);
            for row in 0..3 {
                for col in 0..3 {
                    let want = rows[row][col];
                    let got = cells[row * 3 + col].unwrap_or(0);
                    assert_eq!(
                        got, want,
                        "{total} 件时第 {} 排第 {} 格应该是 {want}，实际 {got}",
                        row + 1,
                        col + 1
                    );
                }
            }
        }
    }

    #[test]
    fn 满环八格各就各位() {
        let cells = ring_slots(8);
        assert_eq!(cells[1], Some(1), "1 号在正上方");
        assert_eq!(cells[2], Some(2), "2 号在右上");
        assert_eq!(cells[5], Some(3), "3 号在右中");
        assert_eq!(cells[8], Some(4), "4 号在右下");
        assert_eq!(cells[7], Some(5), "5 号在正下方");
        assert_eq!(cells[6], Some(6), "6 号在左下");
        assert_eq!(cells[3], Some(7), "7 号在左中");
        assert_eq!(cells[0], Some(8), "8 号在左上");
        assert_eq!(cells[RING_CELL_CENTER], None, "中心永远空着");
    }

    #[test]
    fn 中排正中和正中永远空着() {
        for total in 0u8..=8 {
            let cells = ring_slots(total);
            assert_eq!(cells[RING_CELL_CENTER], None, "{total} 件时中心不该有道具");
            assert_eq!(cells[4], None, "{total} 件时中排正中不该有道具");
        }
    }

    #[test]
    fn 下排正中只在四六八件时用() {
        // 一行里只有一件时算「居中」，所以在下排时落在正中一格。
        let want = [None, None, None, None, Some(2), None, Some(6), None, Some(5)];
        for (total, expected) in want.iter().enumerate() {
            assert_eq!(ring_slots(total as u8)[7], *expected, "{total} 件时的下排正中");
        }
    }

    #[test]
    fn 每件道具只出现在一个格子里() {
        for total in 0u8..=8 {
            let cells = ring_slots(total);
            let mut seen = [false; 9];
            for nth in cells.iter().flatten() {
                assert!(!seen[*nth as usize], "{total} 件时 {nth} 号出现了两次");
                seen[*nth as usize] = true;
            }
            for nth in 1..=total {
                assert!(seen[nth as usize], "{total} 件时 {nth} 号没摆上");
            }
        }
    }

    #[test]
    fn 超量按满环处理() {
        assert_eq!(ring_slots(9), ring_slots(8));
        assert_eq!(ring_slots(255), ring_slots(8));
    }
}
