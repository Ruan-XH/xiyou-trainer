//! 演示数据。设 `XIYOU_DEMO=1` 启动时使用，用来在没装模拟器的机器上
//! 预览界面、调布局。

use crate::game::config::ValueType;
use crate::game::items::RingCell;
use crate::game::snapshot::{
    Character, EmulatorInfo, EnemySlot, GameSnapshot, PlayerSlot, WatchRead, WatchValue,
};

fn slot(character: Character, hp: u16, qi: u8, ids: &[(u8, u8)], cursor: u8) -> PlayerSlot {
    let mut ring: [Option<RingCell>; 8] = Default::default();
    for (index, (id, count)) in ids.iter().enumerate().take(8) {
        ring[index] = Some(RingCell::new(*id, *count));
    }
    let total = ids.len().min(8) as u8;
    let cursor_slot = cursor.saturating_add(1);
    let current = if cursor_slot <= total {
        ring[cursor_slot as usize - 1].clone()
    } else {
        None
    };

    PlayerSlot {
        hp,
        qi,
        character: Some(character),
        active: 2,
        cursor_slot: current.as_ref().map(|_| cursor_slot),
        items: ring,
        item_total: total,
    }
}

fn enemy(hp: u16, x: u8, ty: u32) -> EnemySlot {
    EnemySlot {
        hp,
        active: 2,
        x,
        ty,
    }
}

/// 上一波留下的空槽：血量 / 坐标 / 种类都还在，只有在场标志是 0。
fn stale(hp: u16, x: u8, ty: u32) -> EnemySlot {
    EnemySlot {
        hp,
        active: 0,
        x,
        ty,
    }
}

fn watch(name: &str, ty: ValueType, text: &str) -> WatchRead {
    WatchRead {
        name: name.to_string(),
        ty,
        value: WatchValue::Ok {
            text: text.to_string(),
            hex: text.parse::<u64>().ok().map(|v| format!("0x{v:X}")),
        },
    }
}

/// 造一份演示快照。`tick` 让 BOSS 和小兵动起来，好看出血条变化。
pub fn snapshot(tick: u64) -> GameSnapshot {
    let mut players = vec![PlayerSlot::default(); 4];

    // 5 件：上排是「5 1 2」、中排是「4 · 3」，最能看出落点的不规律
    players[0] = slot(
        Character::SunWukong,
        64,
        45,
        &[(2, 3), (3, 12), (7, 1), (6, 2), (5, 1)],
        1, // 光标停在火把上
    );
    // 满环，用来检验 3×3 道具格的排布
    players[1] = slot(
        Character::ZhuBajie,
        55,
        30,
        &[
            (2, 1),
            (3, 2),
            (4, 5),
            (5, 1),
            (6, 3),
            (9, 1),
            (14, 2),
            (23, 60),
        ],
        4,
    );
    players[2] = slot(Character::ShaWujing, 30, 62, &[(2, 1)], 0);

    GameSnapshot {
        emulator: EmulatorInfo::demo(),
        pid: 0,
        ram_base: 0x0400_0000,
        time_left: (60 - (tick / 20) as u8 % 60).max(1),
        // 演示模式里关卡章固定显示 DEMO，这个值只是让快照字段不为空。
        stage: 1,
        qi_max: 96,
        players,
        enemies: vec![
            // BOSS：血和坐标都在动
            enemy(
                420u16.saturating_sub((tick % 40) as u16 * 3),
                (60 + (tick % 40)) as u8,
                0x2465B6,
            ),
            // 两只同类小兵，一只在走一只快没了
            enemy(18, (33 + (tick % 12)) as u8, 0x240044),
            enemy(7, 58, 0x240044),
            // 上场景残留：血量还在，但类型没登记、在场标志是 0
            stale(0, 77, 0x233B54),
            stale(20, 90, 0x233B54),
            stale(0, 44, 0x237166),
        ],
        watches: vec![
            watch("分数", ValueType::U32, "1234500"),
            watch("BOSS 血量", ValueType::U16, "480"),
        ],
    }
}
