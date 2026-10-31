//! 内存偏移配置：从 `config/offsets.yaml` 加载。
//!
//! 地址不再写死在代码里。查找顺序：
//!
//! 1. 环境变量 `XIYOU_OFFSETS` 指向的文件
//! 2. exe 同级的 `config/offsets.yaml`
//! 3. 当前工作目录的 `config/offsets.yaml`（`cargo run` 就是这一条）
//! 4. exe 往上两级的 `config/offsets.yaml`（`target/debug/` 跑的时候用这条）
//! 5. 都没有就用编译进二进制的内置副本
//!
//! 文件存在但解析失败时**不会**静默回退——会带着错误信息用内置副本启动，
//! 界面上会显示提示，免得改了半天以为生效了。

use std::path::PathBuf;
use std::sync::{Arc, LazyLock};

use serde::de::{self, Deserializer};
use serde::Deserialize;

/// 编译进二进制的默认配置，和仓库里的 `config/offsets.yaml` 内容一致。
const EMBEDDED: &str = include_str!("../../config/offsets.yaml");

// ---------------------------------------------------------------- 数字解析

/// YAML 里既可以写 `0x1BE98`，也可以写 `114328`。解析器给什么类型都接得住。
#[derive(Deserialize)]
#[serde(untagged)]
enum RawNum {
    Str(String),
    Int(i64),
    UInt(u64),
    Float(f64),
}

fn parse_num(text: &str) -> Result<i64, String> {
    let s = text.trim();
    for (prefix, negate) in [("0x", false), ("0X", false), ("-0x", true), ("-0X", true)] {
        if let Some(rest) = s.strip_prefix(prefix) {
            let digits = rest.replace('_', "");
            let value = i64::from_str_radix(&digits, 16)
                .map_err(|e| format!("`{text}` 不是合法十六进制：{e}"))?;
            return Ok(if negate { -value } else { value });
        }
    }
    s.replace('_', "")
        .parse::<i64>()
        .map_err(|e| format!("`{text}` 既不是 0x 十六进制也不是十进制：{e}"))
}

fn raw_to_i64(raw: RawNum) -> Result<i64, String> {
    match raw {
        RawNum::Int(v) => Ok(v),
        RawNum::UInt(v) => i64::try_from(v).map_err(|_| format!("{v} 太大")),
        RawNum::Float(v) => Ok(v as i64),
        RawNum::Str(s) => parse_num(&s),
    }
}

fn de_i64<'de, D: Deserializer<'de>>(d: D) -> Result<i64, D::Error> {
    raw_to_i64(RawNum::deserialize(d)?).map_err(de::Error::custom)
}

fn de_usize<'de, D: Deserializer<'de>>(d: D) -> Result<usize, D::Error> {
    let v = de_i64(d)?;
    usize::try_from(v).map_err(|_| de::Error::custom(format!("{v} 不能为负")))
}

fn de_u8<'de, D: Deserializer<'de>>(d: D) -> Result<u8, D::Error> {
    let v = de_i64(d)?;
    u8::try_from(v).map_err(|_| de::Error::custom(format!("{v} 超出 0..=255")))
}

fn de_u16<'de, D: Deserializer<'de>>(d: D) -> Result<u16, D::Error> {
    let v = de_i64(d)?;
    u16::try_from(v).map_err(|_| de::Error::custom(format!("{v} 超出 0..=65535")))
}

fn de_u32<'de, D: Deserializer<'de>>(d: D) -> Result<u32, D::Error> {
    let v = de_i64(d)?;
    u32::try_from(v).map_err(|_| de::Error::custom(format!("{v} 超出 u32 范围")))
}

fn de_vec_i64<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<i64>, D::Error> {
    Vec::<RawNum>::deserialize(d)?
        .into_iter()
        .map(|raw| raw_to_i64(raw).map_err(de::Error::custom))
        .collect()
}

// ---------------------------------------------------------------- 枚举

/// 字段按哪一套间距递推。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Scale {
    /// 不加间距。
    #[default]
    None,
    /// `stride_player`：血量、气槽、角色 ID。
    Player,
    /// `stride_item`：道具光标、道具环。
    Item,
}

/// 地址的基址是什么。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Base {
    /// 模拟 RAM 首地址（绝大多数情况）。
    #[default]
    Ram,
    /// 模拟器模块基址（少数要看模拟器自身变量时用）。
    Module,
}

/// 读出来的值的类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ValueType {
    U8,
    I8,
    U16,
    I16,
    #[default]
    U32,
    I32,
    U64,
    F32,
    F64,
}

impl ValueType {
    pub fn label(self) -> &'static str {
        match self {
            ValueType::U8 => "u8",
            ValueType::I8 => "i8",
            ValueType::U16 => "u16",
            ValueType::I16 => "i16",
            ValueType::U32 => "u32",
            ValueType::I32 => "i32",
            ValueType::U64 => "u64",
            ValueType::F32 => "f32",
            ValueType::F64 => "f64",
        }
    }
}

// ---------------------------------------------------------------- 结构

#[derive(Debug, Clone, Deserialize)]
pub struct EmulatorCfg {
    pub id: String,
    #[serde(default)]
    pub label: String,
    pub exe: String,
    /// 「RAM 指针」变量相对模块基址的偏移。
    #[serde(deserialize_with = "de_i64")]
    pub ram_ptr_offset: i64,
    #[serde(default)]
    pub desc: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Layout {
    /// 剩余时间。
    #[serde(deserialize_with = "de_i64")]
    pub time: i64,
    /// 关卡号（0 基，第 1 关 = 0）。老配置里没有这一项时退回默认值。
    #[serde(default = "default_stage_rel", deserialize_with = "de_i64")]
    pub stage: i64,
    /// 显示用的偏移：界面显示 `stage + stage_bias`（0 基表要 +1）。缺省 1。
    #[serde(default = "default_stage_bias", deserialize_with = "de_i64")]
    pub stage_bias: i64,
    /// 1P 血量，所有玩家字段的锚点。
    #[serde(deserialize_with = "de_i64")]
    pub player_anchor: i64,
    /// 1 号敌兵血量。
    #[serde(deserialize_with = "de_i64")]
    pub enemy_anchor: i64,
    #[serde(deserialize_with = "de_i64")]
    pub stride_player: i64,
    #[serde(deserialize_with = "de_i64")]
    pub stride_item: i64,
}

impl Layout {
    pub fn stride(&self, scale: Scale) -> i64 {
        match scale {
            Scale::None => 0,
            Scale::Player => self.stride_player,
            Scale::Item => self.stride_item,
        }
    }
}

/// `layout.stage` 的默认值。老配置文件里没有这一行时用它，
/// 免得升级后旧配置直接报「缺字段」而被拒。
fn default_stage_rel() -> i64 {
    0xC03B
}

fn default_stage_bias() -> i64 {
    1
}

/// 玩家结构里的一个字段。
#[derive(Debug, Clone, Deserialize)]
pub struct Field {
    /// 相对 `player_anchor`（敌兵则是 `enemy_anchor`），可为负。
    #[serde(deserialize_with = "de_i64")]
    pub rel: i64,
    #[serde(default)]
    pub scale: Scale,
    /// YAML 里写 `type: u16`。
    #[serde(default, rename = "type", alias = "ty")]
    pub ty: ValueType,
    #[serde(default)]
    pub desc: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PlayerFields {
    pub hp: Field,
    pub qi: Field,
    pub character: Field,
    pub cursor: Field,
    pub items: Field,
    /// 在场标志：0 = 空槽，非 0 = 这个位置有人（实测有人的槽都是 2）。
    ///
    /// 它是玩家结构的第一个字节，比血量锚点早 0x57。界面判断有没有人只看它 ——
    /// 角色 ID 不行（空槽也是 0，而 0 就是悟空），血量也不行（空槽留着上一局的 72）。
    /// 老配置里没有这一段时用默认值。
    #[serde(default = "default_active_field")]
    pub active: Field,
}

fn default_active_field() -> Field {
    Field {
        rel: -0x57,
        scale: Scale::Player,
        ty: ValueType::U8,
        desc: "在场标志".to_string(),
    }
}

/// 敌兵结构里的字段，`rel` 相对 `enemy_anchor`。
#[derive(Debug, Clone, Deserialize)]
pub struct EnemyFields {
    pub hp: Field,
    /// 在场标志：0 = 空槽，2 = 这槽有活着的敌人。比血量早 0x57。
    ///
    /// 判存活只能看它：死槽不清零，残留的血量 / 坐标 / 种类全都在。
    /// 老配置里没有这一段时用默认值（rel -0x57）。
    #[serde(default = "default_enemy_active_field")]
    pub active: Field,
    /// X 坐标。老版本靠它「有没有变化」做活性检测，现在只供调试查看。
    pub x: Field,
    /// 对象定义指针（种类），同种敌人相同，死亡不清零。
    #[serde(rename = "type", alias = "ty")]
    pub ty: Field,
}

fn default_enemy_active_field() -> Field {
    Field {
        rel: -0x57,
        scale: Scale::Player,
        ty: ValueType::U8,
        desc: "在场标志".to_string(),
    }
}

impl Default for EnemyFields {
    fn default() -> Self {
        Self {
            hp: Field {
                rel: 0,
                scale: Scale::Player,
                ty: ValueType::U16,
                desc: "血量".to_string(),
            },
            active: default_enemy_active_field(),
            x: Field {
                rel: -0x2B,
                scale: Scale::Player,
                ty: ValueType::U8,
                desc: "X 坐标".to_string(),
            },
            ty: Field {
                rel: -0x34,
                scale: Scale::Player,
                ty: ValueType::U32,
                desc: "类型（对象定义指针）".to_string(),
            },
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize)]
pub struct Counts {
    #[serde(deserialize_with = "de_usize")]
    pub players: usize,
    #[serde(deserialize_with = "de_usize")]
    pub enemies: usize,
}

impl Default for Counts {
    fn default() -> Self {
        Self {
            players: 4,
            enemies: 6,
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize)]
pub struct Limits {
    #[serde(deserialize_with = "de_u8")]
    pub qi_max: u8,
    /// 玩家血条满值。游戏里血量的最大值，用来算血条比例。
    #[serde(default = "default_hp_max", deserialize_with = "de_u8")]
    pub hp_max: u8,
}

fn default_hp_max() -> u8 {
    72
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            qi_max: 96,
            hp_max: default_hp_max(),
        }
    }
}

/// 敌兵类型 → 显示名 + 满血值。
///
/// 内存里的 `enemy_fields.type` 是 ROM 对象定义指针，同种敌人相同。
/// 这里给每个已知类型起个名字，界面就直接显示名字；表里没有的类型显示编号，
/// 看到新的再补一行即可，不用改代码。
#[derive(Debug, Clone, Deserialize)]
pub struct EnemyType {
    #[serde(deserialize_with = "de_u32")]
    pub value: u32,
    pub name: String,
    /// 这种敌人的满血值，只用来算血条长度。不写按 100 算。
    #[serde(rename = "max", default = "default_enemy_max", deserialize_with = "de_u16")]
    pub max: u16,
}

fn default_enemy_max() -> u16 {
    100
}

/// 道具环的字节布局。
#[derive(Debug, Clone, Deserialize)]
pub struct ItemRing {
    #[serde(deserialize_with = "de_u8", default = "default_capacity")]
    pub capacity: u8,
    #[serde(deserialize_with = "de_i64", default = "default_entry_size")]
    pub entry_size: i64,
    #[serde(deserialize_with = "de_i64", default)]
    pub id_rel: i64,
    #[serde(deserialize_with = "de_i64", default = "default_count_rel")]
    pub count_rel: i64,
    #[serde(deserialize_with = "de_i64", default = "default_total_rel")]
    pub total_rel: i64,
    #[serde(deserialize_with = "de_u8", default = "default_id_min")]
    pub id_min: u8,
    #[serde(deserialize_with = "de_u8", default = "default_id_max")]
    pub id_max: u8,
}

fn default_capacity() -> u8 {
    8
}
fn default_entry_size() -> i64 {
    2
}
fn default_count_rel() -> i64 {
    -1
}
fn default_total_rel() -> i64 {
    18
}
fn default_id_min() -> u8 {
    2
}
fn default_id_max() -> u8 {
    23
}

impl Default for ItemRing {
    fn default() -> Self {
        Self {
            capacity: default_capacity(),
            entry_size: default_entry_size(),
            id_rel: 0,
            count_rel: default_count_rel(),
            total_rel: default_total_rel(),
            id_min: default_id_min(),
            id_max: default_id_max(),
        }
    }
}

impl ItemRing {
    /// 第 `slot` 件（1 基）的 ID 地址。
    pub fn id_addr(&self, base: usize, slot: usize) -> usize {
        offset(base, (slot.saturating_sub(1) as i64) * self.entry_size + self.id_rel)
    }

    /// 第 `slot` 件（1 基）的数量地址。
    pub fn count_addr(&self, base: usize, slot: usize) -> usize {
        offset(base, (slot.saturating_sub(1) as i64) * self.entry_size + self.count_rel)
    }

    /// 环上「当前道具总数」的地址。
    pub fn total_addr(&self, base: usize) -> usize {
        offset(base, self.total_rel)
    }

    pub fn is_item_id(&self, id: u8) -> bool {
        id >= self.id_min && id <= self.id_max
    }
}

/// 一条自定义监视项。
#[derive(Debug, Clone, Deserialize)]
pub struct Watch {
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub desc: String,
    /// YAML 里写 `type: u32`。
    #[serde(default, rename = "type", alias = "ty")]
    pub ty: ValueType,
    pub address: Addr,
}

impl Watch {
    pub fn title(&self) -> &str {
        if self.name.is_empty() {
            &self.id
        } else {
            &self.name
        }
    }
}

/// 地址表达式。
///
/// ```text
/// final = base + offset + index × stride
/// 然后按 deref 依次：读 u32 当指针 → 加偏移
/// ```
#[derive(Debug, Clone, Deserialize)]
pub struct Addr {
    #[serde(default)]
    pub base: Base,
    #[serde(deserialize_with = "de_i64")]
    pub offset: i64,
    #[serde(default)]
    pub scale: Scale,
    #[serde(default)]
    pub index: usize,
    /// 指针链。空数组表示直接读，不跳。
    #[serde(default, deserialize_with = "de_vec_i64")]
    pub deref: Vec<i64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GameConfig {
    pub schema_version: u32,
    #[serde(default)]
    pub game: String,
    pub emulators: Vec<EmulatorCfg>,
    pub layout: Layout,
    #[serde(default)]
    pub counts: Counts,
    #[serde(default)]
    pub limits: Limits,
    pub player_fields: PlayerFields,
    #[serde(default)]
    pub enemy_fields: EnemyFields,
    #[serde(default)]
    pub item_ring: ItemRing,
    /// 敌兵类型对照表。空表也能跑，界面会显示成编号。
    #[serde(default)]
    pub enemy_types: Vec<EnemyType>,
    #[serde(default)]
    pub watches: Vec<Watch>,
}

// ---------------------------------------------------------------- 地址计算

/// 基址加有符号偏移。负偏移不会绕成天文数字。
pub fn offset(base: usize, delta: i64) -> usize {
    base.wrapping_add_signed(delta as isize)
}

/// 终端里的显示宽度：全角字符占两列，`--dump-offsets` 靠它对齐表格。
fn text_width(text: &str) -> usize {
    text.chars()
        .map(|c| if c as u32 > 0x2E80 { 2 } else { 1 })
        .sum()
}

impl GameConfig {
    /// 玩家第 `index` 个槽位的某个字段地址。
    pub fn player_field_addr(&self, ram_base: usize, index: usize, field: &Field) -> usize {
        let delta = self.layout.player_anchor
            + field.rel
            + index as i64 * self.layout.stride(field.scale);
        offset(ram_base, delta)
    }

    /// 第 `index` 个敌兵的某个字段地址（血量 / 坐标 / 类型）。
    pub fn enemy_field_addr(&self, ram_base: usize, index: usize, field: &Field) -> usize {
        let delta = self.layout.enemy_anchor
            + field.rel
            + index as i64 * self.layout.stride(field.scale);
        offset(ram_base, delta)
    }

    /// 剩余时间地址。
    pub fn time_addr(&self, ram_base: usize) -> usize {
        offset(ram_base, self.layout.time)
    }

    /// 关卡号地址。
    pub fn stage_addr(&self, ram_base: usize) -> usize {
        offset(ram_base, self.layout.stage)
    }

    /// 敌兵类型的显示名和满血值。
    ///
    /// 表里查得到就用表里的名字，查不到就显示成 6 位编号——这样遇到没登记的
    /// 新兵种，看板上会直接露出它的类型值，抄进 YAML 就变成中文名。
    pub fn enemy_type(&self, ty: u32) -> (String, u16) {
        if ty == 0 {
            return ("—".to_string(), default_enemy_max());
        }
        match self.enemy_types.iter().find(|t| t.value == ty) {
            Some(t) => (t.name.clone(), t.max.max(1)),
            None => (format!("{ty:06X}"), default_enemy_max()),
        }
    }

    /// 解析一条地址表达式，得到**跳指针之前**的起始地址。
    pub fn start_addr(&self, addr: &Addr, ram_base: usize, module_base: usize) -> usize {
        let base = match addr.base {
            Base::Ram => ram_base,
            Base::Module => module_base,
        };
        let delta = addr.offset + addr.index as i64 * self.layout.stride(addr.scale);
        offset(base, delta)
    }
}

// ---------------------------------------------------------------- 加载

/// 配置从哪来的。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    File,
    Embedded,
}

/// 加载结果。即使出错也会带一份可用的配置，让程序能跑起来。
#[derive(Debug, Clone)]
pub struct Loaded {
    pub config: Arc<GameConfig>,
    pub source: Source,
    pub path: Option<PathBuf>,
    /// 文件读取或解析失败时的说明。
    pub error: Option<String>,
}

impl Loaded {
    /// 给界面用的一行来源说明。
    pub fn describe_source(&self) -> String {
        match (&self.error, &self.path, self.source) {
            (Some(err), _, _) => format!("配置加载失败，已用内置副本：{err}"),
            (None, Some(path), _) => format!("配置：{}", path.display()),
            (None, None, Source::Embedded) => "配置：内置默认值".to_string(),
            _ => "配置：内置默认值".to_string(),
        }
    }
}

static DEFAULT_CONFIG: LazyLock<Arc<GameConfig>> = LazyLock::new(|| {
    let config = from_yaml(EMBEDDED).expect("内置 offsets.yaml 有问题，这是编译期错误");
    Arc::new(config)
});

/// 内置默认配置。
pub fn default_config() -> Arc<GameConfig> {
    DEFAULT_CONFIG.clone()
}

/// 从 YAML 文本解析并校验。
pub fn from_yaml(text: &str) -> Result<GameConfig, String> {
    let config: GameConfig = serde_yaml::from_str(text).map_err(|e| {
        // serde_yaml 的 Display 末尾自带 " at line N column M"，没有就补行号
        let text = e.to_string();
        if text.contains("line") {
            text
        } else {
            format!("{text}（第 {} 行）", e.location().map_or(0, |l| l.line()))
        }
    })?;
    let problems = config.validate();
    if problems.is_empty() {
        Ok(config)
    } else {
        Err(problems.join("；"))
    }
}

/// 按优先级找一个配置文件并加载。
/// 把 `target/debug/../../config` 这种相对路径整理成干净的绝对路径。
fn tidy(path: PathBuf) -> PathBuf {
    let absolute = std::fs::canonicalize(&path).unwrap_or(path);
    let text = absolute.to_string_lossy();
    match text.strip_prefix(r"\\?\") {
        Some(rest) => PathBuf::from(rest),
        None => absolute,
    }
}

pub fn load() -> Loaded {
    for path in candidates() {
        if !path.is_file() {
            continue;
        }
        let path = tidy(path);
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(err) => {
                return Loaded {
                    config: default_config(),
                    source: Source::Embedded,
                    path: Some(path.clone()),
                    error: Some(format!("读不了 {}：{err}", path.display())),
                };
            }
        };
        return match from_yaml(&text) {
            Ok(config) => Loaded {
                config: Arc::new(config),
                source: Source::File,
                path: Some(path),
                error: None,
            },
            Err(err) => {
                let path_text = path.display().to_string();
                Loaded {
                    config: default_config(),
                    source: Source::Embedded,
                    path: Some(path),
                    error: Some(format!("{path_text} 有问题：{err}")),
                }
            }
        };
    }

    Loaded {
        config: default_config(),
        source: Source::Embedded,
        path: None,
        error: None,
    }
}

fn candidates() -> Vec<PathBuf> {
    let mut out = Vec::new();

    if let Some(path) = std::env::var_os("XIYOU_OFFSETS") {
        out.push(PathBuf::from(path));
    }

    if let Some(dir) = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(PathBuf::from))
    {
        out.push(dir.join("config/offsets.yaml"));
        out.push(dir.join("../../config/offsets.yaml"));
    }

    if let Ok(cwd) = std::env::current_dir() {
        out.push(cwd.join("config/offsets.yaml"));
    }

    out
}

impl GameConfig {
    /// 静态校验。返回所有问题的说明，空表示没问题。
    pub fn validate(&self) -> Vec<String> {
        let mut problems = Vec::new();

        if self.schema_version != 1 {
            problems.push(format!(
                "schema_version 是 {}，本程序只认 1",
                self.schema_version
            ));
        }

        if self.emulators.is_empty() {
            problems.push("emulators 不能为空".to_string());
        }
        for (index, emu) in self.emulators.iter().enumerate() {
            if emu.exe.trim().is_empty() {
                problems.push(format!("emulators[{index}] 缺 exe"));
            }
            if self.emulators[..index].iter().any(|other| other.id == emu.id) {
                problems.push(format!("emulators 里 id `{}` 重复", emu.id));
            }
        }

        if self.layout.stride_player <= 0 {
            problems.push("layout.stride_player 必须大于 0".to_string());
        }
        if self.layout.stride_item <= 0 {
            problems.push("layout.stride_item 必须大于 0".to_string());
        }

        if self.counts.players == 0 {
            problems.push("counts.players 至少为 1".to_string());
        }
        if self.counts.players > 8 {
            problems.push("counts.players 最多 8（界面按 2 列排）".to_string());
        }
        if self.counts.enemies > 16 {
            problems.push("counts.enemies 最多 16".to_string());
        }

        if self.item_ring.entry_size <= 0 {
            problems.push("item_ring.entry_size 必须大于 0".to_string());
        }
        if self.item_ring.capacity != 8 {
            problems.push(format!(
                "item_ring.capacity 必须是 8（界面是 3×3 八边形槽位），当前 {}",
                self.item_ring.capacity
            ));
        }
        if self.item_ring.id_min > self.item_ring.id_max {
            problems.push("item_ring.id_min 大于 id_max".to_string());
        }

        // 玩家字段的读取宽度在 `PlayerSlot` 里是固定的，配置里写别的不会生效，
        // 所以直接报错，免得改完以为改掉了。
        for (key, field, expected) in [
            ("hp", &self.player_fields.hp, ValueType::U16),
            ("qi", &self.player_fields.qi, ValueType::U8),
            ("character", &self.player_fields.character, ValueType::U8),
            ("cursor", &self.player_fields.cursor, ValueType::U8),
            ("items", &self.player_fields.items, ValueType::U8),
            ("active", &self.player_fields.active, ValueType::U8),
        ] {
            if field.ty != expected {
                problems.push(format!(
                    "player_fields.{key}.type 必须是 {}，当前 {}",
                    expected.label(),
                    field.ty.label()
                ));
            }
        }

        for (key, field, expected) in [
            ("hp", &self.enemy_fields.hp, ValueType::U16),
            ("x", &self.enemy_fields.x, ValueType::U8),
            ("type", &self.enemy_fields.ty, ValueType::U32),
        ] {
            if field.ty != expected {
                problems.push(format!(
                    "enemy_fields.{key}.type 必须是 {}，当前 {}",
                    expected.label(),
                    field.ty.label()
                ));
            }
        }
        if self.player_fields.hp.rel != 0 {
            problems.push(format!(
                "player_fields.hp.rel 必须是 0（它就是锚点本身），当前 {}",
                self.player_fields.hp.rel
            ));
        }

        for (index, watch) in self.watches.iter().enumerate() {
            if watch.id.trim().is_empty() {
                problems.push(format!("watches[{index}] 缺 id"));
            }
            if self.watches[..index].iter().any(|other| other.id == watch.id) {
                problems.push(format!("watches 里 id `{}` 重复", watch.id));
            }
            if watch.address.deref.len() > 8 {
                problems.push(format!(
                    "watches `{}` 的 deref 超过 8 层，大概率写错了",
                    watch.id
                ));
            }
        }

        for (index, ty) in self.enemy_types.iter().enumerate() {
            if ty.name.trim().is_empty() {
                problems.push(format!("enemy_types[{index}] 缺 name"));
            }
            if ty.max == 0 {
                problems.push(format!("enemy_types[{index}] 的 max 必须大于 0"));
            }
            if self.enemy_types[..index].iter().any(|o| o.value == ty.value) {
                problems.push(format!(
                    "enemy_types 里 value 0x{:X} 重复",
                    ty.value
                ));
            }
        }

        problems
    }

    /// 生成一份可读的地址清单，给 `--dump-offsets` 用。
    pub fn describe(&self, ram_base: usize, module_base: usize) -> String {
        let mut out = String::new();
        out.push_str(&format!("游戏：{}\n", self.game));
        out.push_str(&format!(
            "假定 RAM 基址 0x{ram_base:08X}，模块基址 0x{module_base:08X}（仅用于预览，真实值要连上进程才有）\n\n"
        ));

        out.push_str("[模拟器]\n");
        for emu in &self.emulators {
            out.push_str(&format!(
                "  {:<10} {:<10} RAM 指针 = 模块基址 + 0x{:X}\n",
                emu.id, emu.exe, emu.ram_ptr_offset
            ));
            if !emu.desc.is_empty() {
                out.push_str(&format!("             {}\n", emu.desc));
            }
        }

        out.push_str("\n[布局]\n");
        out.push_str(&format!(
            "  RAM + 0x{:X}  剩余时间\n",
            self.layout.time
        ));
        out.push_str(&format!(
            "  RAM + 0x{:X}  1P 锚点（1P 血量）\n",
            self.layout.player_anchor
        ));
        out.push_str(&format!(
            "  RAM + 0x{:X}  1 号敌兵血量\n",
            self.layout.enemy_anchor
        ));
        out.push_str(&format!(
            "  间距：玩家 0x{:X}，道具 0x{:X}\n",
            self.layout.stride_player, self.layout.stride_item
        ));

        out.push_str("\n[玩家字段]\n");
        for (key, field) in [
            ("hp", &self.player_fields.hp),
            ("qi", &self.player_fields.qi),
            ("character", &self.player_fields.character),
            ("cursor", &self.player_fields.cursor),
            ("items", &self.player_fields.items),
            ("active", &self.player_fields.active),
        ] {
            out.push_str(&format!(
                "  {:<10} 锚点{}{:<9} 间距 {:<7} {:<4} {}\n",
                key,
                if field.rel < 0 { "-" } else { "+" },
                format!("0x{:X}", field.rel.unsigned_abs()),
                match field.scale {
                    Scale::Player => "player",
                    Scale::Item => "item",
                    Scale::None => "无",
                },
                field.ty.label(),
                field.desc
            ));
        }

        out.push_str("\n[敌兵字段]\n");
        for (key, field) in [
            ("hp", &self.enemy_fields.hp),
            ("x", &self.enemy_fields.x),
            ("type", &self.enemy_fields.ty),
        ] {
            out.push_str(&format!(
                "  {:<10} 锚点{}{:<9} 间距 {:<7} {:<4} {}\n",
                key,
                if field.rel < 0 { "-" } else { "+" },
                format!("0x{:X}", field.rel.unsigned_abs()),
                match field.scale {
                    Scale::Player => "player",
                    Scale::Item => "item",
                    Scale::None => "无",
                },
                field.ty.label(),
                field.desc
            ));
        }

        out.push_str("\n[敌兵类型对照]\n");
        // 名字列按表里最长的一条对齐，最短也留 10 列。
        let column = self
            .enemy_types
            .iter()
            .map(|ty| text_width(&ty.name))
            .max()
            .unwrap_or(10)
            .max(10);
        if self.enemy_types.is_empty() {
            out.push_str("  （表是空的，界面会直接把类型值显示成 6 位编号）\n");
        }
        for ty in &self.enemy_types {
            // 中文算两个字符宽，不然 --dump-offsets 的表格会歪。
            let width = text_width(&ty.name);
            let pad = " ".repeat(column.saturating_sub(width).max(1));
            out.push_str(&format!(
                "  0x{:06X}  {}{}满血 {}\n",
                ty.value, ty.name, pad, ty.max
            ));
        }

        out.push_str("\n[各槽位实际地址]\n");
        for index in 0..self.counts.players {
            let p = self.player_field_addr(ram_base, index, &self.player_fields.hp);
            out.push_str(&format!(
                "  {}P 血量 0x{p:08X}  气 0x{:08X}  角色 0x{:08X}  在场 0x{:08X}  光标 0x{:08X}  道具环 0x{:08X}\n",
                index + 1,
                self.player_field_addr(ram_base, index, &self.player_fields.qi),
                self.player_field_addr(ram_base, index, &self.player_fields.character),
                self.player_field_addr(ram_base, index, &self.player_fields.active),
                self.player_field_addr(ram_base, index, &self.player_fields.cursor),
                self.player_field_addr(ram_base, index, &self.player_fields.items),
            ));
        }
        for index in 0..self.counts.enemies {
            out.push_str(&format!(
                "  敌兵 {} 血量 0x{:08X}  坐标 0x{:08X}  类型 0x{:08X}\n",
                index + 1,
                self.enemy_field_addr(ram_base, index, &self.enemy_fields.hp),
                self.enemy_field_addr(ram_base, index, &self.enemy_fields.x),
                self.enemy_field_addr(ram_base, index, &self.enemy_fields.ty),
            ));
        }

        out.push_str("\n[自定义监视]\n");
        if self.watches.is_empty() {
            out.push_str("  （空。在 config/offsets.yaml 的 watches 里加条目）\n");
        }
        for watch in &self.watches {
            let start = self.start_addr(&watch.address, ram_base, module_base);
            let chain = if watch.address.deref.is_empty() {
                "直读".to_string()
            } else {
                format!("跳 {} 层指针", watch.address.deref.len())
            };
            out.push_str(&format!(
                "  {:<14} {:<5} 0x{start:08X} {chain}  {}\n",
                watch.title(),
                watch.ty.label(),
                watch.desc
            ));
        }

        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> GameConfig {
        from_yaml(EMBEDDED).expect("内置配置应当能解析")
    }

    #[test]
    fn 内置配置能解析且合法() {
        let problems = cfg().validate();
        assert!(problems.is_empty(), "内置配置有问题：{problems:?}");
    }

    #[test]
    fn 十六进制与十进制等价() {
        assert_eq!(parse_num("0x1BE98").unwrap(), 0x1BE98);
        assert_eq!(parse_num("114328").unwrap(), 114328);
        assert_eq!(parse_num("-0xACFC").unwrap(), -0xACFC);
        assert_eq!(parse_num(" 0X7E ").unwrap(), 0x7E);
        assert!(parse_num("xyz").is_err());
    }

    #[test]
    fn 玩家字段地址与原版一致() {
        let c = cfg();
        let ram = 0x1000_0000;
        let hp = c.player_field_addr(ram, 0, &c.player_fields.hp);
        assert_eq!(hp, ram + 0x1BE98);

        let qi = c.player_field_addr(ram, 0, &c.player_fields.qi);
        assert_eq!(qi - hp, 0xB5);

        let chara = c.player_field_addr(ram, 0, &c.player_fields.character);
        assert_eq!(hp - chara, 0x0C, "角色 ID 就在血量前面一点");

        let active = c.player_field_addr(ram, 0, &c.player_fields.active);
        assert_eq!(hp - active, 0x57, "在场标志是结构第一个字节");

        let cursor = c.player_field_addr(ram, 0, &c.player_fields.cursor);
        assert_eq!(hp - cursor, 0xACFC, "光标在血量前面");

        let items = c.player_field_addr(ram, 0, &c.player_fields.items);
        assert_eq!(hp - items, 0xAD0D, "道具环更靠前");
    }

    #[test]
    fn 关卡与时间地址与实测一致() {
        let c = cfg();
        let ram = 0x1000_0000;
        assert_eq!(c.time_addr(ram), ram + 0xC06F);
        assert_eq!(c.stage_addr(ram), ram + 0xC03B, "关卡号（0 基）实测在 0xC03B");
        assert_eq!(c.layout.stage_bias, 1, "0 基表要 +1 才是关卡号");
    }

    #[test]
    fn 敌兵字段地址与实测一致() {
        let c = cfg();
        let ram = 0x1000_0000;
        let hp = c.enemy_field_addr(ram, 0, &c.enemy_fields.hp);
        assert_eq!(hp, ram + 0x11A88);

        let active = c.enemy_field_addr(ram, 0, &c.enemy_fields.active);
        assert_eq!(active, ram + 0x11A31, "在场标志 = 0x11A88 - 0x57");
        assert_eq!(hp - active, 0x57, "和玩家表同一个位置规律");

        let active5 = c.enemy_field_addr(ram, 4, &c.enemy_fields.active);
        assert_eq!(active5 - active, 4 * 0x13E);
    }

    #[test]
    fn 两套间距各走各的() {
        let c = cfg();
        let ram = 0x1000_0000;
        let hp1 = c.player_field_addr(ram, 0, &c.player_fields.hp);
        let hp3 = c.player_field_addr(ram, 2, &c.player_fields.hp);
        assert_eq!(hp3 - hp1, 2 * 0x13E);

        let cur1 = c.player_field_addr(ram, 0, &c.player_fields.cursor);
        let cur3 = c.player_field_addr(ram, 2, &c.player_fields.cursor);
        assert_eq!(cur3 - cur1, 2 * 0x7E, "光标按 item 间距，不该叠加 player 间距");
    }

    #[test]
    fn 负偏移不绕成大数() {
        let c = cfg();
        let addrs = 0x1000;
        let items = c.player_field_addr(addrs, 0, &c.player_fields.items);
        assert!(items < addrs + 0x1BE98);
        assert_eq!(items, addrs + 0x1BE98 - 0xAD0D);
    }

    #[test]
    fn 道具环布局对得上() {
        let ring = cfg().item_ring;
        let base = 0x2000;
        assert_eq!(ring.id_addr(base, 1), base);
        assert_eq!(ring.count_addr(base, 1), base - 1);
        assert_eq!(ring.id_addr(base, 2) - ring.id_addr(base, 1), 2);
        assert_eq!(ring.total_addr(base), base + 18);
    }

    #[test]
    fn 监视项支持指针链与序号() {
        let c = cfg();
        let addr = Addr {
            base: Base::Ram,
            offset: 0x100,
            scale: Scale::Player,
            index: 2,
            deref: vec![0x10],
        };
        let start = c.start_addr(&addr, 0x1000_0000, 0);
        assert_eq!(start, 0x1000_0000 + 0x100 + 2 * 0x13E);
    }

    #[test]
    fn 缺字段会报错而不是静默放过() {
        let broken = r#"
schema_version: 1
emulators: []
layout:
  time: 0x1
  player_anchor: 0x2
  enemy_anchor: 0x3
  stride_player: 0x4
  stride_item: 0x5
player_fields:
  hp: { rel: 0x0 }
  qi: { rel: 0x1 }
  character: { rel: 0x2 }
  cursor: { rel: 0x3 }
"#;
        let err = from_yaml(broken).unwrap_err();
        assert!(err.contains("items"), "应当指出缺 items：{err}");
    }

    #[test]
    fn 容量不是八会报错() {
        let text = EMBEDDED.replace("capacity: 8", "capacity: 6");
        let err = from_yaml(&text).unwrap_err();
        assert!(err.contains("capacity"), "{err}");
    }

    #[test]
    fn 十六进制和十进制写法都认() {
        let text = EMBEDDED
            .replace("time: 0xC06F", "time: 49263")
            .replace("player_anchor: 0x1BE98", "player_anchor: 114328");
        let c = from_yaml(&text).expect("十进制应当等价于十六进制");
        assert_eq!(c.layout.time, 0xC06F);
        assert_eq!(c.layout.player_anchor, 0x1BE98);
    }
}
