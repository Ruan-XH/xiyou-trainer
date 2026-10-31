//! 主界面：250×650 的竖版街机机台面板。
//!
//! 版式照设计稿分五块，高度是硬约束（12 + 72 + 4 + 8 + 4 + 273 + 4 + 24 + 4 + 233 + 12 = 650）：
//!
//! ```text
//!   y  12 .. 84    顶部区      标题 / STAGE 章 / 载入按钮 / 剩余时间
//!   y  88 .. 96    分隔线      左细线 + 菱形 + 右细线
//!   y 100 .. 373   敌兵区      标题行 20 + 6 行（行高 38、行间 5）
//!   y 377 .. 401   玩家标题行  「玩 家」+ 五档分段切换
//!   y 405 .. 638   玩家区      总览（4 行）或单人详情（身份 / 血 / 气 / 3×3 道具格）
//! ```
//!
//! 全是 `Painter` 自绘，不用 egui 的控件——设计稿每一块高度都是写死的，
//! 自己算坐标比跟布局系统较劲准得多。只有「载入」按钮和五档切换会接鼠标事件。
//!
//! 界面只做两件事：每 100ms 读一份快照，然后把快照画出来。

use std::sync::Arc;
use std::time::{Duration, Instant};

use eframe::egui::{
    self, Align2, Color32, CornerRadius, CursorIcon, FontFamily, FontId, Mesh, Rect, Sense, Shape,
    Stroke, StrokeKind, pos2, vec2,
};

use crate::game::config::{self, GameConfig};
use crate::game::items;
use crate::game::{GameReader, GameSnapshot, PlayerSlot};

/// 刷新间隔，和原版一致。
const POLL: Duration = Duration::from_millis(100);

/// 标志素材：`tools/make_icon.py` 从 `assets/logo_src.png` 生成的 64×64 裸 RGBA。
const ICON_SIDE: usize = 64;
const ICON_RGBA: &[u8] = include_bytes!("../assets/icon64.rgba");

/// 「载入」按钮上那个标志：墨色圆底半径 + 图形边长。
/// 图形比圆底小一圈，留出印面的边，看起来像一枚印章。
const MARK_BG: f32 = 13.0;
const MARK_LOGO: f32 = 21.0;

// ---------------------------------------------------------------- 画布与栅格

const CANVAS_W: f32 = 250.0;
const CANVAS_H: f32 = 650.0;

/// 左右内边距，内容宽 222。
const PAD_X: f32 = 14.0;
const CONTENT_W: f32 = 222.0;

const TOP_Y: f32 = 12.0;
/// 顶部区总高（标题行 20 + 间距 6 + 操作行 46）。
const TOP_H: f32 = 72.0;
/// 操作行（载入按钮 / 剩余时间卡）的高。
const TOP_ROW_H: f32 = 46.0;
const DIV_Y: f32 = 88.0;
const DIV_H: f32 = 8.0;
const ENEMY_Y: f32 = 100.0;
/// 敌兵区标题行高。
const ENEMY_HEAD_H: f32 = 20.0;
/// 敌兵单行高与行间。
const ENEMY_ROW_H: f32 = 38.0;
const ENEMY_GAP: f32 = 5.0;
const PHDR_Y: f32 = 377.0;
const PLAYER_Y: f32 = 405.0;
const PLAYER_H: f32 = 233.0;

// ---------------------------------------------------------------- 配色

const BG_TOP: Color32 = rgb(0x17, 0x11, 0x0D);
const BG_MID: Color32 = rgb(0x0B, 0x08, 0x06);
const BG_BOT: Color32 = rgb(0x06, 0x04, 0x03);

const CARD: Color32 = rgb(0x17, 0x12, 0x10);
const ROW_CARD: Color32 = rgb(0x13, 0x10, 0x10);
const CELL: Color32 = rgb(0x12, 0x0E, 0x0C);
/// 光标正停着的那一格，比普通格亮一档。
const CELL_CURSOR: Color32 = rgb(0x25, 0x1C, 0x13);
const CELL_EMPTY: Color32 = rgb(0x0D, 0x0A, 0x08);
const SEAL_BG: Color32 = rgb(0x1F, 0x17, 0x12);

/// 金，主。
const GOLD: Color32 = rgb(0xEB, 0xD6, 0xAC);
const GOLD_MID: Color32 = rgb(0xD8, 0xA4, 0x5B);
const GOLD_LIGHT: Color32 = rgb(0xF0, 0xCE, 0x8C);
const GOLD_DIM: Color32 = rgb(0xC9, 0xA4, 0x6E);
const SEAL_INK: Color32 = rgb(0xE8, 0xC9, 0x8C);

/// 金，弱（标签）。
const LABEL: Color32 = rgb(0x8D, 0x7B, 0x65);
const LABEL_SEAT: Color32 = rgb(0x8A, 0x6A, 0x3A);
const LABEL_DIM: Color32 = rgb(0x7A, 0x6A, 0x57);

/// 正文。
const TEXT: Color32 = rgb(0xF0, 0xE2, 0xC8);
const TEXT_KIND: Color32 = rgb(0xDC, 0xC9, 0xA8);
const TEXT_ITEM: Color32 = rgb(0xA9, 0x8A, 0x5E);
const TEXT_DEAD: Color32 = rgb(0x6E, 0x60, 0x53);

/// 血 / 气。
const HP_FILL: Color32 = rgb(0x47, 0x78, 0x5F);
const QI_FILL: Color32 = rgb(0x42, 0x6E, 0x88);
const TRACK: Color32 = rgb(0x25, 0x1D, 0x18);
const ENEMY_TRACK: Color32 = rgb(0x2A, 0x20, 0x1A);
const NUM_HP: Color32 = rgb(0x6E, 0x9C, 0x82);
const NUM_QI: Color32 = rgb(0x6A, 0x93, 0xAE);
const ENEMY_NUM: Color32 = rgb(0xB0, 0x8A, 0x55);
const DOT: Color32 = rgb(0x6A, 0x58, 0x44);

/// 分隔线、切换控件的描边。
const RULE: Color32 = rgb(0x3A, 0x2C, 0x20);
const TAB_BG: Color32 = rgb(0x14, 0x10, 0x10);
const TAB_ACTIVE: Color32 = rgb(0x2E, 0x24, 0x1B);
const TAB_ON: Color32 = rgb(0xEF, 0xCB, 0x86);
const TAB_OFF: Color32 = rgb(0x6E, 0x60, 0x53);

/// 低血量血条（越少越偏橙）。
const HP_HOT: Color32 = rgb(0xD9, 0x6A, 0x38);
const HP_WARM: Color32 = rgb(0xC2, 0x4C, 0x30);
const HP_NORM: Color32 = rgb(0xB2, 0x3F, 0x2C);
const HP_FULL: Color32 = rgb(0xA8, 0x3A, 0x2A);

const fn rgb(r: u8, g: u8, b: u8) -> Color32 {
    Color32::from_rgb(r, g, b)
}

/// 金色描边，`alpha` 是 0..=255 的不透明度。
fn gold(alpha: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(0xD9, 0xA8, 0x5C, alpha)
}

/// 0.0~1.0 的不透明度换算成字节，省得在调用处写 0.16 * 255。
fn a(ratio: f32) -> u8 {
    (ratio.clamp(0.0, 1.0) * 255.0).round() as u8
}

// ---------------------------------------------------------------- 字体

fn sans(size: f32) -> FontId {
    FontId::new(size, FontFamily::Proportional)
}

fn mono(size: f32) -> FontId {
    FontId::new(size, FontFamily::Monospace)
}

fn bold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("bold".into()))
}

// ---------------------------------------------------------------- 状态

/// 连接状态，只影响提示和按钮的颜色。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    /// 正在读数据。
    Live,
    /// 连上了，但 RAM 指针还是 0（还在标题画面 / 没进关）。
    Waiting,
    /// 没连上模拟器。
    Lost,
    /// 演示模式。
    Demo,
}

pub struct Trainer {
    config: Arc<GameConfig>,
    reader: Option<GameReader>,
    snapshot: Option<GameSnapshot>,
    /// 出问题时的原始错误，鼠标停在载入按钮上才显示。
    error: Option<String>,
    state: State,
    demo: bool,
    tick: u64,
    last_poll: Instant,
    /// 玩家区当前档位：0 = 总览，1..=4 = 对应玩家。
    tab: u8,
    /// 剩余时间见过的最大值，用来算进度条比例（不同关卡上限不一样）。
    time_peak: u8,
    /// 顶栏左边的标志，和窗口图标同源（`assets/logo_src.png`）。
    logo: egui::TextureHandle,
}

impl Trainer {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        install_fonts(&cc.egui_ctx);
        install_style(&cc.egui_ctx);

        let loaded = config::load();
        if let Some(err) = &loaded.error {
            eprintln!("配置有误，已退回内置副本：{err}");
        }

        let demo = std::env::var_os("XIYOU_DEMO").is_some();

        // 标志：64×64 的裸 RGBA 在编译期就进了代码段，这里只是上传成纹理。
        // 用 64 而不是 24，是因为后面放大到 24 时双线性采样还有余量，
        // 以后想把顶栏做大也不用重新生成素材。
        let logo = cc.egui_ctx.load_texture(
            "logo",
            egui::ColorImage::from_rgba_unmultiplied([ICON_SIDE, ICON_SIDE], ICON_RGBA),
            egui::TextureOptions::LINEAR,
        );

        Self {
            config: loaded.config,
            reader: None,
            snapshot: None,
            error: None,
            state: if demo { State::Demo } else { State::Waiting },
            demo,
            tick: 0,
            last_poll: Instant::now() - POLL,
            tab: 0,
            time_peak: 60,
            logo,
        }
    }

    /// 重新找一遍模拟器进程。界面上那个「载入」按钮就是干这个的。
    fn reconnect(&mut self) {
        self.reader = None;
        self.snapshot = None;
        self.error = None;
        self.state = State::Waiting;
        self.last_poll = Instant::now() - POLL;
    }

    /// 一次轮询：没连上就找进程，连上了读一份快照。
    fn poll(&mut self) {
        if self.last_poll.elapsed() < POLL {
            return;
        }
        self.last_poll = Instant::now();
        self.tick += 1;

        if self.demo {
            self.snapshot = Some(crate::game::demo::snapshot(self.tick));
            self.note_time();
            return;
        }

        if self.reader.is_none() {
            match GameReader::attach(self.config.clone()) {
                Ok(reader) => {
                    self.reader = Some(reader);
                    self.error = None;
                    self.state = State::Waiting;
                }
                Err(err) => {
                    self.error = Some(err.to_string());
                    self.snapshot = None;
                    self.state = State::Lost;
                    return;
                }
            }
        }

        let Some(reader) = self.reader.as_ref() else {
            return;
        };

        match reader.snapshot() {
            Ok(snapshot) => {
                self.snapshot = Some(snapshot);
                self.error = None;
                self.state = State::Live;
                self.note_time();
            }
            Err(err) => {
                // RAM 指针没准备好是常态（还在标题画面），进程本身没问题，保持句柄。
                // 其它错误一律丢掉句柄，下一轮重新附加。
                let fatal = !matches!(err, crate::game::ReadError::RamNotReady(_));
                self.error = Some(err.to_string());
                self.snapshot = None;
                if fatal {
                    self.reader = None;
                    self.state = State::Lost;
                } else {
                    self.state = State::Waiting;
                }
            }
        }
    }

    /// 记下剩余时间的峰值，用于进度条比例。
    fn note_time(&mut self) {
        if let Some(snapshot) = &self.snapshot {
            self.time_peak = self.time_peak.max(snapshot.time_left).max(1);
        }
    }

    fn hp_max(&self) -> f32 {
        self.config.limits.hp_max.max(1) as f32
    }

    fn qi_max(&self) -> f32 {
        self.config.limits.qi_max.max(1) as f32
    }

    fn player(&self, index: usize) -> Option<&PlayerSlot> {
        self.snapshot.as_ref().and_then(|s| s.players.get(index))
    }

    // ------------------------------------------------------------ 绘制

    fn draw(&mut self, ui: &mut egui::Ui) {
        let painter = ui.painter().clone();
        let canvas = Rect::from_min_size(ui.max_rect().min, vec2(CANVAS_W, CANVAS_H));
        let p = painter.with_clip_rect(canvas);

        // 底：三段竖直渐变。
        vgrad(&p, canvas, &[(0.0, BG_TOP), (0.42, BG_MID), (1.0, BG_BOT)]);

        // 顶部径向暖光。半径给到 200，落到 y≈200 时早已衰减为 0，
        // 所以裁切边界不会留下硬边。
        let glow_zone = Rect::from_min_size(canvas.min, vec2(CANVAS_W, 200.0));
        radial_glow(
            &p.with_clip_rect(glow_zone),
            canvas.min + vec2(CANVAS_W * 0.5, -20.0),
            200.0,
            gold(a(0.16)),
        );

        draw_frame(&p, canvas);
        self.draw_top(ui, &p, canvas);
        draw_divider(&p, canvas);
        self.draw_enemies(&p, canvas);
        self.draw_player_header(ui, &p, canvas);
        self.draw_player_area(&p, canvas);
    }

    // ---- 顶部区
    fn draw_top(&mut self, ui: &mut egui::Ui, p: &egui::Painter, canvas: Rect) {
        let left = canvas.left() + PAD_X;
        let right = canvas.right() - PAD_X;
        let top = canvas.top() + TOP_Y;

        spaced_text(
            p,
            left,
            top + 10.0,
            "西游释厄传",
            &bold(16.0),
            GOLD,
            1.2,
        );

        // 关卡章：宽度按文字量出来再定，右边缘钉死在 right，
        // 左边自己长，免得以后文字变长时挤出去。
        let badge_text = if self.demo {
            "DEMO".to_string()
        } else {
            self.snapshot
                .as_ref()
                .and_then(|s| s.stage_label())
                .unwrap_or_else(|| "STAGE --".to_string())
        };
        let font = mono(9.0);
        let text_w = spaced_width(p, &badge_text, &font, 1.0);
        let badge_w = (text_w + 14.0).max(52.0);
        let badge = Rect::from_min_size(pos2(right - badge_w, top + 2.0), vec2(badge_w, 16.0));
        p.rect_filled(badge, CornerRadius::same(2), gold(a(0.03)));
        p.rect_stroke(
            badge,
            CornerRadius::same(2),
            Stroke::new(1.0, gold(a(0.28))),
            StrokeKind::Inside,
        );
        spaced_text(
            p,
            badge.center().x - text_w * 0.5,
            badge.center().y,
            &badge_text,
            &font,
            GOLD_DIM,
            1.0,
        );

        // 操作行（46）。顶部区 72 = 标题行 20 + 间距 6 + 操作行 46。
        let btn = Rect::from_min_size(
            pos2(left, top + TOP_H - TOP_ROW_H),
            vec2(104.0, TOP_ROW_H),
        );
        let card = Rect::from_min_size(
            pos2(left + 112.0, top + TOP_H - TOP_ROW_H),
            vec2(110.0, TOP_ROW_H),
        );

        let resp = ui.interact(btn, egui::Id::unique("load"), Sense::click());
        let (hovered, clicked) = (resp.hovered(), resp.clicked());
        if hovered {
            ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
        }
        if clicked {
            self.reconnect();
        }
        let tip = match (&self.state, &self.error) {
            (State::Demo, _) => "演示模式（XIYOU_DEMO）".to_string(),
            (_, Some(err)) => format!("重新连接模拟器\n\n{err}"),
            (State::Live, None) => match &self.snapshot {
                Some(s) => format!(
                    "{} · PID {}\nRAM 0x{:08X}\n\n点击重新连接",
                    s.emulator.label, s.pid, s.ram_base
                ),
                None => "点击重新连接".to_string(),
            },
            (State::Waiting, None) => "已连上模拟器，等待进入游戏".to_string(),
            (State::Lost, None) => "未连接模拟器".to_string(),
        };
        drop(resp.on_hover_text(tip));

        // 金色金属渐变：四段，自上而下。
        vgrad(
            p,
            btn,
            &[
                (0.0, rgb(0xF4, 0xD7, 0x9E)),
                (0.35, rgb(0xD9, 0xA8, 0x5F)),
                (0.70, rgb(0xB8, 0x82, 0x3F)),
                (1.0, rgb(0x8E, 0x61, 0x28)),
            ],
        );
        if hovered {
            p.rect_filled(btn, CornerRadius::ZERO, Color32::from_white_alpha(18));
        }
        if self.state == State::Lost {
            p.rect_filled(btn, CornerRadius::ZERO, Color32::from_rgba_unmultiplied(0x8E, 0x20, 0x18, 70));
        }
        p.rect_stroke(
            btn,
            CornerRadius::ZERO,
            Stroke::new(1.0, gold(a(0.35))),
            StrokeKind::Inside,
        );

        // 内容整体居中：标志（深色圆底 + 图） + 「载 入」。
        // 标志本身是金色的，直接压在金色按钮上看不清，所以垫一个墨色圆底当作印面。
        let ink = rgb(0x2A, 0x1A, 0x08);
        let label = "载 入";
        let font = bold(16.5);
        let tw = spaced_width(p, label, &font, 2.0);
        let group = MARK_BG * 2.0 + 6.0 + tw;
        let start = btn.center().x - group * 0.5;
        // 印章比几何中心低 1px；文字仍留在原锚点，两者不共用中线。
        let mark_cy = btn.center().y + 1.0;
        let text_cy = btn.center().y;
        let mark_cx = start + MARK_BG;
        p.circle_filled(pos2(mark_cx, mark_cy), MARK_BG, ink);
        p.image(
            self.logo.id(),
            Rect::from_center_size(pos2(mark_cx, mark_cy), vec2(MARK_LOGO, MARK_LOGO)),
            Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
            Color32::WHITE,
        );
        // 汉字字面比 em 框靠下，锚点抬高一点才看着在正中。
        spaced_text(
            p,
            start + MARK_BG * 2.0 + 6.0,
            text_cy,
            label,
            &font,
            ink,
            2.0,
        );

        // 剩余时间卡 110×46
        p.rect_filled(card, CornerRadius::same(3), CARD);
        p.rect_stroke(
            card,
            CornerRadius::same(3),
            Stroke::new(1.0, gold(a(0.16))),
            StrokeKind::Inside,
        );
        let cx0 = card.left() + 8.0;
        let cx1 = card.right() - 8.0;
        p.text(pos2(cx0, card.top() + 9.0), Align2::LEFT_CENTER, "剩余时间", sans(9.5), LABEL);
        p.text(pos2(cx1, card.top() + 9.0), Align2::RIGHT_CENTER, "SEC", sans(9.5), LABEL);

        let (time_text, ratio) = match &self.snapshot {
            Some(s) => (
                format!("{}", s.time_left),
                (s.time_left as f32 / self.time_peak.max(1) as f32).clamp(0.0, 1.0),
            ),
            None => ("--".to_string(), 0.0),
        };
        p.text(
            pos2(cx1, card.top() + 28.0),
            Align2::RIGHT_CENTER,
            time_text,
            mono(23.0),
            GOLD_LIGHT,
        );
        let track = Rect::from_min_size(pos2(cx0, card.bottom() - 6.0), vec2(cx1 - cx0, 3.0));
        bar(p, track, ratio, GOLD_MID, ENEMY_TRACK);
    }

    // ---- 敌兵区
    fn draw_enemies(&self, p: &egui::Painter, canvas: Rect) {
        let left = canvas.left() + PAD_X;
        let right = canvas.right() - PAD_X;
        let top = canvas.top() + ENEMY_Y;

        spaced_text(p, left, top + 10.0, "敌 兵", &bold(12.0), GOLD_MID, 3.0);
        let slots = self
            .snapshot
            .as_ref()
            .map(|s| s.enemies.len())
            .unwrap_or(self.config.counts.enemies);
        // 右上角报的是「场上还有几只」，不是槽位总数：六个槽是固定的，
        // 报总数没信息量，玩家真正想知道的是眼前还剩几只。
        let live = self
            .snapshot
            .as_ref()
            .map(|s| s.enemies.iter().filter(|e| e.live()).count())
            .unwrap_or(0);
        p.text(
            pos2(right, top + 10.0),
            Align2::RIGHT_CENTER,
            format!("{live} 只"),
            sans(10.0),
            LABEL_DIM,
        );

        let mut y = top + ENEMY_HEAD_H;
        for index in 0..slots.min(6) {
            let row = Rect::from_min_size(pos2(left, y), vec2(CONTENT_W, ENEMY_ROW_H));
            self.draw_enemy_row(p, row, index);
            y += ENEMY_ROW_H + ENEMY_GAP;
        }
    }

    fn draw_enemy_row(&self, p: &egui::Painter, row: Rect, index: usize) {
        let slot = self.snapshot.as_ref().and_then(|s| s.enemies.get(index));
        let (name, hp, max, known, alive) = match slot {
            Some(e) => {
                let (name, max) = self.config.enemy_type(e.ty);
                (name, e.hp, max.max(1), e.ty != 0, e.alive())
            }
            None => ("—".to_string(), 0u16, 100u16, false, false),
        };
        let ratio = (hp as f32 / max as f32).clamp(0.0, 1.0);
        // 空槽要按「有没有活兵」判，不能只看血量：死槽的血量、种类全留着，
        // 光看 hp 会把上一波残留的 20 血当成一只在场的怪。
        let dead = !alive || !known || hp == 0;
        // 空槽连兵种名都不显示——那是上一波残留的种类，写出来照样误导。
        let name = if alive { name } else { "—".to_string() };

        p.rect_filled(row, CornerRadius::same(3), ROW_CARD);
        // 残血行的描边提亮，扫一眼就能看出哪个快死了。
        let edge = if !dead && ratio < 0.2 { 0.34 } else if dead { 0.06 } else { 0.10 };
        p.rect_stroke(
            row,
            CornerRadius::same(3),
            Stroke::new(1.0, gold(a(edge))),
            StrokeKind::Inside,
        );

        // 序号（和兵种名同一行，居中在字上而不是行的几何中心）
        p.text(
            pos2(row.left() + 15.0, row.center().y),
            Align2::CENTER_CENTER,
            format!("{:02}", index + 1),
            mono(12.0),
            DOT,
        );

        // 名称 + 血量。整行文字比几何中心高 4px——中文汉字的字面框偏下，
        // 纯按中心摆会看着「沉」在行里。
        let gx = row.left() + 28.0;
        let gr = row.right() - 8.0;
        let name_cy = row.top() + 13.5;
        let name_color = if dead { TEXT_DEAD } else { TEXT_KIND };
        p.text(pos2(gx, name_cy), Align2::LEFT_CENTER, name, sans(14.0), name_color);

        // 血量数字单独一个锚点：字号比兵种名小一档，视觉重心也比汉字低，
        // 要和行内其它字看着齐平就得单独往下压 2px。
        let hp_cy = name_cy + 2.0;

        if dead {
            p.text(
                pos2(gr, hp_cy),
                Align2::RIGHT_CENTER,
                "---",
                mono(14.5),
                TEXT_DEAD,
            );
        } else {
            p.text(
                pos2(gr, hp_cy),
                Align2::RIGHT_CENTER,
                format!("{hp:>3}"),
                mono(14.5),
                ENEMY_NUM,
            );
        }

        let track = Rect::from_min_size(pos2(gx, row.top() + 29.0), vec2(gr - gx, 3.0));
        if dead {
            p.rect_filled(track, CornerRadius::same(2), ENEMY_TRACK);
        } else {
            let fill = if ratio < 0.15 {
                HP_HOT
            } else if ratio < 0.4 {
                HP_WARM
            } else if ratio < 0.7 {
                HP_NORM
            } else {
                HP_FULL
            };
            bar(p, track, ratio, fill, ENEMY_TRACK);
        }
    }

    // ---- 玩家标题行 + 档位切换
    fn draw_player_header(&mut self, ui: &mut egui::Ui, p: &egui::Painter, canvas: Rect) {
        let left = canvas.left() + PAD_X;
        let right = canvas.right() - PAD_X;
        let top = canvas.top() + PHDR_Y;
        let cy = top + 11.0;

        spaced_text(p, left, cy, "玩 家", &bold(12.0), GOLD_MID, 2.0);

        // 128×22 的分段控件：ALL 26 + 4×25 + 左右各 1 内边距。
        let ctl = Rect::from_min_size(pos2(right - 128.0, top + 1.0), vec2(128.0, 22.0));
        p.rect_filled(ctl, CornerRadius::same(2), TAB_BG);
        p.rect_stroke(
            ctl,
            CornerRadius::same(2),
            Stroke::new(1.0, RULE),
            StrokeKind::Inside,
        );

        let labels = ["ALL", "1P", "2P", "3P", "4P"];
        let mut x = ctl.left() + 1.0;
        for (index, label) in labels.iter().enumerate() {
            let width = if index == 0 { 26.0 } else { 25.0 };
            let seg = Rect::from_min_size(pos2(x, ctl.top() + 1.0), vec2(width, ctl.height() - 2.0));
            let active = self.tab as usize == index;

            if active {
                p.rect_filled(seg, CornerRadius::same(1), TAB_ACTIVE);
            }
            let font = mono(if index == 0 { 10.5 } else { 11.0 });
            p.text(
                seg.center(),
                Align2::CENTER_CENTER,
                *label,
                font,
                if active { TAB_ON } else { TAB_OFF },
            );

            let resp = ui.interact(
                seg,
                egui::Id::unique(("ptab", index)),
                Sense::click(),
            );
            if resp.hovered() {
                ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
            }
            if resp.clicked() {
                self.tab = index as u8;
            }
            x += width;
        }
    }

    // ---- 玩家区
    fn draw_player_area(&self, p: &egui::Painter, canvas: Rect) {
        let card = Rect::from_min_size(
            pos2(canvas.left() + PAD_X, canvas.top() + PLAYER_Y),
            vec2(CONTENT_W, PLAYER_H),
        );
        p.rect_filled(card, CornerRadius::same(2), CARD);
        p.rect_stroke(
            card,
            CornerRadius::same(2),
            Stroke::new(1.0, gold(a(0.16))),
            StrokeKind::Inside,
        );

        if self.tab == 0 {
            self.draw_overview(p, card);
        } else {
            self.draw_detail(p, card, self.tab as usize - 1);
        }
    }

    /// 状态 A：四位玩家总览。
    fn draw_overview(&self, p: &egui::Painter, card: Rect) {
        let inner = card.shrink(7.0);
        let row_h = 44.0;
        let gap = 9.0;
        let total = 4.0 * row_h + 3.0 * gap;
        let mut y = inner.top() + ((inner.height() - total) * 0.5).max(0.0);

        for index in 0..4 {
            let row = Rect::from_min_size(pos2(inner.left(), y), vec2(inner.width(), row_h));
            let slot = self.player(index);
            let occupied = slot.map(|s| s.occupied()).unwrap_or(false);

            p.rect_filled(
                row,
                CornerRadius::same(2),
                if occupied { ROW_CARD } else { CELL_EMPTY },
            );
            p.rect_stroke(
                row,
                CornerRadius::same(2),
                Stroke::new(1.0, gold(a(if occupied { 0.08 } else { 0.04 }))),
                StrokeKind::Inside,
            );

            let l = row.left() + 6.0;
            let r = row.right() - 6.0;
            let info_cy = row.top() + 11.0;

            p.text(
                pos2(l, info_cy + 4.0),
                Align2::LEFT_CENTER,
                format!("{}P", index + 1),
                mono(12.5),
                if occupied { LABEL_SEAT } else { DOT },
            );

            if !occupied {
                // 空位行没有血条，整行都是空的，字就摆在行心上。
                // 汉字按 CENTER 对齐时字面框偏下，上挪 2px 才看着正。
                p.text(
                    pos2(l + 19.0, row.center().y - 2.0),
                    Align2::LEFT_CENTER,
                    "空位",
                    sans(15.0),
                    TEXT_DEAD,
                );
                y += row_h + gap;
                continue;
            }

            let Some(slot) = slot else {
                y += row_h + gap;
                continue;
            };

            p.text(
                pos2(l + 19.0, info_cy + 3.0),
                Align2::LEFT_CENTER,
                slot.label(),
                bold(16.5),
                TEXT,
            );
            p.text(
                pos2(r, info_cy + 1.0),
                Align2::RIGHT_CENTER,
                slot.cursor_cell().map(|c| c.name).unwrap_or("无"),
                sans(13.0),
                TEXT_ITEM,
            );

            // 双槽行：血槽 + 数值 + 气槽 + 数值。总宽 194。
            let bar_cy = row.top() + 32.0;
            let hp_ratio = (slot.hp as f32 / self.hp_max()).clamp(0.0, 1.0);
            let qi_ratio = (slot.qi as f32 / self.qi_max()).clamp(0.0, 1.0);

            bar(
                p,
                Rect::from_min_size(pos2(l, bar_cy - 1.5), vec2(66.0, 3.0)),
                hp_ratio,
                HP_FILL,
                TRACK,
            );
            p.text(
                pos2(l + 92.0, bar_cy),
                Align2::RIGHT_CENTER,
                format!("{}", slot.hp),
                mono(12.0),
                NUM_HP,
            );
            bar(
                p,
                Rect::from_min_size(pos2(l + 102.0, bar_cy - 1.5), vec2(66.0, 3.0)),
                qi_ratio,
                QI_FILL,
                TRACK,
            );
            p.text(
                pos2(r, bar_cy),
                Align2::RIGHT_CENTER,
                format!("{}", slot.qi),
                mono(12.0),
                NUM_QI,
            );

            y += row_h + gap;
        }
    }

    /// 状态 B：单人详情（身份 / 血 / 气 / 3×3 道具格）。
    fn draw_detail(&self, p: &egui::Painter, card: Rect, index: usize) {
        let inner = card.shrink(10.0);
        let x0 = inner.left();
        let x1 = inner.right();
        let y0 = inner.top();

        let slot = self.player(index).filter(|s| s.occupied());
        let Some(slot) = slot else {
            p.text(
                card.center(),
                Align2::CENTER_CENTER,
                "该位置无人",
                sans(14.0),
                TEXT_DEAD,
            );
            return;
        };

        // 身份行
        let seal = pos2(x0 + 15.0, y0 + 16.0);
        p.circle_filled(seal, 15.0, SEAL_BG);
        p.circle_stroke(seal, 15.0, Stroke::new(1.0, gold(a(0.35))));
        let initial: String = slot.label().chars().take(1).collect();
        // 汉字字面框本身比 em 框偏下，居中要靠上挪一点才看着正。
        p.text(pos2(seal.x, seal.y - 1.0), Align2::CENTER_CENTER, initial, bold(14.5), SEAL_INK);

        let name_font = bold(16.5);
        // 名字和后面的「1P」共用一个基线，整体比印章中心上挪 1px。
        let name_cy = seal.y - 1.0;
        p.text(
            pos2(x0 + 34.0, name_cy),
            Align2::LEFT_CENTER,
            slot.label(),
            name_font.clone(),
            TEXT,
        );
        let name_w = p
            .layout_no_wrap(slot.label().to_string(), name_font, TEXT)
            .size()
            .x;
        p.text(
            pos2(x0 + 34.0 + name_w + 8.0, name_cy),
            Align2::LEFT_CENTER,
            format!("{}P", index + 1),
            mono(11.0),
            LABEL,
        );

        // 当前道具：总览里地方小只给名字，详情页把数量一起带上。
        // 它和下面道具格里的高亮是同一份数据——都是光标正指着的那件。
        p.text(
            pos2(x1, y0 + 8.0),
            Align2::RIGHT_CENTER,
            "当前道具",
            sans(9.5),
            LABEL_DIM,
        );
        match slot.cursor_cell() {
            Some(item) => {
                p.text(
                    pos2(x1, y0 + 25.0),
                    Align2::RIGHT_CENTER,
                    format!("{} ×{}", item.name, item.count),
                    sans(13.0),
                    GOLD_LIGHT,
                );
            }
            None => {
                p.text(
                    pos2(x1, y0 + 25.0),
                    Align2::RIGHT_CENTER,
                    "—",
                    sans(13.0),
                    TEXT_DEAD,
                );
            }
        }

        // 血行 / 气行
        self.draw_stat_row(p, x0, x1, y0 + 48.0, "血", slot.hp as f32 / self.hp_max(), HP_FILL, &format!("{}", slot.hp), NUM_HP);
        self.draw_stat_row(p, x0, x1, y0 + 70.0, "气", slot.qi as f32 / self.qi_max(), QI_FILL, &format!("{}", slot.qi), NUM_QI);

        // 道具格（3×3，格 36 高、纵向间距 6，正中留空）。
        // 具体哪一格摆第几件由道具总数决定——游戏里道具的落点不按顺时针排，
        // 同一件道具在 2 件和 3 件时位置都不一样，表在 items::ring_slots。
        let cells = items::ring_slots(slot.item_total);
        let grid_top = y0 + 82.0;
        let cell_w = (inner.width() - 8.0) / 3.0;
        for row in 0..3 {
            for col in 0..3 {
                let idx = row * 3 + col;
                let cell = Rect::from_min_size(
                    pos2(x0 + col as f32 * (cell_w + 4.0), grid_top + row as f32 * 42.0),
                    vec2(cell_w, 36.0),
                );
                // 这格摆的是第几件（1 基）；中排正中永远不摆。
                let nth = if idx == items::RING_CELL_CENTER {
                    None
                } else {
                    cells[idx]
                };
                let item = nth.and_then(|k| slot.items[(k - 1) as usize].clone());
                // 光标正指着的那件：亮一档，跟右上角的「当前道具」对上。
                let is_cursor = nth.is_some() && slot.cursor_slot == nth;

                match item {
                    Some(item) => {
                        p.rect_filled(
                            cell,
                            CornerRadius::same(2),
                            if is_cursor { CELL_CURSOR } else { CELL },
                        );
                        p.rect_stroke(
                            cell,
                            CornerRadius::same(2),
                            Stroke::new(1.0, gold(a(if is_cursor { 0.45 } else { 0.08 }))),
                            StrokeKind::Inside,
                        );
                        p.text(
                            pos2(cell.left() + 5.0, cell.top() + 13.0),
                            Align2::LEFT_CENTER,
                            item.name,
                            sans(12.0),
                            if is_cursor { TEXT } else { TEXT_KIND },
                        );
                        p.text(
                            pos2(cell.right() - 5.0, cell.top() + 27.0),
                            Align2::RIGHT_CENTER,
                            format!("×{}", item.count),
                            mono(10.0),
                            if is_cursor { GOLD_LIGHT } else { TEXT_ITEM },
                        );
                    }
                    None => {
                        p.rect_filled(cell, CornerRadius::same(2), CELL_EMPTY);
                        p.rect_stroke(
                            cell,
                            CornerRadius::same(2),
                            Stroke::new(1.0, gold(a(0.04))),
                            StrokeKind::Inside,
                        );
                    }
                }
            }
        }
    }

    /// 详情页的一行：标签 + 3px 细槽 + 右对齐数值。
    #[allow(clippy::too_many_arguments)]
    fn draw_stat_row(
        &self,
        p: &egui::Painter,
        x0: f32,
        x1: f32,
        cy: f32,
        label: &str,
        ratio: f32,
        fill: Color32,
        value: &str,
        num_color: Color32,
    ) {
        p.text(pos2(x0, cy), Align2::LEFT_CENTER, label, sans(11.0), LABEL_DIM);
        let bx0 = x0 + 18.0;
        let bx1 = x1 - 32.0;
        bar(
            p,
            Rect::from_min_size(pos2(bx0, cy - 1.5), vec2((bx1 - bx0).max(1.0), 3.0)),
            ratio.clamp(0.0, 1.0),
            fill,
            TRACK,
        );
        p.text(
            pos2(x1, cy),
            Align2::RIGHT_CENTER,
            value,
            mono(12.0),
            num_color,
        );
    }
}

impl eframe::App for Trainer {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll();
        ctx.request_repaint_after(POLL);
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        ui.set_min_size(vec2(CANVAS_W, CANVAS_H));
        self.draw(ui);
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        BG_BOT.to_normalized_gamma_f32()
    }
}

// ---------------------------------------------------------------- 绘制小件

/// 竖直渐变。`stops` 按 t 递增给，t 是 0..=1 的高度比例。
fn vgrad(p: &egui::Painter, rect: Rect, stops: &[(f32, Color32)]) {
    for pair in stops.windows(2) {
        let (t0, c0) = pair[0];
        let (t1, c1) = pair[1];
        if t1 <= t0 {
            continue;
        }
        let y0 = rect.top() + rect.height() * t0.clamp(0.0, 1.0);
        let y1 = rect.top() + rect.height() * t1.clamp(0.0, 1.0);

        let mut mesh = Mesh::default();
        mesh.colored_vertex(pos2(rect.left(), y0), c0);
        mesh.colored_vertex(pos2(rect.right(), y0), c0);
        mesh.colored_vertex(pos2(rect.left(), y1), c1);
        mesh.colored_vertex(pos2(rect.right(), y1), c1);
        mesh.add_triangle(0, 1, 2);
        mesh.add_triangle(1, 3, 2);
        p.add(Shape::mesh(mesh));
    }
}

/// 径向渐变：中心一个颜色，边缘透明。用来做顶部暖光。
fn radial_glow(p: &egui::Painter, center: egui::Pos2, radius: f32, inner: Color32) {
    const SEGMENTS: usize = 64;
    let mut mesh = Mesh::default();
    mesh.colored_vertex(center, inner);
    for i in 0..=SEGMENTS {
        let angle = i as f32 / SEGMENTS as f32 * std::f32::consts::TAU;
        let pos = center + vec2(angle.cos(), angle.sin()) * radius;
        mesh.colored_vertex(pos, Color32::TRANSPARENT);
    }
    for i in 0..SEGMENTS {
        mesh.add_triangle(0, 1 + i as u32, 2 + i as u32);
    }
    p.add(Shape::mesh(mesh));
}

/// 一根 3px 的条：先铺槽底，再按比例填。
fn bar(p: &egui::Painter, rect: Rect, ratio: f32, fill: Color32, track: Color32) {
    let radius = CornerRadius::same((rect.height() * 0.5) as u8);
    p.rect_filled(rect, radius, track);
    let ratio = ratio.clamp(0.0, 1.0);
    if ratio <= 0.0 {
        return;
    }
    let width = (rect.width() * ratio).max(rect.height());
    let filled = Rect::from_min_size(rect.min, vec2(width, rect.height()));
    p.rect_filled(filled, radius, fill);
}

/// 面板内框。四角那四个 L 形角标去掉了——远看像是给屏幕定位用的小方框。
fn draw_frame(p: &egui::Painter, canvas: Rect) {
    let inner = Rect::from_min_size(
        canvas.min + vec2(5.0, 5.0),
        vec2(CANVAS_W - 10.0, CANVAS_H - 10.0),
    );
    p.rect_stroke(
        inner,
        CornerRadius::same(2),
        Stroke::new(1.0, gold(a(0.10))),
        StrokeKind::Inside,
    );
}

/// 顶部区下面的分隔线：左细线 + 菱形 + 右细线。
fn draw_divider(p: &egui::Painter, canvas: Rect) {
    let left = canvas.left() + PAD_X;
    let right = canvas.right() - PAD_X;
    let cy = canvas.top() + DIV_Y + DIV_H * 0.5;
    let cx = canvas.center().x;
    let half = 3.5;

    let stroke = Stroke::new(1.0, RULE);
    p.line_segment([pos2(left, cy), pos2(cx - half - 4.0, cy)], stroke);
    p.line_segment([pos2(cx + half + 4.0, cy), pos2(right, cy)], stroke);

    let diamond = vec![
        pos2(cx, cy - half),
        pos2(cx + half, cy),
        pos2(cx, cy + half),
        pos2(cx - half, cy),
    ];
    p.add(Shape::convex_polygon(diamond, GOLD_DIM, Stroke::NONE));
}

// ---------------------------------------------------------------- 字距

/// 单个字符的宽度。
fn char_width(p: &egui::Painter, ch: char, font: &FontId) -> f32 {
    p.layout_no_wrap(ch.to_string(), font.clone(), Color32::WHITE)
        .size()
        .x
}

/// 带字距的整串宽度。egui 没有 letter-spacing，只能自己一个个量。
fn spaced_width(p: &egui::Painter, text: &str, font: &FontId, spacing: f32) -> f32 {
    let chars: Vec<char> = text.chars().collect();
    if chars.is_empty() {
        return 0.0;
    }
    let sum: f32 = chars.iter().map(|c| char_width(p, *c, font)).sum();
    sum + spacing * (chars.len() - 1) as f32
}

/// 逐字绘制，模拟字距。`left` 是首个字的左边缘，`cy` 是垂直中心。
fn spaced_text(
    p: &egui::Painter,
    left: f32,
    cy: f32,
    text: &str,
    font: &FontId,
    color: Color32,
    spacing: f32,
) {
    let mut x = left;
    for ch in text.chars() {
        let s = ch.to_string();
        let width = char_width(p, ch, font);
        p.text(pos2(x, cy), Align2::LEFT_CENTER, s, font.clone(), color);
        x += width + spacing;
    }
}

// ---------------------------------------------------------------- 主题

/// 正文候选中文字体。按顺序试，第一个读得出来的就用。
///
/// 首选黑体：小字号（10~14px）下笔画干净、不糊，雅黑压到这么小吃不住。
/// 黑体从 XP 起就长在每台 Windows 上，换个机器看也是同一副面孔。
/// 元组是（路径, 字体集合里的序号）。
const CJK_FACES: &[(&str, u32)] = &[
    ("C:/Windows/Fonts/simhei.ttf", 0),
    ("C:/Windows/Fonts/Noto Sans SC (TrueType).otf", 0),
    ("C:/Windows/Fonts/NotoSansSC-VF.ttf", 0),
    ("C:/Windows/Fonts/msyh.ttc", 0),
    ("C:/Windows/Fonts/simsun.ttc", 0),
];

/// 重字（标题、名字）用的字体。
///
/// 黑体没有独立的粗体文件，标题就靠字号（16.5 vs 12）和颜色区分；
/// 硬塞一份雅黑粗 / Noto Bold 会和正文的笔画风格打架，所以整页只用一套。
const CJK_BOLD_FACES: &[(&str, u32)] = &[
    ("C:/Windows/Fonts/simhei.ttf", 0),
    ("C:/Windows/Fonts/Noto Sans SC Bold (TrueType).otf", 0),
    ("C:/Windows/Fonts/Noto Sans SC Medium (TrueType).otf", 0),
    ("C:/Windows/Fonts/msyhbd.ttc", 0),
];

/// 数字用等宽，血量跳动时不会左右抖。
const DIGIT_FACES: &[(&str, u32)] = &[
    ("C:/Windows/Fonts/consola.ttf", 0),
    ("C:/Windows/Fonts/cour.ttf", 0),
];

/// 从候选表里挑第一个能读到的。
fn pick_face(faces: &[(&'static str, u32)]) -> Option<(Vec<u8>, u32, &'static str)> {
    faces.iter().find_map(|(path, index)| {
        std::fs::read(path)
            .ok()
            .map(|bytes| (bytes, *index, *path))
    })
}

/// egui 自带的字体没有汉字。这里喂三套系统字体：
///
/// * 正文 / 标签 → 黑体（没有就 Noto Sans SC / 雅黑 / 宋体）
/// * 数字       → Consolas 等宽（`Monospace`），跳动不抖
/// * 标题 / 重字 → 同正文一套，见 `CJK_BOLD_FACES`
///
/// 全都没有时汉字会显示成方块，但程序还能跑。
fn install_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();

    let cjk = pick_face(CJK_FACES);
    let cjk_bold = pick_face(CJK_BOLD_FACES);
    let digits = pick_face(DIGIT_FACES);

    let Some((cjk_bytes, cjk_index, cjk_path)) = cjk else {
        eprintln!("没找到中文字体（黑体 / Noto Sans SC / 雅黑 / 宋体），界面里的汉字会显示成方块");
        return;
    };
    let name_of = |path: &str| {
        path.rsplit('/')
            .next()
            .unwrap_or(path)
            .to_string()
    };
    eprintln!(
        "字体：正文 {} / 重字 {} / 数字 {}",
        name_of(cjk_path),
        cjk_bold
            .as_ref()
            .map(|(_, _, path)| name_of(path))
            .unwrap_or_else(|| "（无，用正文字重）".to_string()),
        digits
            .as_ref()
            .map(|(_, _, path)| name_of(path))
            .unwrap_or_else(|| "（无，用正文）".to_string()),
    );

    // `Blob` 是 Arc<dyn AsRef<[u8]>>，包一层 Arc 就不用再拷一遍。
    fonts.font_data.insert(
        "cjk".to_owned(),
        Arc::new(egui::FontData::from_blob(
            Arc::new(cjk_bytes),
            cjk_index,
        )),
    );

    let mut has_bold = false;
    if let Some((bold_bytes, bold_index, _)) = cjk_bold {
        fonts.font_data.insert(
            "cjk_bold".to_owned(),
            Arc::new(egui::FontData::from_blob(
                Arc::new(bold_bytes),
                bold_index,
            )),
        );
        has_bold = true;
    }

    let mut has_digits = false;
    if let Some((digit_bytes, digit_index, _)) = digits {
        fonts.font_data.insert(
            "digits".to_owned(),
            Arc::new(egui::FontData::from_blob(
                Arc::new(digit_bytes),
                digit_index,
            )),
        );
        has_digits = true;
    }

    // 正文：中文字体优先，后面保留 egui 自带的拉丁字体兜底。
    fonts
        .families
        .entry(FontFamily::Proportional)
        .or_default()
        .insert(0, "cjk".to_owned());

    // 等宽：先数字字体，再中文字体（Consolas 没有汉字），最后是自带的等宽字体。
    let monospace = fonts.families.entry(FontFamily::Monospace).or_default();
    if has_digits {
        monospace.insert(0, "digits".to_owned());
    }
    monospace.insert(if has_digits { 1 } else { 0 }, "cjk".to_owned());

    // 重字：有粗体用粗体，没有就用普通字重顶。
    let mut heavy = Vec::new();
    if has_bold {
        heavy.push("cjk_bold".to_owned());
    }
    heavy.push("cjk".to_owned());
    fonts
        .families
        .insert(FontFamily::Name("bold".into()), heavy);

    ctx.set_fonts(fonts);
}

/// 深色主题，关掉所有默认圆角和间距——这套界面自己算坐标。
fn install_style(ctx: &egui::Context) {
    ctx.set_theme(egui::Theme::Dark);
    ctx.all_styles_mut(|style| {
        style.visuals = egui::Visuals::dark();
        style.visuals.panel_fill = BG_BOT;
        style.visuals.window_fill = BG_BOT;
        style.spacing.item_spacing = vec2(0.0, 0.0);
        style.spacing.button_padding = vec2(0.0, 0.0);
        // 亚像素定位会让小字边缘糊一层灰，这里关掉，代价是字距略生硬。
        style.visuals.text_options.subpixel_binning = false;
    });
}
