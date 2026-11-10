// FiveM Launcher - Rust + egui (UI เนทีฟ ไม่มีเบราว์เซอร์ในตัว กิน RAM น้อย)
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use eframe::egui::{self, pos2, vec2, Align, Align2, Color32, Pos2, RichText, Sense, Stroke};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::{Duration, Instant};

const DEFAULT_ADDR: &str = "89.38.101.183:30120";
const REFRESH_EVERY: Duration = Duration::from_secs(30);

const BG: Color32 = Color32::from_rgb(10, 12, 22);
const ROW: Color32 = Color32::from_rgb(20, 23, 38);
const ROW_LINE: Color32 = Color32::from_rgb(38, 42, 64);
const TEXT_DIM: Color32 = Color32::from_rgb(130, 136, 160);
const ACCENT: Color32 = Color32::from_rgb(92, 102, 255);
const GREEN: Color32 = Color32::from_rgb(34, 197, 94);
const AMBER: Color32 = Color32::from_rgb(245, 179, 1);
const RED: Color32 = Color32::from_rgb(239, 68, 68);
const GOLD: Color32 = Color32::from_rgb(250, 190, 40);

// ---------- ข้อมูล ----------
#[derive(Serialize, Deserialize, Clone, Default)]
struct Server {
    #[serde(default)]
    id: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    address: String,
    #[serde(default)]
    fav: bool,
}

#[derive(Serialize, Deserialize, Default)]
struct State {
    #[serde(default)]
    servers: Vec<Server>,
    #[serde(default)]
    last: Option<String>,
}

fn state_path() -> PathBuf {
    let base = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("FiveMLauncher").join("servers.json")
}

fn load_state() -> State {
    let parsed = std::fs::read_to_string(state_path())
        .ok()
        .and_then(|s| serde_json::from_str::<State>(&s).ok());
    match parsed {
        Some(s) => s,
        None => State {
            servers: vec![Server {
                id: "default1".into(),
                name: String::new(),
                address: DEFAULT_ADDR.into(),
                fav: false,
            }],
            last: None,
        },
    }
}

fn save_state(s: &State) {
    let p = state_path();
    if let Some(d) = p.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    if let Ok(j) = serde_json::to_string_pretty(s) {
        let _ = std::fs::write(p, j);
    }
}

fn normalize(addr: &str) -> Option<String> {
    let mut a = addr.trim();
    for p in ["fivem://connect/", "http://", "https://"] {
        if let Some(r) = a.strip_prefix(p) {
            a = r;
        }
    }
    let a = a.trim_end_matches('/');
    let ok = (3..=120).contains(&a.len())
        && a.chars()
            .all(|c| c.is_ascii_alphanumeric() || "._:/-".contains(c));
    ok.then(|| a.to_string())
}

fn clean(s: &str) -> String {
    let mut out = String::new();
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        if c == '^' && it.peek().map_or(false, |n| n.is_ascii_digit()) {
            it.next();
            continue;
        }
        out.push(c);
    }
    out.trim().to_string()
}

// ---------- สถานะเซิร์ฟเวอร์ ----------
struct IconData {
    w: usize,
    h: usize,
    rgba: Vec<u8>,
}

struct Status {
    online: bool,
    name: String,
    players: Option<u64>,
    max: Option<u64>,
    ping: u32,
    icon: Option<IconData>,
}

fn offline() -> Status {
    Status {
        online: false,
        name: String::new(),
        players: None,
        max: None,
        ping: 0,
        icon: None,
    }
}

fn get_json(url: &str, secs: u64) -> Option<(serde_json::Value, u32)> {
    let t = Instant::now();
    let resp = ureq::get(url)
        .set("User-Agent", "FiveM-Launcher/1.0")
        .timeout(Duration::from_secs(secs))
        .call()
        .ok()?;
    let v: serde_json::Value = resp.into_json().ok()?;
    Some((v, t.elapsed().as_millis() as u32))
}

fn decode_icon(b64: &str) -> Option<IconData> {
    use base64::Engine;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(b64.trim())
        .ok()?;
    let img = image::load_from_memory(&bytes).ok()?.to_rgba8();
    Some(IconData {
        w: img.width() as usize,
        h: img.height() as usize,
        rgba: img.into_raw(),
    })
}

fn fetch_status(address: &str, want_icon: bool) -> Status {
    if address.contains("cfx.re/join/") {
        let code = address.rsplit('/').next().unwrap_or("");
        let url = format!("https://servers-frontend.fivem.net/api/servers/single/{code}");
        return match get_json(&url, 6) {
            Some((v, ms)) => {
                let d = &v["Data"];
                Status {
                    online: true,
                    name: d["hostname"].as_str().unwrap_or("").to_string(),
                    players: d["clients"].as_u64(),
                    max: d["sv_maxclients"].as_u64(),
                    ping: ms,
                    icon: None,
                }
            }
            None => offline(),
        };
    }

    let host = if address.contains(':') {
        address.to_string()
    } else {
        format!("{address}:30120")
    };

    if let Some((v, ms)) = get_json(&format!("http://{host}/dynamic.json"), 3) {
        let icon = if want_icon {
            get_json(&format!("http://{host}/info.json"), 3)
                .and_then(|(i, _)| i["icon"].as_str().and_then(decode_icon))
        } else {
            None
        };
        return Status {
            online: true,
            name: v["hostname"].as_str().unwrap_or("").to_string(),
            players: v["clients"].as_u64(),
            max: v["sv_maxclients"].as_u64(),
            ping: ms,
            icon,
        };
    }

    // อ่านข้อมูลไม่ได้ ลองต่อ TCP ดูว่าเซิร์ฟเวอร์ยังเปิดอยู่ไหม
    use std::net::ToSocketAddrs;
    let t = Instant::now();
    if let Some(sa) = host.to_socket_addrs().ok().and_then(|mut i| i.next()) {
        if std::net::TcpStream::connect_timeout(&sa, Duration::from_secs(3)).is_ok() {
            return Status {
                online: true,
                name: String::new(),
                players: None,
                max: None,
                ping: t.elapsed().as_millis() as u32,
                icon: None,
            };
        }
    }
    offline()
}

// ---------- เปิด FiveM ----------
#[cfg(windows)]
fn open_url(u: &str) {
    use std::os::windows::process::CommandExt;
    let _ = std::process::Command::new("rundll32")
        .args(["url.dll,FileProtocolHandler", u])
        .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
        .spawn();
}

#[cfg(not(windows))]
fn open_url(u: &str) {
    let _ = std::process::Command::new("xdg-open").arg(u).spawn();
}

fn launch_fivem() {
    if let Some(base) = std::env::var_os("LOCALAPPDATA") {
        let exe = PathBuf::from(base).join("FiveM").join("FiveM.exe");
        if exe.exists() {
            let mut c = std::process::Command::new(&exe);
            if let Some(dir) = exe.parent() {
                c.current_dir(dir);
            }
            if c.spawn().is_ok() {
                return;
            }
        }
    }
    open_url("fivem://");
}

// ---------- ฟอนต์ / สไตล์ ----------
fn setup_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    // ฟอนต์เริ่มต้นของ egui ไม่มีภาษาไทย จึงยืมฟอนต์จาก Windows (ไม่ต้องฝังในไฟล์)
    let win = std::env::var("WINDIR").unwrap_or_else(|_| "C:\\Windows".into());
    for name in ["leelawui.ttf", "tahoma.ttf"] {
        let path = PathBuf::from(&win).join("Fonts").join(name);
        if let Ok(bytes) = std::fs::read(path) {
            fonts
                .font_data
                .insert("thai".to_owned(), egui::FontData::from_owned(bytes));
            for fam in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
                if let Some(list) = fonts.families.get_mut(&fam) {
                    list.push("thai".to_owned());
                }
            }
            break;
        }
    }
    ctx.set_fonts(fonts);
}

fn setup_style(ctx: &egui::Context) {
    let mut v = egui::Visuals::dark();
    v.panel_fill = BG;
    v.window_fill = Color32::from_rgb(18, 21, 36);
    v.extreme_bg_color = Color32::from_rgb(14, 16, 28);
    v.widgets.inactive.weak_bg_fill = Color32::from_rgb(28, 32, 52);
    v.widgets.hovered.weak_bg_fill = Color32::from_rgb(40, 45, 72);
    ctx.set_visuals(v);
    ctx.style_mut(|s| {
        s.spacing.item_spacing = vec2(10.0, 10.0);
        s.spacing.button_padding = vec2(14.0, 7.0);
    });
}

// ---------- แอป ----------
#[derive(PartialEq, Clone, Copy)]
enum Filter {
    All,
    Online,
    Fav,
}

enum Act {
    Add,
    Refresh,
    Launch,
    Connect(String),
    Fav(String),
    Edit(String),
    Del(String),
}

#[derive(Default)]
struct Editor {
    id: Option<String>,
    name: String,
    address: String,
    error: String,
}

struct Msg {
    id: String,
    status: Status,
}

struct App {
    state: State,
    status: HashMap<String, Status>,
    icons: HashMap<String, egui::TextureHandle>,
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
    last_refresh: Instant,
    query: String,
    filter: Filter,
    editor: Option<Editor>,
    confirm_del: Option<String>,
    toast: Option<(String, Instant)>,
}

fn display_name(s: &Server, st: Option<&Status>) -> String {
    if !s.name.is_empty() {
        return s.name.clone();
    }
    if let Some(st) = st {
        let c = clean(&st.name);
        if !c.is_empty() {
            return c;
        }
    }
    s.address.clone()
}

impl App {
    fn new() -> Self {
        let (tx, rx) = channel();
        Self {
            state: load_state(),
            status: HashMap::new(),
            icons: HashMap::new(),
            tx,
            rx,
            last_refresh: Instant::now()
                .checked_sub(REFRESH_EVERY)
                .unwrap_or_else(Instant::now),
            query: String::new(),
            filter: Filter::All,
            editor: None,
            confirm_del: None,
            toast: None,
        }
    }

    fn refresh(&mut self, ctx: &egui::Context) {
        self.last_refresh = Instant::now();
        for s in &self.state.servers {
            let Some(addr) = normalize(&s.address) else {
                continue;
            };
            let want_icon = !self.icons.contains_key(&s.id);
            let tx = self.tx.clone();
            let id = s.id.clone();
            let ctx = ctx.clone();
            std::thread::spawn(move || {
                let status = fetch_status(&addr, want_icon);
                let _ = tx.send(Msg { id, status });
                ctx.request_repaint();
            });
        }
    }

    fn on_msg(&mut self, ctx: &egui::Context, m: Msg) {
        let mut st = m.status;
        if let Some(ic) = st.icon.take() {
            let img = egui::ColorImage::from_rgba_unmultiplied([ic.w, ic.h], &ic.rgba);
            let tex = ctx.load_texture(format!("icon-{}", m.id), img, egui::TextureOptions::LINEAR);
            self.icons.insert(m.id.clone(), tex);
        }
        self.status.insert(m.id, st);
    }

    fn visible(&self) -> Vec<Server> {
        let q = self.query.trim().to_lowercase();
        self.state
            .servers
            .iter()
            .filter(|s| {
                let st = self.status.get(&s.id);
                let pass = match self.filter {
                    Filter::All => true,
                    Filter::Online => st.map_or(false, |x| x.online),
                    Filter::Fav => s.fav,
                };
                pass && (q.is_empty()
                    || display_name(s, st).to_lowercase().contains(&q)
                    || s.address.to_lowercase().contains(&q))
            })
            .cloned()
            .collect()
    }

    fn say(&mut self, msg: String) {
        self.toast = Some((msg, Instant::now()));
    }

    fn connect(&mut self, id: &str) {
        let found = self.state.servers.iter().find(|s| s.id == id).map(|s| {
            (
                normalize(&s.address),
                display_name(s, self.status.get(&s.id)),
            )
        });
        let Some((addr, name)) = found else { return };
        match addr {
            Some(a) => {
                open_url(&format!("fivem://connect/{a}"));
                self.state.last = Some(id.to_string());
                save_state(&self.state);
                self.say(format!("กำลังเปิด FiveM → {name}"));
            }
            None => self.say("ที่อยู่เซิร์ฟเวอร์ไม่ถูกต้อง".to_string()),
        }
    }
}

fn star(ui: &mut egui::Ui, on: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(26.0, 26.0), Sense::click());
    let c = rect.center();
    let (r, ri) = (11.0f32, 11.0f32 * 0.45);
    let col = if on {
        GOLD
    } else if resp.hovered() {
        Color32::from_rgb(150, 155, 180)
    } else {
        Color32::from_rgb(80, 85, 110)
    };
    let outer = |i: usize| -> Pos2 {
        let a = (-90.0f32 + 72.0 * i as f32).to_radians();
        c + vec2(a.cos(), a.sin()) * r
    };
    let inner = |i: usize| -> Pos2 {
        let a = (-54.0f32 + 72.0 * i as f32).to_radians();
        c + vec2(a.cos(), a.sin()) * ri
    };
    let p = ui.painter();
    // วาดดาวจากรูปนูนหลายชิ้น (ห้าเหลี่ยมตรงกลาง + สามเหลี่ยมห้าแฉก)
    p.add(egui::Shape::convex_polygon(
        (0..5).map(|i| inner(i)).collect(),
        col,
        Stroke::NONE,
    ));
    for i in 0..5 {
        p.add(egui::Shape::convex_polygon(
            vec![inner((i + 4) % 5), outer(i), inner(i)],
            col,
            Stroke::NONE,
        ));
    }
    resp
}

fn draw_row(
    ui: &mut egui::Ui,
    s: &Server,
    st: Option<&Status>,
    icon: Option<&egui::TextureHandle>,
    name: &str,
    acts: &mut Vec<Act>,
) {
    egui::Frame::none()
        .fill(ROW)
        .stroke(Stroke::new(1.0, ROW_LINE))
        .rounding(14.0)
        .inner_margin(egui::Margin::symmetric(14.0, 12.0))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                // ไอคอน + จุดสถานะ
                let (rect, _) = ui.allocate_exact_size(vec2(52.0, 52.0), Sense::hover());
                match icon {
                    Some(t) => {
                        let img = egui::Image::from_texture(egui::load::SizedTexture::new(
                            t.id(),
                            rect.size(),
                        ))
                        .rounding(12.0);
                        ui.put(rect, img);
                    }
                    None => {
                        let p = ui.painter();
                        p.rect_filled(rect, 12.0, Color32::from_rgb(38, 44, 80));
                        let letter = name
                            .chars()
                            .next()
                            .map(|c| c.to_uppercase().to_string())
                            .unwrap_or_default();
                        p.text(
                            rect.center(),
                            Align2::CENTER_CENTER,
                            letter,
                            egui::FontId::proportional(24.0),
                            Color32::WHITE,
                        );
                    }
                }
                let online = st.map_or(false, |x| x.online);
                let dot = rect.right_bottom() - vec2(4.0, 4.0);
                ui.painter().circle_filled(dot, 7.0, ROW);
                ui.painter()
                    .circle_filled(dot, 5.0, if online { GREEN } else { RED });

                // ชื่อ + แถบจำนวนผู้เล่น
                ui.vertical(|ui| {
                    ui.label(RichText::new(name).size(17.0).strong());
                    let frac = match st {
                        Some(x) => match (x.players, x.max) {
                            (Some(p), Some(m)) if m > 0 => p as f32 / m as f32,
                            _ => 0.0,
                        },
                        None => 0.0,
                    };
                    ui.add(
                        egui::ProgressBar::new(frac.clamp(0.0, 1.0))
                            .desired_width(240.0)
                            .desired_height(6.0)
                            .fill(ACCENT),
                    );
                });

                // ปุ่ม / ตัวเลขทางขวา (ตัวแรกที่ใส่จะอยู่ขวาสุด)
                ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                    let play =
                        egui::Button::new(RichText::new("เล่นเลย").strong().color(Color32::WHITE))
                            .fill(ACCENT)
                            .rounding(10.0)
                            .min_size(vec2(100.0, 36.0));
                    if ui.add(play).clicked() {
                        acts.push(Act::Connect(s.id.clone()));
                    }
                    if star(ui, s.fav).clicked() {
                        acts.push(Act::Fav(s.id.clone()));
                    }
                    if ui.small_button("ลบ").clicked() {
                        acts.push(Act::Del(s.id.clone()));
                    }
                    if ui.small_button("แก้ไข").clicked() {
                        acts.push(Act::Edit(s.id.clone()));
                    }
                    ui.add_space(8.0);
                    match st {
                        Some(x) if x.online => {
                            let col = if x.ping < 60 {
                                GREEN
                            } else if x.ping < 120 {
                                AMBER
                            } else {
                                RED
                            };
                            ui.label(RichText::new(format!("{} ms", x.ping)).color(col));
                            if let (Some(p), Some(m)) = (x.players, x.max) {
                                ui.label(format!("{p} / {m}"));
                            }
                        }
                        Some(_) => {
                            ui.label(RichText::new("ออฟไลน์").color(RED));
                        }
                        None => {
                            ui.label(RichText::new("กำลังเช็ค…").color(TEXT_DIM));
                        }
                    }
                });
            });
        });
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        while let Ok(m) = self.rx.try_recv() {
            self.on_msg(ctx, m);
        }
        if self.last_refresh.elapsed() >= REFRESH_EVERY {
            self.refresh(ctx);
        }
        // ตื่นมาเช็คเวลาทุก 5 วินาทีเท่านั้น ที่เหลือหลับ (ประหยัด CPU/แบต)
        ctx.request_repaint_after(Duration::from_secs(5));

        let mut acts: Vec<Act> = Vec::new();
        let visible = self.visible();
        let total = self.state.servers.len();
        let online = self
            .state
            .servers
            .iter()
            .filter(|s| self.status.get(&s.id).map_or(false, |x| x.online))
            .count();
        let players: u64 = self
            .state
            .servers
            .iter()
            .filter_map(|s| self.status.get(&s.id))
            .filter(|x| x.online)
            .map(|x| x.players.unwrap_or(0))
            .sum();

        egui::CentralPanel::default()
            .frame(
                egui::Frame::none()
                    .fill(BG)
                    .inner_margin(egui::Margin::same(20.0)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.label(RichText::new("เซิร์ฟเวอร์").size(28.0).strong());
                        ui.label(
                            RichText::new(format!("ออนไลน์ {online}/{total}  ·  ผู้เล่นรวม {players}"))
                                .color(TEXT_DIM),
                        );
                    });
                    ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                        let add = egui::Button::new(RichText::new("+ เพิ่มเซิร์ฟเวอร์").strong())
                            .min_size(vec2(0.0, 36.0));
                        if ui.add(add).clicked() {
                            acts.push(Act::Add);
                        }
                        if ui.button("รีเฟรช").clicked() {
                            acts.push(Act::Refresh);
                        }
                        if ui.button("เปิด FiveM เฉยๆ").clicked() {
                            acts.push(Act::Launch);
                        }
                    });
                });
                ui.horizontal(|ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut self.query)
                            .hint_text("ค้นหาเซิร์ฟเวอร์...")
                            .desired_width(280.0),
                    );
                    ui.selectable_value(&mut self.filter, Filter::All, "ทั้งหมด");
                    ui.selectable_value(&mut self.filter, Filter::Online, "ออนไลน์");
                    ui.selectable_value(&mut self.filter, Filter::Fav, "โปรด");
                });
                ui.add_space(4.0);
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        if visible.is_empty() {
                            ui.add_space(30.0);
                            ui.vertical_centered(|ui| {
                                let msg = if total == 0 {
                                    "ยังไม่มีเซิร์ฟเวอร์ กด “+ เพิ่มเซิร์ฟเวอร์” เพื่อเริ่มต้น"
                                } else {
                                    "ไม่พบเซิร์ฟเวอร์ที่ตรงกับเงื่อนไข"
                                };
                                ui.label(RichText::new(msg).color(TEXT_DIM));
                            });
                        }
                        for s in &visible {
                            let st = self.status.get(&s.id);
                            let name = display_name(s, st);
                            draw_row(ui, s, st, self.icons.get(&s.id), &name, &mut acts);
                        }
                    });
            });

        for a in acts {
            match a {
                Act::Add => self.editor = Some(Editor::default()),
                Act::Refresh => self.refresh(ctx),
                Act::Launch => launch_fivem(),
                Act::Connect(id) => self.connect(&id),
                Act::Fav(id) => {
                    if let Some(s) = self.state.servers.iter_mut().find(|s| s.id == id) {
                        s.fav = !s.fav;
                    }
                    save_state(&self.state);
                }
                Act::Edit(id) => {
                    if let Some(s) = self.state.servers.iter().find(|s| s.id == id) {
                        self.editor = Some(Editor {
                            id: Some(id.clone()),
                            name: s.name.clone(),
                            address: s.address.clone(),
                            error: String::new(),
                        });
                    }
                }
                Act::Del(id) => self.confirm_del = Some(id),
            }
        }

        // หน้าต่างเพิ่ม/แก้ไข
        if let Some(mut ed) = self.editor.take() {
            let mut keep = true;
            let mut save = false;
            let title = if ed.id.is_some() {
                "แก้ไขเซิร์ฟเวอร์"
            } else {
                "เพิ่มเซิร์ฟเวอร์"
            };
            egui::Window::new(title)
                .collapsible(false)
                .resizable(false)
                .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.label("ชื่อเซิร์ฟเวอร์ (เว้นว่างเพื่อใช้ชื่อจากเซิร์ฟเวอร์)");
                    ui.add(egui::TextEdit::singleline(&mut ed.name).desired_width(340.0));
                    ui.label("IP:Port หรือ cfx.re/join/โค้ด");
                    ui.add(egui::TextEdit::singleline(&mut ed.address).desired_width(340.0));
                    if !ed.error.is_empty() {
                        ui.colored_label(RED, &ed.error);
                    }
                    ui.horizontal(|ui| {
                        if ui.button("บันทึก").clicked() {
                            save = true;
                        }
                        if ui.button("ยกเลิก").clicked() {
                            keep = false;
                        }
                    });
                });
            if save {
                match normalize(&ed.address) {
                    Some(addr) => {
                        let name = ed.name.trim().to_string();
                        match &ed.id {
                            Some(id) => {
                                if let Some(s) = self.state.servers.iter_mut().find(|s| &s.id == id)
                                {
                                    s.name = name;
                                    s.address = addr;
                                }
                            }
                            None => {
                                let id = std::time::SystemTime::now()
                                    .duration_since(std::time::UNIX_EPOCH)
                                    .map(|d| d.as_millis())
                                    .unwrap_or(0);
                                self.state.servers.push(Server {
                                    id: format!("s{id}"),
                                    name,
                                    address: addr,
                                    fav: false,
                                });
                            }
                        }
                        save_state(&self.state);
                        self.refresh(ctx);
                        keep = false;
                    }
                    None => ed.error = "ที่อยู่ไม่ถูกต้อง (ใช้ IP:Port หรือ cfx.re/join/โค้ด)".into(),
                }
            }
            if keep {
                self.editor = Some(ed);
            }
        }

        // ยืนยันการลบ
        if let Some(id) = self.confirm_del.clone() {
            let label = self
                .state
                .servers
                .iter()
                .find(|s| s.id == id)
                .map(|s| display_name(s, self.status.get(&s.id)))
                .unwrap_or_default();
            let mut close = false;
            let mut yes = false;
            egui::Window::new("ยืนยันการลบ")
                .collapsible(false)
                .resizable(false)
                .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.label(format!("ลบ \"{label}\" ใช่หรือไม่?"));
                    ui.horizontal(|ui| {
                        if ui.button("ลบ").clicked() {
                            yes = true;
                            close = true;
                        }
                        if ui.button("ยกเลิก").clicked() {
                            close = true;
                        }
                    });
                });
            if yes {
                self.state.servers.retain(|s| s.id != id);
                if self.state.last.as_deref() == Some(id.as_str()) {
                    self.state.last = None;
                }
                self.status.remove(&id);
                self.icons.remove(&id);
                save_state(&self.state);
            }
            if close {
                self.confirm_del = None;
            }
        }

        // ข้อความแจ้งเตือนมุมล่าง
        let expired = self
            .toast
            .as_ref()
            .map_or(false, |(_, t)| t.elapsed() > Duration::from_secs(3));
        if expired {
            self.toast = None;
        }
        if let Some((msg, _)) = &self.toast {
            egui::Area::new(egui::Id::new("toast"))
                .anchor(Align2::CENTER_BOTTOM, [0.0, -20.0])
                .show(ctx, |ui| {
                    egui::Frame::none()
                        .fill(Color32::from_rgb(30, 34, 56))
                        .stroke(Stroke::new(1.0, ROW_LINE))
                        .rounding(12.0)
                        .inner_margin(egui::Margin::symmetric(16.0, 10.0))
                        .show(ui, |ui| {
                            ui.label(RichText::new(msg.as_str()).strong());
                        });
                });
            ctx.request_repaint_after(Duration::from_millis(500));
        }
    }
}

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([960.0, 680.0])
            .with_min_inner_size([700.0, 460.0])
            .with_title("FiveM Launcher"),
        ..Default::default()
    };
    eframe::run_native(
        "FiveM Launcher",
        options,
        Box::new(|cc| {
            setup_fonts(&cc.egui_ctx);
            setup_style(&cc.egui_ctx);
            Ok(Box::new(App::new()))
        }),
    )
}
