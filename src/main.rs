// FiveM Launcher - Rust + egui (UI เนทีฟ ไม่มีเบราว์เซอร์ในตัว กิน RAM น้อย)
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use eframe::egui::{self, pos2, vec2, Align, Align2, Color32, Pos2, RichText, Sense, Stroke};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::{Duration, Instant};

// เซิร์ฟเวอร์เริ่มต้น (ใช้ตอนเปิดโปรแกรมครั้งแรกที่ยังไม่มีไฟล์ servers.json)
// เพิ่มได้โดยใส่ที่อยู่ต่อท้ายในวงเล็บเหลี่ยม คั่นด้วยเครื่องหมายคอมมา
const DEFAULT_SERVERS: &[&str] = &["89.38.101.183:30120"];
const REFRESH_EVERY: Duration = Duration::from_secs(30);

// ระบบอัปเดตตัวเอง: เช็คเวอร์ชันล่าสุดจาก GitHub Releases ของ repository นี้
// (repository ต้องเป็น Public โปรแกรมจึงจะอ่านได้)
const VERSION: &str = env!("CARGO_PKG_VERSION");
const REPO: &str = "Nxttiiznnn-ee1/fivem-launcher";
const ASSET: &str = "fivem-launcher.exe";

const BG: Color32 = Color32::from_rgb(10, 12, 22);
const ROW: Color32 = Color32::from_rgb(20, 23, 38);
const ROW_LINE: Color32 = Color32::from_rgb(38, 42, 64);
const TEXT_DIM: Color32 = Color32::from_rgb(130, 136, 160);
const ACCENT: Color32 = Color32::from_rgb(92, 102, 255);
const GREEN: Color32 = Color32::from_rgb(34, 197, 94);
const AMBER: Color32 = Color32::from_rgb(245, 179, 1);
const RED: Color32 = Color32::from_rgb(239, 68, 68);
const GOLD: Color32 = Color32::from_rgb(250, 190, 40);

// สีประจำ game build (build เดียวกันได้สีเดียวกัน = สลับเซิร์ฟเวอร์กันได้โดยไม่ต้องรีสตาร์ท)
const BUILD_COLORS: [Color32; 6] = [
    Color32::from_rgb(96, 165, 250),
    Color32::from_rgb(74, 222, 128),
    Color32::from_rgb(251, 191, 36),
    Color32::from_rgb(244, 114, 182),
    Color32::from_rgb(45, 212, 191),
    Color32::from_rgb(167, 139, 250),
];

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
    // เลข game build ที่บังคับใช้ (เว้นว่าง = ตรวจจากเซิร์ฟเวอร์อัตโนมัติ)
    #[serde(default)]
    build: String,
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
            servers: DEFAULT_SERVERS
                .iter()
                .enumerate()
                .map(|(i, a)| Server {
                    id: format!("default{}", i + 1),
                    address: a.to_string(),
                    ..Default::default()
                })
                .collect(),
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
    // เลข game build ที่เซิร์ฟเวอร์บังคับ (sv_enforceGameBuild) ถ้ามี
    build: Option<String>,
    // true = อ่าน info.json ได้แล้ว ไม่ต้องโหลดซ้ำทุกรอบ
    info_ok: bool,
}

fn offline() -> Status {
    Status {
        online: false,
        name: String::new(),
        players: None,
        max: None,
        ping: 0,
        icon: None,
        build: None,
        info_ok: false,
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

// เซิร์ฟเวอร์บางตัวส่งตัวเลขมาเป็นข้อความ เช่น "2048" จึงรองรับทั้งสองแบบ
fn num(v: &serde_json::Value) -> Option<u64> {
    v.as_u64()
        .or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
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

fn fetch_status(address: &str, want_info: bool) -> Status {
    if address.contains("cfx.re/join/") {
        let code = address.rsplit('/').next().unwrap_or("");
        let url = format!("https://servers-frontend.fivem.net/api/servers/single/{code}");
        return match get_json(&url, 6) {
            Some((v, ms)) => {
                let d = &v["Data"];
                Status {
                    online: true,
                    name: d["hostname"].as_str().unwrap_or("").to_string(),
                    players: num(&d["clients"]),
                    max: num(&d["sv_maxclients"]),
                    ping: ms,
                    build: d["vars"]["sv_enforceGameBuild"]
                        .as_str()
                        .map(|s| s.to_string()),
                    info_ok: true,
                    ..offline()
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
        let (icon, build, info_ok) = if want_info {
            match get_json(&format!("http://{host}/info.json"), 3) {
                Some((i, _)) => (
                    i["icon"].as_str().and_then(decode_icon),
                    i["vars"]["sv_enforceGameBuild"]
                        .as_str()
                        .map(|s| s.to_string()),
                    true,
                ),
                None => (None, None, false),
            }
        } else {
            (None, None, false)
        };
        return Status {
            online: true,
            name: v["hostname"].as_str().unwrap_or("").to_string(),
            players: num(&v["clients"]),
            max: num(&v["sv_maxclients"]),
            ping: ms,
            icon,
            build,
            info_ok,
        };
    }

    // อ่านข้อมูลไม่ได้ ลองต่อ TCP ดูว่าเซิร์ฟเวอร์ยังเปิดอยู่ไหม
    use std::net::ToSocketAddrs;
    let t = Instant::now();
    if let Some(sa) = host.to_socket_addrs().ok().and_then(|mut i| i.next()) {
        if std::net::TcpStream::connect_timeout(&sa, Duration::from_secs(3)).is_ok() {
            return Status {
                online: true,
                ping: t.elapsed().as_millis() as u32,
                ..offline()
            };
        }
    }
    offline()
}

// ---------- เปิด FiveM ----------
#[cfg(windows)]
fn open_url(u: &str) {
    // ส่งให้ explorer.exe เป็นคนเปิด เพื่อให้ FiveM เห็นว่าถูกเรียกจากเชลล์ของ Windows
    // (ไม่ใช่จากโปรแกรมอื่น) ซึ่งเป็นเงื่อนไขที่ FiveM ตรวจก่อนยอมเปิด
    let _ = std::process::Command::new("explorer.exe").arg(u).spawn();
}

#[cfg(not(windows))]
fn open_url(u: &str) {
    let _ = std::process::Command::new("xdg-open").arg(u).spawn();
}

fn fivem_exe() -> Option<PathBuf> {
    let base = std::env::var_os("LOCALAPPDATA")?;
    let exe = PathBuf::from(base).join("FiveM").join("FiveM.exe");
    exe.exists().then_some(exe)
}

// เปิด FiveM ผ่านเชลล์ของ Windows (เหมือนดับเบิลคลิก) ไม่สั่งรันโปรแกรมตรงๆ
// เพราะ FiveM จะแจ้ง "should be launched directly from the shell or a web browser"
fn launch_fivem() {
    match fivem_exe() {
        Some(exe) => open_url(&exe.to_string_lossy()),
        None => open_url("fivem://"),
    }
}

// เปิดเกมเข้าเซิร์ฟเวอร์ผ่านลิงก์ fivem://connect/ (วิธีมาตรฐานของ FiveM)
fn connect_game(addr: &str) {
    open_url(&format!("fivem://connect/{addr}"));
}

// ---------- อัปเดตตัวเอง ----------
#[derive(Clone)]
struct Update {
    version: String,
    url: String,
    sha256: Option<String>,
}

enum UpdMsg {
    Found(Update),
    Failed(String),
}

fn parse_ver(s: &str) -> [u32; 3] {
    let mut out = [0u32; 3];
    for (i, p) in s.trim_start_matches('v').split('.').take(3).enumerate() {
        out[i] = p.trim().parse().unwrap_or(0);
    }
    out
}

fn check_update() -> Option<Update> {
    let url = format!("https://api.github.com/repos/{REPO}/releases/latest");
    let (v, _) = get_json(&url, 6)?;
    let tag = v["tag_name"].as_str()?;
    if parse_ver(tag) <= parse_ver(VERSION) {
        return None;
    }
    let asset = v["assets"]
        .as_array()?
        .iter()
        .find(|a| a["name"].as_str() == Some(ASSET))?;
    Some(Update {
        version: tag.trim_start_matches('v').to_string(),
        url: asset["browser_download_url"].as_str()?.to_string(),
        sha256: asset["digest"]
            .as_str()
            .and_then(|d| d.strip_prefix("sha256:"))
            .map(|d| d.to_lowercase()),
    })
}

// โหลดไฟล์ใหม่ สลับแทนไฟล์ที่กำลังรัน (Windows เปลี่ยนชื่อไฟล์ที่กำลังรันได้) แล้วเปิดตัวใหม่
fn apply_update(u: &Update) -> Result<(), String> {
    use sha2::{Digest, Sha256};
    // รับเฉพาะไฟล์ที่มาจาก Releases ของ repository นี้เท่านั้น
    if !u
        .url
        .starts_with(&format!("https://github.com/{REPO}/releases/download/"))
    {
        return Err("ที่อยู่ไฟล์อัปเดตไม่ถูกต้อง".into());
    }
    let cur = std::env::current_exe().map_err(|e| e.to_string())?;
    let new = cur.with_extension("new");
    let old = cur.with_extension("old");

    let resp = ureq::get(&u.url)
        .set("User-Agent", "FiveM-Launcher/1.0")
        .timeout(Duration::from_secs(180))
        .call()
        .map_err(|e| format!("โหลดไฟล์ไม่สำเร็จ: {e}"))?;
    let mut buf = Vec::new();
    resp.into_reader()
        .take(150 * 1024 * 1024)
        .read_to_end(&mut buf)
        .map_err(|e| format!("โหลดไฟล์ไม่สำเร็จ: {e}"))?;

    // ตรวจว่าเป็นไฟล์ .exe จริง และตรงกับค่า SHA-256 ที่ GitHub ระบุ (ถ้ามี)
    if buf.len() < 1_000_000 || !buf.starts_with(b"MZ") {
        return Err("ไฟล์ที่โหลดมาไม่ใช่โปรแกรมที่ถูกต้อง".into());
    }
    if let Some(expected) = &u.sha256 {
        let got = format!("{:x}", Sha256::digest(&buf));
        if &got != expected {
            return Err("ค่าตรวจสอบไฟล์ไม่ตรงกัน (ยกเลิกการอัปเดต)".into());
        }
    }

    std::fs::write(&new, &buf).map_err(|e| e.to_string())?;
    let _ = std::fs::remove_file(&old);
    if let Err(e) = std::fs::rename(&cur, &old) {
        let _ = std::fs::remove_file(&new);
        return Err(format!("สลับไฟล์ไม่สำเร็จ: {e}"));
    }
    if let Err(e) = std::fs::rename(&new, &cur) {
        // พลาด: คืนไฟล์เดิมกลับ
        let _ = std::fs::rename(&old, &cur);
        return Err(format!("สลับไฟล์ไม่สำเร็จ: {e}"));
    }
    std::process::Command::new(&cur)
        .spawn()
        .map_err(|e| format!("เปิดเวอร์ชันใหม่ไม่ได้: {e}"))?;
    Ok(())
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
    DragStart(String),
    DragEnd,
    Update,
}

#[derive(Default)]
struct Editor {
    id: Option<String>,
    name: String,
    address: String,
    build: String,
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
    builds: HashMap<String, String>,
    info_done: HashSet<String>,
    dragging: Option<String>,
    row_rects: Vec<(String, egui::Rect)>,
    update: Option<Update>,
    updating: bool,
    update_checked: bool,
    utx: Sender<UpdMsg>,
    urx: Receiver<UpdMsg>,
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
        let (utx, urx) = channel();
        Self {
            state: load_state(),
            status: HashMap::new(),
            icons: HashMap::new(),
            builds: HashMap::new(),
            info_done: HashSet::new(),
            dragging: None,
            row_rects: Vec::new(),
            update: None,
            updating: false,
            update_checked: false,
            utx,
            urx,
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
            let want_info = !self.info_done.contains(&s.id);
            let tx = self.tx.clone();
            let id = s.id.clone();
            let ctx = ctx.clone();
            std::thread::spawn(move || {
                let status = fetch_status(&addr, want_info);
                let _ = tx.send(Msg { id, status });
                ctx.request_repaint();
            });
        }
    }

    fn on_msg(&mut self, ctx: &egui::Context, m: Msg) {
        let mut st = m.status;
        if st.info_ok {
            self.info_done.insert(m.id.clone());
        }
        if let Some(b) = st.build.take() {
            self.builds.insert(m.id.clone(), b);
        }
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

    // ย้ายแถวไปตำแหน่งที่ปล่อยเมาส์ (y = ตำแหน่งแนวตั้งของเมาส์)
    fn reorder(&mut self, id: &str, y: f32) {
        let Some(from) = self.state.servers.iter().position(|s| s.id == id) else {
            return;
        };
        let target = self
            .row_rects
            .iter()
            .filter(|(rid, r)| rid != id && r.center().y < y)
            .count();
        let s = self.state.servers.remove(from);
        let at = target.min(self.state.servers.len());
        self.state.servers.insert(at, s);
        save_state(&self.state);
    }

    // เลข build ของเซิร์ฟเวอร์: ที่ตรวจเจอจากเซิร์ฟเวอร์ (หรือที่เคยตั้งไว้เองในไฟล์)
    fn build_of(&self, s: &Server) -> Option<String> {
        if !s.build.is_empty() {
            Some(s.build.clone())
        } else {
            self.builds.get(&s.id).cloned()
        }
    }

    fn connect(&mut self, id: &str) {
        let Some(s) = self.state.servers.iter().find(|s| s.id == id) else {
            return;
        };
        let addr = normalize(&s.address);
        let name = display_name(s, self.status.get(&s.id));
        let target_build = self.build_of(s);
        // build ของเซิร์ฟเวอร์ล่าสุดที่เข้าผ่าน launcher นี้
        let last_build = self
            .state
            .last
            .as_ref()
            .and_then(|lid| self.state.servers.iter().find(|x| &x.id == lid))
            .and_then(|x| self.build_of(x));
        match addr {
            Some(a) => {
                connect_game(&a);
                self.state.last = Some(id.to_string());
                save_state(&self.state);
                let msg = match (&last_build, &target_build) {
                    (Some(l), Some(t)) if l != t => {
                        format!("กำลังเปิด FiveM → {name}  ·  build {l} → {t} FiveM จะรีสตาร์ท")
                    }
                    _ => format!("กำลังเปิด FiveM → {name}"),
                };
                self.say(msg);
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
    build: Option<(&str, Color32)>,
    can_drag: bool,
    rects: &mut Vec<(String, egui::Rect)>,
    acts: &mut Vec<Act>,
) {
    let framed = egui::Frame::none()
        .fill(ROW)
        .stroke(Stroke::new(1.0, ROW_LINE))
        .rounding(14.0)
        .inner_margin(egui::Margin::symmetric(14.0, 12.0))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                // ที่จับสำหรับลากสลับลำดับ (เปิดใช้เมื่อไม่ได้ค้นหา/กรองอยู่)
                if can_drag {
                    let (hr, hresp) = ui.allocate_exact_size(vec2(14.0, 52.0), Sense::drag());
                    let col = if hresp.hovered() || hresp.dragged() {
                        TEXT_DIM
                    } else {
                        Color32::from_rgb(70, 75, 100)
                    };
                    for dx in [-3.0f32, 3.0] {
                        for dy in [-8.0f32, 0.0, 8.0] {
                            ui.painter()
                                .circle_filled(hr.center() + vec2(dx, dy), 1.6, col);
                        }
                    }
                    if hresp.dragged() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
                    } else if hresp.hovered() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
                    }
                    if hresp.drag_started() {
                        acts.push(Act::DragStart(s.id.clone()));
                    }
                    if hresp.drag_stopped() {
                        acts.push(Act::DragEnd);
                    }
                }
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
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(name).size(17.0).strong());
                        if let Some((b, col)) = build {
                            egui::Frame::none()
                                .fill(col.gamma_multiply(0.2))
                                .rounding(8.0)
                                .inner_margin(egui::Margin::symmetric(8.0, 2.0))
                                .show(ui, |ui| {
                                    ui.label(
                                        RichText::new(format!("build {b}"))
                                            .size(12.0)
                                            .strong()
                                            .color(col),
                                    );
                                });
                        }
                    });
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
    rects.push((s.id.clone(), framed.response.rect));
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        while let Ok(m) = self.rx.try_recv() {
            self.on_msg(ctx, m);
        }
        // เช็คอัปเดตครั้งเดียวตอนเปิดโปรแกรม
        if !self.update_checked {
            self.update_checked = true;
            let tx = self.utx.clone();
            let c = ctx.clone();
            std::thread::spawn(move || {
                if let Some(u) = check_update() {
                    let _ = tx.send(UpdMsg::Found(u));
                    c.request_repaint();
                }
            });
        }
        while let Ok(m) = self.urx.try_recv() {
            match m {
                UpdMsg::Found(u) => self.update = Some(u),
                UpdMsg::Failed(e) => {
                    self.updating = false;
                    self.say(format!("อัปเดตไม่สำเร็จ: {e}"));
                }
            }
        }
        if self.last_refresh.elapsed() >= REFRESH_EVERY {
            self.refresh(ctx);
        }
        // ตื่นมาเช็คเวลาทุก 5 วินาทีเท่านั้น ที่เหลือหลับ (ประหยัด CPU/แบต)
        ctx.request_repaint_after(Duration::from_secs(5));

        // แต่ละ build ได้สีประจำตัว (เรียงตามเลข build ให้สีคงที่)
        let mut distinct: Vec<String> = self
            .state
            .servers
            .iter()
            .filter_map(|s| self.build_of(s))
            .collect();
        distinct.sort();
        distinct.dedup();
        let build_info: HashMap<String, (String, Color32)> = self
            .state
            .servers
            .iter()
            .filter_map(|s| {
                let b = self.build_of(s)?;
                let idx = distinct.iter().position(|d| *d == b)?;
                Some((s.id.clone(), (b, BUILD_COLORS[idx % BUILD_COLORS.len()])))
            })
            .collect();

        let mut acts: Vec<Act> = Vec::new();
        let mut new_rects: Vec<(String, egui::Rect)> = Vec::new();
        let can_drag = self.filter == Filter::All && self.query.trim().is_empty();
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
                            RichText::new(format!(
                                "ออนไลน์ {online}/{total}  ·  ผู้เล่นรวม {players}  ·  v{VERSION}"
                            ))
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
                        if let Some(u) = &self.update {
                            let label = if self.updating {
                                "กำลังอัปเดต…".to_string()
                            } else {
                                format!("อัปเดตเป็น v{}", u.version)
                            };
                            let b = egui::Button::new(
                                RichText::new(label).strong().color(Color32::WHITE),
                            )
                            .fill(GREEN.gamma_multiply(0.7));
                            if ui.add_enabled(!self.updating, b).clicked() {
                                acts.push(Act::Update);
                            }
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
                            let build = build_info.get(&s.id).map(|(b, c)| (b.as_str(), *c));
                            draw_row(
                                ui,
                                s,
                                st,
                                self.icons.get(&s.id),
                                &name,
                                build,
                                can_drag,
                                &mut new_rects,
                                &mut acts,
                            );
                        }

                        // เส้นบอกตำแหน่งที่จะวางแถว ระหว่างกำลังลาก
                        if let Some(dragged) = self.dragging.as_deref() {
                            if let Some(pos) = ui.ctx().input(|i| i.pointer.latest_pos()) {
                                let others: Vec<&egui::Rect> = self
                                    .row_rects
                                    .iter()
                                    .filter(|(rid, _)| rid.as_str() != dragged)
                                    .map(|(_, r)| r)
                                    .collect();
                                if !others.is_empty() {
                                    let target =
                                        others.iter().filter(|r| r.center().y < pos.y).count();
                                    let y = if target == 0 {
                                        others[0].top() - 5.0
                                    } else {
                                        others[target - 1].bottom() + 5.0
                                    };
                                    ui.ctx()
                                        .layer_painter(egui::LayerId::new(
                                            egui::Order::Foreground,
                                            egui::Id::new("dropline"),
                                        ))
                                        .hline(
                                            egui::Rangef::new(others[0].left(), others[0].right()),
                                            y,
                                            Stroke::new(3.0, ACCENT),
                                        );
                                }
                            }
                        }
                    });
            });
        self.row_rects = new_rects;

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
                            build: s.build.clone(),
                            error: String::new(),
                        });
                    }
                }
                Act::Del(id) => self.confirm_del = Some(id),
                Act::Update => {
                    if let Some(u) = self.update.clone() {
                        self.updating = true;
                        self.say("กำลังโหลดเวอร์ชันใหม่…".to_string());
                        let tx = self.utx.clone();
                        let c = ctx.clone();
                        std::thread::spawn(move || match apply_update(&u) {
                            Ok(()) => std::process::exit(0),
                            Err(e) => {
                                let _ = tx.send(UpdMsg::Failed(e));
                                c.request_repaint();
                            }
                        });
                    }
                }
                Act::DragStart(id) => self.dragging = Some(id),
                Act::DragEnd => {
                    if let Some(id) = self.dragging.take() {
                        if let Some(pos) = ctx.input(|i| i.pointer.latest_pos()) {
                            self.reorder(&id, pos.y);
                        }
                    }
                }
            }
        }
        // กันค้างในโหมดลาก ถ้าปล่อยเมาส์ไปแล้วแต่ยังไม่ได้เคลียร์
        if self.dragging.is_some() && !ctx.input(|i| i.pointer.any_down()) {
            self.dragging = None;
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
            let build = ed.build.trim().to_string();
            let build_ok = build.is_empty()
                || ((3..=5).contains(&build.len()) && build.chars().all(|c| c.is_ascii_digit()));
            if save && !build_ok {
                ed.error = "เลข build ต้องเป็นตัวเลข 3–5 หลัก เช่น 2944".into();
            } else if save {
                match normalize(&ed.address) {
                    Some(addr) => {
                        let name = ed.name.trim().to_string();
                        match &ed.id {
                            Some(id) => {
                                if let Some(s) = self.state.servers.iter_mut().find(|s| &s.id == id)
                                {
                                    s.name = name;
                                    s.address = addr;
                                    s.build = build;
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
                                    build,
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
            .map_or(false, |(_, t)| t.elapsed() > Duration::from_secs(5));
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
    // ลบไฟล์เวอร์ชันเก่าที่เหลือจากการอัปเดตรอบก่อน
    if let Ok(p) = std::env::current_exe() {
        let _ = std::fs::remove_file(p.with_extension("old"));
    }
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
