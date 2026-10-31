use eframe::egui::{self, Color32, RichText, ScrollArea, Vec2};
use serde::Deserialize;
use std::fs::File;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([960.0, 680.0])
            .with_min_inner_size([720.0, 500.0])
            .with_title("APK Install Tool"),
        ..Default::default()
    };

    eframe::run_native(
        "APK Install Tool",
        options,
        Box::new(|cc| {
            let mut visuals = egui::Visuals::dark();
            visuals.window_fill = Color32::from_rgb(18, 18, 22);
            visuals.panel_fill = Color32::from_rgb(24, 24, 30);
            visuals.extreme_bg_color = Color32::from_rgb(12, 12, 16);
            visuals.widgets.noninteractive.bg_fill = Color32::from_rgb(32, 32, 40);
            visuals.widgets.inactive.bg_fill = Color32::from_rgb(40, 40, 52);
            visuals.widgets.hovered.bg_fill = Color32::from_rgb(55, 55, 72);
            visuals.widgets.active.bg_fill = Color32::from_rgb(70, 70, 95);
            visuals.selection.bg_fill = Color32::from_rgb(70, 110, 200);
            visuals.override_text_color = Some(Color32::from_rgb(220, 220, 230));
            cc.egui_ctx.set_visuals(visuals);

            let mut style = (*cc.egui_ctx.style()).clone();
            style.text_styles.insert(
                egui::TextStyle::Body,
                egui::FontId::new(15.0, egui::FontFamily::Proportional),
            );
            style.text_styles.insert(
                egui::TextStyle::Button,
                egui::FontId::new(15.0, egui::FontFamily::Proportional),
            );
            style.text_styles.insert(
                egui::TextStyle::Heading,
                egui::FontId::new(22.0, egui::FontFamily::Proportional),
            );
            cc.egui_ctx.set_style(style);

            Ok(Box::new(App::new()))
        }),
    )
}

#[derive(Clone)]
struct AppHit {
    name: String,
    package: String,
    summary: String,
    source: String,
}

#[derive(Deserialize)]
struct FdroidSearch {
    apps: Vec<FdroidApp>,
}

#[derive(Deserialize)]
struct FdroidApp {
    name: String,
    summary: Option<String>,
    url: String,
}

#[derive(Deserialize)]
struct FdroidPackageInfo {
    #[serde(rename = "packageName")]
    package_name: String,
    #[serde(rename = "suggestedVersionCode")]
    suggested_version_code: i64,
}

enum WorkerMsg {
    Devices(Result<Vec<String>, String>),
    SearchDone(Result<Vec<AppHit>, String>),
    DownloadDone(Result<PathBuf, String>),
    InstallDone(Result<(), String>),
    Log(String),
}

struct App {
    devices: Vec<String>,
    selected_device: Option<usize>,
    search: String,
    results: Vec<AppHit>,
    selected: Option<usize>,
    searching: bool,
    downloading: bool,
    installing: bool,
    direct_url: String,
    output_dir: PathBuf,
    last_apk: Option<PathBuf>,
    log: Vec<(String, Color32)>,
    tx: Sender<WorkerMsg>,
    rx: Receiver<WorkerMsg>,
}

impl App {
    fn new() -> Self {
        let (tx, rx) = mpsc::channel();
        let mut app = Self {
            devices: vec![],
            selected_device: None,
            search: String::new(),
            results: vec![],
            selected: None,
            searching: false,
            downloading: false,
            installing: false,
            direct_url: String::new(),
            output_dir: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            last_apk: None,
            log: vec![],
            tx,
            rx,
        };
        app.refresh_devices();
        app.log(
            "Search apps on F-Droid, download APK, install via ADB".to_string(),
            Color32::LIGHT_GRAY,
        );
        app.log(
            "Store-only apps: paste a direct .apk URL below.".to_string(),
            Color32::LIGHT_GRAY,
        );
        app
    }

    fn log(&mut self, msg: String, color: Color32) {
        self.log.push((msg, color));
        if self.log.len() > 250 {
            self.log.remove(0);
        }
    }

    fn serial(&self) -> Option<&str> {
        self.selected_device
            .and_then(|i| self.devices.get(i))
            .map(|s| s.as_str())
    }

    fn refresh_devices(&mut self) {
        let tx = self.tx.clone();
        thread::spawn(move || match which::which("adb") {
            Ok(_) => match run_adb(None, &["devices"]) {
                Ok(text) => {
                    let list: Vec<String> = text
                        .lines()
                        .skip(1)
                        .filter(|l| l.contains("\tdevice"))
                        .map(|l| l.split('\t').next().unwrap_or("").to_string())
                        .filter(|s| !s.is_empty())
                        .collect();
                    let _ = tx.send(WorkerMsg::Devices(Ok(list)));
                }
                Err(e) => {
                    let _ = tx.send(WorkerMsg::Devices(Err(e)));
                }
            },
            Err(_) => {
                let _ = tx.send(WorkerMsg::Devices(Err(
                    "ADB not found in PATH. Install Platform Tools.".into(),
                )));
            }
        });
    }

    fn do_search(&mut self) {
        let q = self.search.trim().to_string();
        if q.is_empty() {
            self.log("Enter app name".to_string(), Color32::YELLOW);
            return;
        }
        self.searching = true;
        self.results.clear();
        self.selected = None;
        self.log(format!("Searching internet for \"{}\"…", q), Color32::LIGHT_BLUE);

        let tx = self.tx.clone();
        thread::spawn(move || {
            let url = format!(
                "https://search.f-droid.org/api/search_apps?q={}",
                urlencoding::encode(&q)
            );
            match http_get_json::<FdroidSearch>(&url) {
                Ok(data) => {
                    let mut hits = Vec::new();
                    for a in data.apps {
                        let package = a
                            .url
                            .trim_end_matches('/')
                            .rsplit('/')
                            .next()
                            .unwrap_or("")
                            .to_string();
                        if package.is_empty() {
                            continue;
                        }
                        hits.push(AppHit {
                            name: a.name,
                            package,
                            summary: a.summary.unwrap_or_default(),
                            source: "F-Droid".into(),
                        });
                    }
                    let _ = tx.send(WorkerMsg::SearchDone(Ok(hits)));
                }
                Err(e) => {
                    let _ = tx.send(WorkerMsg::SearchDone(Err(e)));
                }
            }
        });
    }

    fn do_download_selected(&mut self) {
        let Some(idx) = self.selected else {
            self.log("Select an app first".to_string(), Color32::YELLOW);
            return;
        };
        let hit = self.results[idx].clone();
        self.downloading = true;
        self.log(
            format!("Resolving download for {}…", hit.name),
            Color32::LIGHT_BLUE,
        );

        let out_dir = self.output_dir.clone();
        let tx = self.tx.clone();
        thread::spawn(move || {
            let info_url = format!("https://f-droid.org/api/v1/packages/{}", hit.package);
            let info = match http_get_json::<FdroidPackageInfo>(&info_url) {
                Ok(i) => i,
                Err(e) => {
                    let _ = tx.send(WorkerMsg::DownloadDone(Err(format!(
                        "Package metadata: {}",
                        e
                    ))));
                    return;
                }
            };

            let apk_url = format!(
                "https://f-droid.org/repo/{}_{}.apk",
                info.package_name, info.suggested_version_code
            );
            let safe_name = format!(
                "{}_{}.apk",
                hit.package.replace('.', "_"),
                info.suggested_version_code
            );
            let dest = out_dir.join(&safe_name);

            match download_file(&apk_url, &dest) {
                Ok(()) => {
                    let _ = tx.send(WorkerMsg::DownloadDone(Ok(dest)));
                }
                Err(e) => {
                    let _ = tx.send(WorkerMsg::DownloadDone(Err(e)));
                }
            }
        });
    }

    fn do_download_url(&mut self) {
        let url = self.direct_url.trim().to_string();
        if url.is_empty() || !url.starts_with("http") {
            self.log("Paste a full https://…apk URL".to_string(), Color32::YELLOW);
            return;
        }
        self.downloading = true;
        self.log(format!("Downloading {}…", url), Color32::LIGHT_BLUE);

        let out_dir = self.output_dir.clone();
        let tx = self.tx.clone();
        thread::spawn(move || {
            let name = url
                .rsplit('/')
                .next()
                .unwrap_or("download.apk")
                .split('?')
                .next()
                .unwrap_or("download.apk");
            let name = if name.ends_with(".apk") {
                name.to_string()
            } else {
                format!("{}.apk", name)
            };
            let dest = out_dir.join(name);
            match download_file(&url, &dest) {
                Ok(()) => {
                    let _ = tx.send(WorkerMsg::DownloadDone(Ok(dest)));
                }
                Err(e) => {
                    let _ = tx.send(WorkerMsg::DownloadDone(Err(e)));
                }
            }
        });
    }

    fn do_install(&mut self) {
        let Some(path) = self.last_apk.clone() else {
            self.log("Download an APK first".to_string(), Color32::YELLOW);
            return;
        };
        if !path.exists() {
            self.log("APK file missing".to_string(), Color32::from_rgb(255, 100, 100));
            return;
        }
        if self.devices.is_empty() {
            self.log(
                "No device connected".to_string(),
                Color32::from_rgb(255, 100, 100),
            );
            return;
        }
        self.installing = true;
        self.log(format!("Installing {}…", path.display()), Color32::LIGHT_BLUE);

        let serial = self.serial().map(|s| s.to_string());
        let tx = self.tx.clone();
        thread::spawn(move || {
            let path_s = path.to_str().unwrap_or("");
            let st = run_adb_status(serial.as_deref(), &["install", "-r", "-t", path_s]);
            if let Ok(s) = &st {
                if s.success() {
                    let _ = tx.send(WorkerMsg::InstallDone(Ok(())));
                    return;
                }
            }
            match run_adb_status(serial.as_deref(), &["install", "-r", path_s]) {
                Ok(s) if s.success() => {
                    let _ = tx.send(WorkerMsg::InstallDone(Ok(())));
                }
                Ok(_) => {
                    let _ = tx.send(WorkerMsg::InstallDone(Err(
                        "Install failed (signature conflict? uninstall old app first)".into(),
                    )));
                }
                Err(e) => {
                    let _ = tx.send(WorkerMsg::InstallDone(Err(e)));
                }
            }
        });
    }

    fn poll(&mut self) {
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                WorkerMsg::Devices(Ok(list)) => {
                    self.devices = list;
                    if self.selected_device.is_none() && !self.devices.is_empty() {
                        self.selected_device = Some(0);
                    }
                    if self.devices.is_empty() {
                        self.log("No online devices".to_string(), Color32::YELLOW);
                    } else {
                        self.log(
                            format!("Found {} device(s)", self.devices.len()),
                            Color32::from_rgb(100, 220, 140),
                        );
                    }
                }
                WorkerMsg::Devices(Err(e)) => {
                    self.log(e, Color32::from_rgb(255, 100, 100));
                }
                WorkerMsg::SearchDone(Ok(hits)) => {
                    self.searching = false;
                    let n = hits.len();
                    self.results = hits;
                    if n == 0 {
                        self.log(
                            "Nothing on F-Droid. Try another name, or paste a direct APK URL."
                                .to_string(),
                            Color32::YELLOW,
                        );
                    } else {
                        self.log(
                            format!("Found {} app(s) on F-Droid", n),
                            Color32::from_rgb(100, 220, 140),
                        );
                    }
                }
                WorkerMsg::SearchDone(Err(e)) => {
                    self.searching = false;
                    self.log(e, Color32::from_rgb(255, 100, 100));
                }
                WorkerMsg::DownloadDone(Ok(path)) => {
                    self.downloading = false;
                    self.last_apk = Some(path.clone());
                    self.log(
                        format!("Downloaded → {}", path.display()),
                        Color32::from_rgb(100, 220, 140),
                    );
                }
                WorkerMsg::DownloadDone(Err(e)) => {
                    self.downloading = false;
                    self.log(e, Color32::from_rgb(255, 100, 100));
                }
                WorkerMsg::InstallDone(Ok(())) => {
                    self.installing = false;
                    self.log(
                        "Installed on phone!".to_string(),
                        Color32::from_rgb(100, 220, 140),
                    );
                }
                WorkerMsg::InstallDone(Err(e)) => {
                    self.installing = false;
                    self.log(e, Color32::from_rgb(255, 100, 100));
                }
                WorkerMsg::Log(s) => self.log(s, Color32::LIGHT_GRAY),
            }
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll();
        if self.searching || self.downloading || self.installing {
            ctx.request_repaint_after(std::time::Duration::from_millis(80));
        }

        egui::TopBottomPanel::top("top").show(ctx, |ui| {
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.add_space(12.0);
                ui.heading(RichText::new("APK Install Tool").strong());
                ui.label(
                    RichText::new("  online search → download → ADB install")
                        .size(13.0)
                        .color(Color32::from_rgb(140, 140, 160)),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.add_space(12.0);
                    if ui.button("⟳ Devices").clicked() {
                        self.refresh_devices();
                    }
                });
            });
            ui.add_space(4.0);
        });

        egui::TopBottomPanel::bottom("log")
            .resizable(true)
            .default_height(150.0)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Log").strong().size(14.0));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button("Clear").clicked() {
                            self.log.clear();
                        }
                    });
                });
                ui.separator();
                ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .stick_to_bottom(true)
                    .show(ui, |ui| {
                        for (msg, color) in &self.log {
                            ui.label(RichText::new(msg).color(*color).size(13.0));
                        }
                    });
            });

        egui::SidePanel::left("left")
            .resizable(true)
            .default_width(420.0)
            .min_width(300.0)
            .show(ctx, |ui| {
                ui.add_space(10.0);

                ui.label(
                    RichText::new("Device")
                        .strong()
                        .size(13.0)
                        .color(Color32::from_rgb(160, 160, 180)),
                );
                ui.add_space(4.0);
                if self.devices.is_empty() {
                    ui.label(RichText::new("No device").color(Color32::from_rgb(255, 140, 100)));
                } else {
                    egui::ComboBox::from_id_salt("device")
                        .selected_text(
                            self.selected_device
                                .and_then(|i| self.devices.get(i))
                                .cloned()
                                .unwrap_or_default(),
                        )
                        .width(ui.available_width() - 20.0)
                        .show_ui(ui, |ui| {
                            for (i, d) in self.devices.iter().enumerate() {
                                ui.selectable_value(&mut self.selected_device, Some(i), d);
                            }
                        });
                }

                ui.add_space(14.0);
                ui.label(
                    RichText::new("Search online (F-Droid)")
                        .strong()
                        .size(13.0)
                        .color(Color32::from_rgb(160, 160, 180)),
                );
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    let resp = ui.add(
                        egui::TextEdit::singleline(&mut self.search)
                            .hint_text("telegram, firefox, newpipe…")
                            .desired_width(ui.available_width() - 90.0),
                    );
                    if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        self.do_search();
                    }
                    let btn = ui.add_enabled(
                        !self.searching,
                        egui::Button::new(if self.searching { "…" } else { "Search" })
                            .min_size(Vec2::new(70.0, 0.0)),
                    );
                    if btn.clicked() {
                        self.do_search();
                    }
                });

                ui.add_space(10.0);
                ui.separator();
                ui.add_space(6.0);
                ui.label(
                    RichText::new(format!("Results ({})", self.results.len()))
                        .strong()
                        .size(13.0)
                        .color(Color32::from_rgb(160, 160, 180)),
                );
                ui.add_space(4.0);

                ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        if self.results.is_empty() && !self.searching {
                            ui.label(
                                RichText::new("No results yet")
                                    .italics()
                                    .color(Color32::from_rgb(120, 120, 140)),
                            );
                        }
                        for i in 0..self.results.len() {
                            let selected = self.selected == Some(i);
                            let hit = &self.results[i];
                            let text = format!("{}  [{}]\n{}", hit.name, hit.source, hit.package);
                            let resp = ui.selectable_label(selected, text);
                            if resp.clicked() {
                                self.selected = Some(i);
                            }
                            if resp.double_clicked() {
                                self.selected = Some(i);
                                self.do_download_selected();
                            }
                        }
                    });
            });

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.add_space(14.0);

            ui.label(
                RichText::new("Selected")
                    .size(13.0)
                    .color(Color32::from_rgb(160, 160, 180)),
            );
            ui.add_space(4.0);
            let sel_text = self
                .selected
                .and_then(|i| self.results.get(i))
                .map(|h| format!("{}\n{}", h.name, h.package))
                .unwrap_or_else(|| "— none —".to_string());
            ui.label(
                RichText::new(&sel_text)
                    .size(17.0)
                    .strong()
                    .color(if self.selected.is_some() {
                        Color32::from_rgb(130, 180, 255)
                    } else {
                        Color32::from_rgb(100, 100, 120)
                    }),
            );

            if let Some(i) = self.selected {
                if let Some(h) = self.results.get(i) {
                    if !h.summary.is_empty() {
                        ui.add_space(6.0);
                        ui.label(
                            RichText::new(&h.summary)
                                .size(13.0)
                                .color(Color32::from_rgb(160, 160, 175)),
                        );
                    }
                }
            }

            ui.add_space(16.0);
            ui.separator();
            ui.add_space(12.0);

            ui.horizontal(|ui| {
                ui.label("Save to:");
                ui.label(
                    RichText::new(self.output_dir.display().to_string())
                        .color(Color32::from_rgb(180, 180, 200)),
                );
                if ui.button("Change…").clicked() {
                    if let Some(p) = rfd::FileDialog::new().pick_folder() {
                        self.output_dir = p;
                    }
                }
            });

            ui.add_space(18.0);
            ui.vertical_centered(|ui| {
                let can_dl = self.selected.is_some() && !self.downloading && !self.installing;
                let label = if self.downloading {
                    "Downloading…"
                } else {
                    "⬇  Download APK"
                };
                if ui
                    .add_enabled(
                        can_dl,
                        egui::Button::new(RichText::new(label).size(16.0))
                            .min_size(Vec2::new(240.0, 40.0)),
                    )
                    .clicked()
                {
                    self.do_download_selected();
                }

                ui.add_space(12.0);

                let can_inst = self.last_apk.is_some() && !self.downloading && !self.installing;
                let ilabel = if self.installing {
                    "Installing…"
                } else {
                    "⬆  Install on phone"
                };
                if ui
                    .add_enabled(
                        can_inst,
                        egui::Button::new(RichText::new(ilabel).size(16.0))
                            .min_size(Vec2::new(240.0, 40.0)),
                    )
                    .clicked()
                {
                    self.do_install();
                }

                ui.add_space(18.0);
                if let Some(ref p) = self.last_apk {
                    ui.label(
                        RichText::new(format!(
                            "Last APK: {}",
                            p.file_name().unwrap_or_default().to_string_lossy()
                        ))
                        .size(13.0)
                        .color(Color32::from_rgb(140, 200, 140)),
                    );
                    if ui.button("Open folder").clicked() {
                        let _ = open::that(&self.output_dir);
                    }
                }
            });

            ui.add_space(20.0);
            ui.separator();
            ui.add_space(10.0);
            ui.label(
                RichText::new("Or paste direct APK URL")
                    .strong()
                    .size(13.0)
                    .color(Color32::from_rgb(160, 160, 180)),
            );
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut self.direct_url)
                        .hint_text("https://…/app.apk")
                        .desired_width(ui.available_width() - 100.0),
                );
                if ui
                    .add_enabled(!self.downloading, egui::Button::new("Get URL"))
                    .clicked()
                {
                    self.do_download_url();
                }
            });
            ui.add_space(8.0);
            ui.label(
                RichText::new(
                    "F-Droid = open-source. For PUBG etc.: paste APK URL from APKMirror.",
                )
                .size(12.0)
                .italics()
                .color(Color32::from_rgb(110, 110, 130)),
            );
        });
    }
}

fn http_get_json<T: for<'de> Deserialize<'de>>(url: &str) -> Result<T, String> {
    let client = reqwest::blocking::Client::builder()
        .user_agent("apk-adb-tool/0.3")
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| e.to_string())?;
    let resp = client.get(url).send().map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("HTTP {} for {}", resp.status(), url));
    }
    resp.json::<T>().map_err(|e| e.to_string())
}

fn download_file(url: &str, dest: &PathBuf) -> Result<(), String> {
    let client = reqwest::blocking::Client::builder()
        .user_agent("apk-adb-tool/0.3")
        .timeout(std::time::Duration::from_secs(300))
        .build()
        .map_err(|e| e.to_string())?;
    let mut resp = client.get(url).send().map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("Download HTTP {} — {}", resp.status(), url));
    }
    let mut file = File::create(dest).map_err(|e| e.to_string())?;
    let mut buf = Vec::new();
    resp.copy_to(&mut buf).map_err(|e| e.to_string())?;
    if buf.len() < 1000 {
        return Err("Downloaded file too small (not an APK?)".into());
    }
    file.write_all(&buf).map_err(|e| e.to_string())?;
    Ok(())
}

fn run_adb(serial: Option<&str>, args: &[&str]) -> Result<String, String> {
    let mut full = Vec::new();
    if let Some(s) = serial {
        full.push("-s");
        full.push(s);
    }
    full.extend_from_slice(args);
    let output = Command::new("adb")
        .args(&full)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| format!("Failed to run adb: {}", e))?;
    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        return Err(format!("adb {:?} → {}", full, err.trim()));
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

fn run_adb_status(
    serial: Option<&str>,
    args: &[&str],
) -> Result<std::process::ExitStatus, String> {
    let mut full = Vec::new();
    if let Some(s) = serial {
        full.push("-s");
        full.push(s);
    }
    full.extend_from_slice(args);
    Command::new("adb")
        .args(&full)
        .status()
        .map_err(|e| format!("Failed to run adb: {}", e))
}
