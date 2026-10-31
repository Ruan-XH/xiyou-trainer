//! 把「相对偏移」算成「可以直接粘进 Cheat Engine 的绝对地址」，输出一份 YAML。
//!
//! 绝对地址 = RAM 首地址（每次启动模拟器都会变）+ 偏移，所以这份文件是**快照**：
//! 模拟器一重启就得重新生成（`xiyou-trainer --addresses`）。
//! `config/offsets.yaml` 才是长期有效的那份。

use std::fmt::Write as _;

use winmem::Process;

use crate::game::config::GameConfig;
use crate::game::snapshot::PlayerAddrs;

/// 这次运行的真实基址。
#[derive(Debug, Clone)]
pub struct Locate {
    pub exe: String,
    pub pid: u32,
    pub module_base: usize,
    pub ram_base: usize,
}

/// 依次试配置里的模拟器，找到在跑的那个，读出 RAM 首地址。
pub fn locate(config: &GameConfig) -> Result<Locate, String> {
    let mut tried = Vec::new();
    for emu in &config.emulators {
        let process = match Process::by_name(&emu.exe) {
            Ok(process) => process,
            Err(err) => {
                tried.push(format!("{}：{err}", emu.exe));
                continue;
            }
        };
        let module_base = process
            .module_base(&emu.exe)
            .map_err(|e| format!("{} 取模块基址失败：{e}", emu.exe))?;
        let ram_base = process
            .read::<u32>(crate::game::config::offset(
                module_base,
                emu.ram_ptr_offset,
            ))
            .map_err(|e| format!("{} 读 RAM 指针失败：{e}", emu.exe))?
            as usize;

        if ram_base == 0 {
            return Err(format!(
                "{} 的 RAM 指针是 0，游戏可能还没进关（模拟器在跑，但没载入 ROM）",
                emu.exe
            ));
        }

        return Ok(Locate {
            exe: emu.exe.clone(),
            pid: process.pid(),
            module_base,
            ram_base,
        });
    }
    Err(format!(
        "没找到在跑的模拟器（{}）。先把游戏开起来再生成。",
        tried.join(" / ")
    ))
}

/// 生成 YAML 文本。
pub fn yaml(config: &GameConfig, live: &Locate) -> String {
    let ram = live.ram_base;
    let mut out = String::new();

    let _ = writeln!(
        out,
        "# ============================================================================="
    );
    let _ = writeln!(
        out,
        "#  西游释厄传 —— 绝对地址表（可以直接粘进 Cheat Engine）"
    );
    let _ = writeln!(
        out,
        "# ============================================================================="
    );
    let _ = writeln!(out, "#");
    let _ = writeln!(out, "#  进程        {}", live.exe);
    let _ = writeln!(out, "#  PID         {}", live.pid);
    let _ = writeln!(out, "#  模块基址    0x{:08X}", live.module_base);
    let _ = writeln!(out, "#  RAM 首地址  0x{ram:08X}");
    let _ = writeln!(out, "#");
    let _ = writeln!(
        out,
        "#  ⚠️ 这些是**这一次运行**的绝对地址。RAM 首地址是模拟器每次启动时申请的内存块，"
    );
    let _ = writeln!(
        out,
        "#     重启模拟器就会变，届时重新跑一次 `xiyou-trainer --addresses` 覆盖本文件。"
    );
    let _ = writeln!(
        out,
        "#     长期有效的是 config/offsets.yaml 里的相对偏移，这份只是拿来用的快照。"
    );
    let _ = writeln!(out, "#");
    let _ = writeln!(
        out,
        "#  三种基址的关系："
    );
    let _ = writeln!(
        out,
        "#      模块基址 0x{:08X} + RAM 指针偏移 → 读出 RAM 首地址 0x{ram:08X}",
        live.module_base
    );
    let _ = writeln!(
        out,
        "#      RAM 首地址 + 下面每个偏移 = 下面写的绝对地址"
    );
    let _ = writeln!(
        out,
        "# ============================================================================="
    );
    let _ = writeln!(out);

    let _ = writeln!(out, "# 两个基址本身，方便手算别的地址");
    let _ = writeln!(
        out,
        "module_base: 0x{:08X}   # 模拟器 exe 的模块基址，通常固定在 0x00400000",
        live.module_base
    );
    let _ = writeln!(
        out,
        "ram_base:    0x{ram:08X}   # 模拟 RAM 首地址（= 模块基址 + RAM指针偏移，读出来的）"
    );
    let _ = writeln!(out);

    let _ = writeln!(
        out,
        "# -----------------------------------------------------------------------------"
    );
    let _ = writeln!(out, "#  关卡");
    let _ = writeln!(
        out,
        "# -----------------------------------------------------------------------------"
    );
    let _ = writeln!(
        out,
        "time:  0x{:08X}   # 剩余时间，u8",
        config.time_addr(ram)
    );
    let _ = writeln!(
        out,
        "stage: 0x{:08X}   # 关卡号，u8（0 基，界面显示 = 值 + {}）",
        config.stage_addr(ram),
        config.layout.stage_bias
    );
    let _ = writeln!(out);

    let _ = writeln!(
        out,
        "# -----------------------------------------------------------------------------"
    );
    let _ = writeln!(
        out,
        "#  玩家（每人 5 个字段；空位上的值是残留数据，不用管）"
    );
    let _ = writeln!(
        out,
        "# -----------------------------------------------------------------------------"
    );
    let _ = writeln!(out, "players:");
    for index in 0..config.counts.players {
        let addr = PlayerAddrs::new(config, ram, index);
        let n = index + 1;
        let _ = writeln!(out, "  p{n}:");
        let _ = writeln!(
            out,
            "    hp:        0x{:08X}   # {n}P 血量，u16",
            addr.hp
        );
        let _ = writeln!(
            out,
            "    qi:        0x{:08X}   # {n}P 气槽，u8（上限 {}）",
            addr.qi, config.limits.qi_max
        );
        let _ = writeln!(
            out,
            "    character: 0x{:08X}   # {n}P 角色 ID，u8（0 悟空 / 1 八戒 / 2 悟净 / 3 小龙女 / 4 龙马）",
            addr.character
        );
        let _ = writeln!(
            out,
            "    active:    0x{:08X}   # {n}P 在场标志，u8（0 = 空槽）",
            addr.active
        );
        let _ = writeln!(
            out,
            "    cursor:    0x{:08X}   # {n}P 道具光标，u8（0 基，指向道具环第几格）",
            addr.cursor
        );
        let _ = writeln!(
            out,
            "    items:     0x{:08X}   # {n}P 道具环起点，u8；每格 2 字节 [数量][ID]，共 8 格",
            addr.items
        );
        let _ = writeln!(
            out,
            "    item_total: 0x{:08X}   # {n}P 当前道具总数，u8",
            config.item_ring.total_addr(addr.items)
        );
    }
    let _ = writeln!(out);

    let _ = writeln!(
        out,
        "# -----------------------------------------------------------------------------"
    );
    let _ = writeln!(out, "#  敌兵");
    let _ = writeln!(
        out,
        "# -----------------------------------------------------------------------------"
    );
    let _ = writeln!(out, "enemies:");
    for index in 0..config.counts.enemies {
        let n = index + 1;
        let fields = &config.enemy_fields;
        let _ = writeln!(out, "  e{n}:");
        let _ = writeln!(
            out,
            "    active: 0x{:08X}   # 敌兵 {n} 在场标志，u8（0 空槽 / 2 有兵）",
            config.enemy_field_addr(ram, index, &fields.active)
        );
        let _ = writeln!(
            out,
            "    hp:   0x{:08X}   # 敌兵 {n} 血量，u16",
            config.enemy_field_addr(ram, index, &fields.hp)
        );
        let _ = writeln!(
            out,
            "    x:    0x{:08X}   # 敌兵 {n} X 坐标，u8（调试用，不参与存活判定）",
            config.enemy_field_addr(ram, index, &fields.x)
        );
        let _ = writeln!(
            out,
            "    type: 0x{:08X}   # 敌兵 {n} 类型（对象定义指针），u32",
            config.enemy_field_addr(ram, index, &fields.ty)
        );
    }
    let _ = writeln!(out);

    let _ = writeln!(
        out,
        "# -----------------------------------------------------------------------------"
    );
    let _ = writeln!(out, "#  自定义监视项地址（watches 里的条目算出来的起点）");
    let _ = writeln!(
        out,
        "# -----------------------------------------------------------------------------"
    );
    if config.watches.is_empty() {
        let _ = writeln!(
            out,
            "# （空。在 config/offsets.yaml 的 watches 里加条目后重新生成）"
        );
    } else {
        let _ = writeln!(out, "watches:");
        for watch in &config.watches {
            let start = config.start_addr(&watch.address, ram, live.module_base);
            let chain = if watch.address.deref.is_empty() {
                "直读".to_string()
            } else {
                format!("还需跳 {} 层指针", watch.address.deref.len())
            };
            let _ = writeln!(
                out,
                "  {}: 0x{start:08X}   # {}，{}，{}",
                watch.id,
                watch.title(),
                watch.ty.label(),
                chain
            );
        }
    }

    out
}
