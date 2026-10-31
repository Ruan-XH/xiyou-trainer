use std::fmt;
use std::sync::Arc;

use winmem::{MemError, Process};

use crate::game::config::{self, GameConfig, ValueType, Watch};
use crate::game::items::{self, RING_SLOTS, RingCell};

/// 当前连上的模拟器。字段直接从配置里拷过来，方便界面显示。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmulatorInfo {
    pub label: String,
    /// 「RAM 指针」相对模块基址的偏移。
    pub ram_ptr_offset: i64,
}

impl EmulatorInfo {
    /// 演示模式用的假值。
    pub fn demo() -> Self {
        Self {
            label: "演示模式".to_string(),
            ram_ptr_offset: 0,
        }
    }
}

/// 可选角色。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Character {
    /// 0
    SunWukong,
    /// 1
    ZhuBajie,
    /// 2
    ShaWujing,
    /// 3
    XiaoLongNv,
    /// 4
    LongMa,
}

impl Character {
    pub fn from_id(id: u8) -> Option<Self> {
        Some(match id {
            0 => Character::SunWukong,
            1 => Character::ZhuBajie,
            2 => Character::ShaWujing,
            3 => Character::XiaoLongNv,
            4 => Character::LongMa,
            _ => return None,
        })
    }

    pub fn name(self) -> &'static str {
        match self {
            Character::SunWukong => "孙悟空",
            Character::ZhuBajie => "猪八戒",
            Character::ShaWujing => "沙悟净",
            Character::XiaoLongNv => "小龙女",
            Character::LongMa => "龙马",
        }
    }
}

/// 一个玩家槽位的全部字段地址，集中算一遍避免各处手写正负偏移。
#[derive(Debug, Clone, Copy)]
pub struct PlayerAddrs {
    pub hp: usize,
    pub qi: usize,
    pub character: usize,
    pub active: usize,
    pub cursor: usize,
    pub items: usize,
}

impl PlayerAddrs {
    /// `index` 从 0 开始（0 就是 1P）。
    ///
    /// 字段各自的间距由配置里的 `scale` 决定——这是原版最容易踩错的地方：
    /// 血量/气槽/角色走 `stride_player`，光标/道具环走 `stride_item`，
    /// 两套混用会把两个间距叠加，地址直接跑飞。
    pub fn new(config: &GameConfig, ram_base: usize, index: usize) -> Self {
        let fields = &config.player_fields;
        Self {
            hp: config.player_field_addr(ram_base, index, &fields.hp),
            qi: config.player_field_addr(ram_base, index, &fields.qi),
            character: config.player_field_addr(ram_base, index, &fields.character),
            active: config.player_field_addr(ram_base, index, &fields.active),
            cursor: config.player_field_addr(ram_base, index, &fields.cursor),
            items: config.player_field_addr(ram_base, index, &fields.items),
        }
    }

    /// 道具环里第 `slot` 件（1 基）的 ID 地址。
    pub fn item_id_addr(&self, config: &GameConfig, slot: usize) -> usize {
        config.item_ring.id_addr(self.items, slot)
    }

    /// 道具环里第 `slot` 件（1 基）的数量地址。
    pub fn item_count_addr(&self, config: &GameConfig, slot: usize) -> usize {
        config.item_ring.count_addr(self.items, slot)
    }
}

/// 一个玩家槽位读出来的状态。
///
/// 界面现在只用 `hp` + 光标那件道具，其余字段照样读（`--addresses`、以后的详情页
/// 都要用），先留着别删——它们读错的代价和读对一样，都是几次 ReadProcessMemory。
#[derive(Debug, Clone, Default)]
#[allow(dead_code)]
pub struct PlayerSlot {
    pub hp: u16,
    pub qi: u8,
    pub character: Option<Character>,
    /// 在场标志：0 = 空槽，非 0（实测都是 2）= 这个位置有人。
    /// 它是玩家结构的第一个字节，比血量早 0x57，判「有没有人」只看它。
    pub active: u8,
    /// 光标指向环上第几格（1 基）。超出道具总数或那格没道具时为 `None`。
    pub cursor_slot: Option<u8>,
    /// 环上 8 个位置，`None` 表示这格没道具。
    pub items: [Option<RingCell>; RING_SLOTS],
    /// 游戏自己记的道具总数。
    pub item_total: u8,
}

/// 一个敌兵槽位。
///
/// 判断「这槽还有没有活着的敌人」只能看 `active`（比血量早 0x57 的那一字节，
/// 0 = 空槽 / 2 = 有兵）。血量和坐标都不能用——死槽不清零，残留数据会留到
/// 下一波，新兵直接复用槽位。`ty` 是种类（对象定义指针），同种敌人相同、
/// 死亡也不清零。
#[derive(Debug, Clone, Copy, Default)]
#[allow(dead_code)]
pub struct EnemySlot {
    pub hp: u16,
    /// 在场标志：0 = 空槽，2 = 这槽有活着的敌人。
    pub active: u8,
    pub x: u8,
    pub ty: u32,
}

#[allow(dead_code)]
impl EnemySlot {
    /// 这槽有没有活着的敌人。
    ///
    /// 死亡动画那一秒多一点里它仍然是 2（人还在屏幕上），等槽位真正释放才归
    /// 0，这个滞后正好对上「怪还没消失」的观感，不用另外补偿。
    pub fn alive(&self) -> bool {
        self.active != 0
    }

    /// 还剩血、能打的敌人。
    ///
    /// 比 `alive` 严一档：死亡动画期间 `active` 还是 2 但血已经归零，
    /// 那时界面上这行显示的是「---」，计数也不该把它算进去，两者才对得上。
    pub fn live(&self) -> bool {
        self.alive() && self.hp > 0
    }
}

#[allow(dead_code)]
impl PlayerSlot {
    /// 这个位置到底有没有人。
    ///
    /// 看玩家结构的第一个字节（`active`，比血量早 0x57）：空槽是 0，有人是 2。
    ///
    /// 不能用角色 ID 代替 —— 空槽读出来也是 0，而 0 恰好是孙悟空的编号，
    /// 分不清「有人玩悟空」和「这个位置没人」；也不能用血量，空槽会留着
    /// 上一局的 72 / 68。
    pub fn occupied(&self) -> bool {
        self.active != 0
    }

    /// 光标停着的那件道具（名称 + 数量）。总览和详情页的「当前道具」都从这里取，
    /// 指向空位时返回 `None`。
    pub fn cursor_cell(&self) -> Option<&RingCell> {
        let slot = self.cursor_slot? as usize;
        self.items.get(slot.checked_sub(1)?)?.as_ref()
    }

    /// 气槽百分比，用于画条。
    pub fn qi_ratio(&self, qi_max: u8) -> f32 {
        if qi_max == 0 {
            return 0.0;
        }
        (self.qi as f32 / qi_max as f32).clamp(0.0, 1.0)
    }

    /// 显示用的角色名。空位返回「空位」，在场但角色字节认不出来时返回「未知」。
    pub fn label(&self) -> &'static str {
        if !self.occupied() {
            return "空位";
        }
        self.character.map(Character::name).unwrap_or("未知")
    }
}

/// 自定义监视项读出来的值。
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum WatchValue {
    /// 读到了。`text` 是十进制，`hex` 是同一值的十六进制写法。
    Ok {
        text: String,
        /// 竖版看板地方小，底部条只显示十进制；留在这里给 dump / 以后的详情用。
        #[allow(dead_code)]
        hex: Option<String>,
    },
    /// 读失败（指针为空、地址不可读等）。
    Err(String),
}

#[allow(dead_code)]
impl WatchValue {
    pub fn text(&self) -> &str {
        match self {
            WatchValue::Ok { text, .. } => text,
            WatchValue::Err(err) => err,
        }
    }

    #[allow(dead_code)]
    pub fn hex(&self) -> Option<&str> {
        match self {
            WatchValue::Ok { hex, .. } => hex.as_deref(),
            WatchValue::Err(_) => None,
        }
    }
}

/// 一条监视项的读取结果。
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct WatchRead {
    pub name: String,
    /// 读取宽度。界面暂不显示，留着给详情和排查用。
    #[allow(dead_code)]
    pub ty: ValueType,
    pub value: WatchValue,
}

/// 读出来的标量值。
///
/// `WatchValue` 里还留着一份十六进制写法（`hex`）和类型（`WatchRead.ty`）：
/// 竖版看板地方小，底部监视条只显示十进制，这两个暂时没被界面用到，
/// 但 `--dump-offsets` 和以后想加「点开看详情」时还要用。
#[derive(Debug, Clone, Copy)]
enum Value {
    U(u64),
    I(i64),
    F(f64),
}

impl Value {
    fn text(self) -> String {
        match self {
            Value::U(v) => v.to_string(),
            Value::I(v) => v.to_string(),
            Value::F(v) => {
                if v.fract() == 0.0 {
                    format!("{v:.0}")
                } else {
                    format!("{v:.3}")
                }
            }
        }
    }

    fn hex(self) -> Option<String> {
        match self {
            Value::U(v) => Some(format!("0x{v:X}")),
            Value::I(v) if v >= 0 => Some(format!("0x{v:X}")),
            _ => None,
        }
    }
}

/// 把原始的关卡字节换成给人看的关卡号。
///
/// 游戏里存的是 0 基（第 1 关 = 0），所以显示要加 `bias`。明显离谱的值
/// （0xFF 之类的未初始化内存、或者超出关卡总数的）一律当「还没进关」，返回 0，
/// 界面就会显示 `STAGE --` 而不是胡编一个关号。
fn stage_number(raw: u8, bias: i64) -> u8 {
    /// 这一关之后就没有了，超过它的肯定是没读到有效值。
    const MAX_STAGE: i64 = 12;
    let n = raw as i64 + bias;
    if (1..=MAX_STAGE).contains(&n) {
        n as u8
    } else {
        0
    }
}

/// 一次完整读取的结果。
#[derive(Debug, Clone)]
pub struct GameSnapshot {
    pub emulator: EmulatorInfo,
    pub pid: u32,
    pub ram_base: usize,
    pub time_left: u8,
    /// 关卡号，1 基（已经加过 `stage_bias`）；0 = 还没进关（标题画面 / 读盘中）。
    pub stage: u8,
    /// 气槽上限。纯文字界面不画条，暂时用不上，留着给以后的详情页。
    #[allow(dead_code)]
    pub qi_max: u8,
    pub players: Vec<PlayerSlot>,
    pub enemies: Vec<EnemySlot>,
    /// `watches` 是在配置里自定义的监视项，界面上暂时没地方显示（240px 宽塞不下），
    /// 数据照读，`--dump-offsets` 和以后的「详情」视图要用。
    #[allow(dead_code)]
    pub watches: Vec<WatchRead>,
}

impl GameSnapshot {
    /// 右上角关卡章的文字：「STAGE 3」。
    ///
    /// 还没进关时返回 `None`，界面自己决定显示什么占位。
    ///
    /// 曾经这里还拼过小关号（STAGE 3 · 2），但那一格在内存里会自己乱跳，
    /// 实机看着像坏了，所以只留关号。
    pub fn stage_label(&self) -> Option<String> {
        stage_text(self.stage)
    }
}

/// 拼关卡章文字。单独拎出来是为了能脱离快照直接测。
fn stage_text(stage: u8) -> Option<String> {
    if stage == 0 {
        return None;
    }
    Some(format!("STAGE {stage}"))
}

/// 附加失败的原因。
#[derive(Debug)]
pub enum AttachError {
    /// 配置里所有进程都没找到。
    NoProcess,
    /// 找到了进程但打不开（权限等）。
    Open { name: String, source: MemError },
    /// 模块枚举失败。
    Module { source: MemError },
}

impl fmt::Display for AttachError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AttachError::NoProcess => write!(f, "没找到模拟器进程，先启动 Fba.exe / Fbas.exe"),
            AttachError::Open { name, source } => write!(f, "附加 {name} 失败：{source}"),
            AttachError::Module { source } => write!(f, "取模块基址失败：{source}"),
        }
    }
}

/// 读取失败的原因。
#[derive(Debug)]
pub enum ReadError {
    /// RAM 指针还是 0，说明游戏还没进关卡 / 模拟器还没加载 ROM。
    RamNotReady(u32),
    /// 监视项的指针链断了。
    NullPointer { watch: String, step: usize },
    /// 底层读取失败。
    Mem(MemError),
}

impl fmt::Display for ReadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ReadError::RamNotReady(v) => write!(f, "RAM 指针为 0x{v:08X}，游戏可能还没进关"),
            ReadError::NullPointer { watch, step } => {
                write!(f, "监视项 `{watch}` 第 {step} 层指针是 0")
            }
            ReadError::Mem(e) => write!(f, "读取内存失败：{e}"),
        }
    }
}

impl From<MemError> for ReadError {
    fn from(e: MemError) -> Self {
        ReadError::Mem(e)
    }
}

/// 挂在模拟器上的读取器。持有进程句柄，离开作用域自动关闭。
pub struct GameReader {
    process: Process,
    config: Arc<GameConfig>,
    emulator: EmulatorInfo,
    module_base: usize,
}

impl GameReader {
    /// 按配置里的顺序依次尝试各个模拟器。
    pub fn attach(config: Arc<GameConfig>) -> Result<Self, AttachError> {
        let mut last_open_error = None;

        for emu in &config.emulators {
            match Process::by_name(&emu.exe) {
                Ok(process) => {
                    let module_base = process
                        .module_base(&emu.exe)
                        .map_err(|source| AttachError::Module { source })?;
                    let info = EmulatorInfo {
                        label: if emu.label.is_empty() {
                            emu.id.clone()
                        } else {
                            emu.label.clone()
                        },
                        ram_ptr_offset: emu.ram_ptr_offset,
                    };
                    return Ok(Self {
                        process,
                        config,
                        emulator: info,
                        module_base,
                    });
                }
                Err(MemError::ProcessNotFound { .. }) => continue,
                Err(source) => {
                    last_open_error = Some(AttachError::Open {
                        name: emu.exe.clone(),
                        source,
                    });
                }
            }
        }

        Err(last_open_error.unwrap_or(AttachError::NoProcess))
    }

    /// 读 RAM 指针。游戏没加载 ROM 时这个值是 0。
    pub fn ram_base(&self) -> Result<usize, ReadError> {
        let addr = config::offset(self.module_base, self.emulator.ram_ptr_offset);
        let raw: u32 = self.process.read(addr)?;
        if raw == 0 {
            return Err(ReadError::RamNotReady(raw));
        }
        Ok(raw as usize)
    }

    /// 读一份完整快照。
    pub fn snapshot(&self) -> Result<GameSnapshot, ReadError> {
        let ram_base = self.ram_base()?;
        let config = &self.config;

        let time_left: u8 = self.process.read(config.time_addr(ram_base))?;
        // 关卡表是 0 基（第 1 关 = 0），换成给人看的号码要靠 stage_bias。
        let stage_raw: u8 = self.process.read(config.stage_addr(ram_base))?;
        let stage = stage_number(stage_raw, config.layout.stage_bias);

        let mut players = Vec::with_capacity(config.counts.players);
        for index in 0..config.counts.players {
            players.push(self.read_player(ram_base, index)?);
        }

        let mut enemies = Vec::with_capacity(config.counts.enemies);
        for index in 0..config.counts.enemies {
            let fields = &config.enemy_fields;
            enemies.push(EnemySlot {
                hp: self.process.read(config.enemy_field_addr(ram_base, index, &fields.hp))?,
                active: self
                    .process
                    .read(config.enemy_field_addr(ram_base, index, &fields.active))?,
                x: self.process.read(config.enemy_field_addr(ram_base, index, &fields.x))?,
                ty: self
                    .process
                    .read(config.enemy_field_addr(ram_base, index, &fields.ty))?,
            });
        }

        let watches = config
            .watches
            .iter()
            .map(|watch| self.read_watch(watch, ram_base))
            .collect();

        Ok(GameSnapshot {
            emulator: self.emulator.clone(),
            pid: self.process.pid(),
            ram_base,
            time_left,
            stage,
            qi_max: config.limits.qi_max,
            players,
            enemies,
            watches,
        })
    }

    fn read_player(&self, ram_base: usize, index: usize) -> Result<PlayerSlot, ReadError> {
        let config = &self.config;
        let ring_cfg = &config.item_ring;
        let addrs = PlayerAddrs::new(config, ram_base, index);

        let character = Character::from_id(self.process.read::<u8>(addrs.character)?);
        let active: u8 = self.process.read(addrs.active)?;
        let hp: u16 = self.process.read(addrs.hp)?;
        let qi: u8 = self.process.read(addrs.qi)?;
        let cursor: u8 = self.process.read(addrs.cursor)?;
        let item_total: u8 = self.process.read(ring_cfg.total_addr(addrs.items))?;

        // 8 格全读，但只认前 total 个。游戏里 total 偶尔会短暂越界，钳一下。
        let total = item_total.min(RING_SLOTS as u8);
        let mut ring: [Option<RingCell>; RING_SLOTS] = Default::default();
        for slot in 1..=total as usize {
            let id: u8 = self.process.read(addrs.item_id_addr(config, slot))?;
            let count: u8 = self.process.read(addrs.item_count_addr(config, slot))?;
            if ring_cfg.is_item_id(id) && items::item_name(id).is_some() && count > 0 {
                ring[slot - 1] = Some(RingCell::new(id, count));
            }
        }

        // 光标是 0 基，环是 1 基；指向空位时不算「当前道具」。
        let cursor_slot = cursor.saturating_add(1);
        let cursor_item = if cursor_slot <= total {
            ring[cursor_slot as usize - 1].clone()
        } else {
            None
        };

        Ok(PlayerSlot {
            hp,
            qi,
            character,
            active,
            cursor_slot: cursor_item.as_ref().map(|_| cursor_slot),
            items: ring,
            item_total: total,
        })
    }

    /// 解析一条监视项：先算起始地址，再按 deref 一层层跳指针。
    fn read_watch(&self, watch: &Watch, ram_base: usize) -> WatchRead {
        let value = match self.resolve_watch(watch, ram_base) {
            Ok(addr) => match read_value(&self.process, addr, watch.ty) {
                Ok(v) => WatchValue::Ok {
                    text: v.text(),
                    hex: v.hex(),
                },
                Err(err) => WatchValue::Err(format!("读不了 0x{addr:08X}：{err}")),
            },
            Err(err) => WatchValue::Err(err.to_string()),
        };

        WatchRead {
            name: watch.title().to_string(),
            ty: watch.ty,
            value,
        }
    }

    fn resolve_watch(&self, watch: &Watch, ram_base: usize) -> Result<usize, ReadError> {
        let mut addr = self
            .config
            .start_addr(&watch.address, ram_base, self.module_base);

        for (step, delta) in watch.address.deref.iter().enumerate() {
            let pointer: u32 = self.process.read(addr)?;
            if pointer == 0 {
                return Err(ReadError::NullPointer {
                    watch: watch.title().to_string(),
                    step: step + 1,
                });
            }
            addr = config::offset(pointer as usize, *delta);
        }

        Ok(addr)
    }
}

fn read_value(process: &Process, addr: usize, ty: ValueType) -> Result<Value, MemError> {
    Ok(match ty {
        ValueType::U8 => Value::U(process.read::<u8>(addr)? as u64),
        ValueType::I8 => Value::I(process.read::<i8>(addr)? as i64),
        ValueType::U16 => Value::U(process.read::<u16>(addr)? as u64),
        ValueType::I16 => Value::I(process.read::<i16>(addr)? as i64),
        ValueType::U32 => Value::U(process.read::<u32>(addr)? as u64),
        ValueType::I32 => Value::I(process.read::<i32>(addr)? as i64),
        ValueType::U64 => Value::U(process.read::<u64>(addr)?),
        ValueType::F32 => Value::F(process.read::<f32>(addr)? as f64),
        ValueType::F64 => Value::F(process.read::<f64>(addr)?),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::config::default_config;

    const RAM: usize = 0x1000_0000;

    #[test]
    fn 关卡章只有关号() {
        assert_eq!(stage_text(3).as_deref(), Some("STAGE 3"));
        assert_eq!(stage_text(4).as_deref(), Some("STAGE 4"));
        // 还没进关
        assert_eq!(stage_text(0), None);
        // 不再拼小关号：那一格会乱跳，界面上不能出现分隔点
        let text = stage_text(5).unwrap();
        assert!(!text.contains('\u{B7}'), "不该再带小关号");
    }

    #[test]
    fn 关卡号由原始字节加偏移得来() {
        // 0 基表：第 1 关 = 0、第 2 关 = 1、第 3 关 = 2
        assert_eq!(stage_number(0, 1), 1);
        assert_eq!(stage_number(1, 1), 2);
        assert_eq!(stage_number(2, 1), 3);
        // 没读到时是 0xFF 之类的乱值，不能显示成「第 256 关」
        assert_eq!(stage_number(0xFF, 1), 0, "0xFF 当未进关");
        assert_eq!(stage_number(0xF0, 1), 0);
        // 万一以后 bias 配成 0（1 基表），也不能让 0 关露出来
        assert_eq!(stage_number(0, 0), 0);
        assert_eq!(stage_number(1, 0), 1);
    }

    #[test]
    fn 一级偏移与原版一致() {
        let config = default_config();
        let p1 = PlayerAddrs::new(&config, RAM, 0);
        assert_eq!(p1.hp, RAM + 0x1BE98);
        assert_eq!(p1.qi - p1.hp, 0xB5);
        assert_eq!(p1.hp - p1.character, 0x0C, "角色 ID 就在血量前面一点");
        assert_eq!(p1.hp - p1.active, 0x57, "在场标志是结构第一个字节");
        assert_eq!(p1.hp - p1.cursor, 0xACFC, "光标在血量前面");
        assert_eq!(p1.hp - p1.items, 0xAD0D, "道具环更靠前");
    }

    #[test]
    fn 玩家之间按各自间距递推() {
        let config = default_config();
        let p1 = PlayerAddrs::new(&config, RAM, 0);
        let p3 = PlayerAddrs::new(&config, RAM, 2);
        assert_eq!(p3.hp - p1.hp, 2 * 0x13E);
        assert_eq!(p3.qi - p1.qi, 2 * 0x13E);
        assert_eq!(p3.character - p1.character, 2 * 0x13E);
        assert_eq!(p3.cursor - p1.cursor, 2 * 0x7E, "光标用的是另一个间距");
        assert_eq!(p3.items - p1.items, 2 * 0x7E);
    }

    #[test]
    fn 道具格相邻两字节() {
        let config = default_config();
        let p1 = PlayerAddrs::new(&config, RAM, 0);
        assert_eq!(p1.item_id_addr(&config, 1), p1.items);
        assert_eq!(
            p1.item_count_addr(&config, 1),
            p1.items - 1,
            "数量在 ID 前一个字节"
        );
        assert_eq!(
            p1.item_id_addr(&config, 2) - p1.item_id_addr(&config, 1),
            2
        );
    }

    #[test]
    fn 负偏移不会下溢() {
        let config = default_config();
        let addrs = PlayerAddrs::new(&config, 0x1000, 0);
        assert!(addrs.cursor < addrs.hp);
        assert!(addrs.items < addrs.cursor);
    }

    #[test]
    fn 角色id映射() {
        assert_eq!(Character::from_id(0).map(|c| c.name()), Some("孙悟空"));
        assert_eq!(Character::from_id(4).map(|c| c.name()), Some("龙马"));
        assert_eq!(Character::from_id(5), None, "5 以上不是有效角色");
        assert_eq!(Character::from_id(255), None);
    }

    #[test]
    fn 有没有人看在场标志不看角色id() {
        // 有人：在场字节非 0，角色认得出
        let slot = PlayerSlot {
            character: Some(Character::SunWukong),
            active: 2,
            ..Default::default()
        };
        assert!(slot.occupied());
        assert_eq!(slot.label(), "孙悟空");

        // 关键用例：空槽的角色字节读出来是 0，而 0 就是孙悟空。
        // 只看角色 ID 会把空位显示成「孙悟空」，所以必须先看在场字节。
        let empty = PlayerSlot {
            character: Some(Character::SunWukong),
            active: 0,
            ..Default::default()
        };
        assert!(!empty.occupied(), "在场字节是 0 就是空位，哪怕角色字节像悟空");
        assert_eq!(empty.label(), "空位");

        // 在场但角色字节认不出来：算有人，显示「未知」
        let unknown = PlayerSlot {
            character: None,
            active: 2,
            ..Default::default()
        };
        assert!(unknown.occupied());
        assert_eq!(unknown.label(), "未知");
    }

    #[test]
    fn 当前道具跟着光标走() {
        let mut slot = PlayerSlot {
            item_total: 3,
            ..Default::default()
        };
        slot.items[0] = Some(RingCell::new(2, 5)); // 刀
        slot.items[2] = Some(RingCell::new(7, 1)); // 照妖镜

        slot.cursor_slot = Some(1);
        assert_eq!(slot.cursor_cell().map(|c| c.name), Some("刀"));
        assert_eq!(slot.cursor_cell().map(|c| c.count), Some(5));

        slot.cursor_slot = Some(2);
        assert!(slot.cursor_cell().is_none(), "指向空格时没有当前道具");

        slot.cursor_slot = Some(3);
        assert_eq!(slot.cursor_cell().map(|c| c.name), Some("照妖镜"));

        slot.cursor_slot = Some(9);
        assert!(slot.cursor_cell().is_none(), "越界不能读到别人的道具");

        slot.cursor_slot = None;
        assert!(slot.cursor_cell().is_none());
    }

    #[test]
    fn 小兵存活看在场标志不看血量() {
        // 死槽：血量、坐标、种类全是上一波剩下的，只有在场标志是 0
        let stale = EnemySlot {
            hp: 20,
            active: 0,
            x: 90,
            ty: 0x233B54,
        };
        assert!(!stale.alive(), "残留 20 血也不能当成有兵");
        assert!(!stale.live());

        // 活兵：受伤掉血时标志不变
        let hurt = EnemySlot {
            hp: 3,
            active: 2,
            x: 40,
            ty: 0x240044,
        };
        assert!(hurt.alive());
        assert!(hurt.live());

        // 死亡动画那一秒多：血已归零，人还在屏幕上，标志仍是 2
        let dying = EnemySlot {
            hp: 0,
            active: 2,
            x: 40,
            ty: 0x240044,
        };
        assert!(dying.alive());
        assert!(!dying.live(), "血已归零，不该算进「还剩几只」");
    }

    #[test]
    fn 气槽比例按配置上限() {
        let slot = PlayerSlot {
            qi: 48,
            ..Default::default()
        };
        assert!((slot.qi_ratio(96) - 0.5).abs() < 1e-6);
        assert_eq!(slot.qi_ratio(0), 0.0, "上限为 0 不能除零");
    }

    /// 真的读一次内存：读自己进程里堆上的值，验证类型分派和格式化。
    #[test]
    fn 类型化读取走真实内存() {
        let process = Process::by_pid(std::process::id()).expect("打开自身进程");

        let word = Box::new(0xDEAD_BEEFu32);
        let addr = &*word as *const u32 as usize;
        let value = read_value(&process, addr, ValueType::U32).expect("读 u32");
        assert_eq!(value.text(), "3735928559");
        assert_eq!(value.hex().as_deref(), Some("0xDEADBEEF"));

        let byte = Box::new(200u8);
        let addr = &*byte as *const u8 as usize;
        assert_eq!(read_value(&process, addr, ValueType::U8).unwrap().text(), "200");

        let float = Box::new(12.5f32);
        let addr = &*float as *const f32 as usize;
        assert_eq!(
            read_value(&process, addr, ValueType::F32).unwrap().text(),
            "12.500"
        );

        let negative = Box::new(-7i16);
        let addr = &*negative as *const i16 as usize;
        assert_eq!(
            read_value(&process, addr, ValueType::I16).unwrap().text(),
            "-7"
        );
    }
}
