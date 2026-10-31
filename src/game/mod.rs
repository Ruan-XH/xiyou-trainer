//! 游戏数据层：定位进程、算地址、读一份快照。
//!
//! 这一层完全不碰 UI，可以单独跑测试，也可以在命令行里复用。
//!
//! 所有偏移都来自 `config/offsets.yaml`（可执行文件旁边放一份就能覆盖内置值），
//! 代码里不再写死任何地址。

pub mod addresses;
pub mod config;
pub mod demo;
pub mod items;
pub mod snapshot;

#[allow(unused_imports)]
pub use config::{GameConfig, Loaded, Source};
#[allow(unused_imports)]
pub use snapshot::{
    AttachError, Character, EmulatorInfo, EnemySlot, GameReader, GameSnapshot, PlayerAddrs,
    PlayerSlot, ReadError, WatchRead, WatchValue,
};
