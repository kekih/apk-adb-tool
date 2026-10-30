use eframe::egui::{self, Color32, RichText, ScrollArea, Sense, Vec2};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([920.0, 640.0])
            .with_min_inner_size([700.0, 480.0])
            .with_title("APK ADB Tool"),
        ..Default::default()
    };

    eframe::run_native(
        "APK ADB Tool",
        options,
        Box::new(|cc| {
            // Force dark theme
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

            // Slightly larger fonts
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

enum WorkerMsg {
    Devices(Result<Vec<String>, String>),
    Packages(Result<Vec<String>, String>),
    PullDone(Result<PathBuf, String>),
    InstallDone(Result<(), String>),
    Log(String),
}

struct App {
    // ADB / device
    adb_ok: Option<bool>,
    devices: Vec<String>,
    selected_device: Option<usize>,

    // Search
    search: String,
    third_party_only: bool,
    packages: Vec<String>,
    selected_pkg: Option<usize>,
    searching: bool,

    // Actions
    output_dir: PathBuf,
    pulling: bool,
    installing: bool,
    last_pulled: Option<PathBuf>,

    // Log
    log: Vec<(String, Color32)>,

    // Channel
    tx: Sender<WorkerMsg>,
    rx: Receiver<WorkerMsg>,
}

impl App {
    fn new() -> Self {
        let (tx, rx) = mpsc::channel();
        let mut app = Self {
            adb_ok: None,
            devices: vec![],
            selected_device: None,
            search: String::new(),
            third_party_only: true,
            packages: vec![],
            selected_pkg: None,
            searching: false,
            output_dir: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            pulling: false,
            installing: false,
            last_pulled: None,
            log: vec![],
            tx,
            rx,
        };
        app.refresh_devices();
        app
    }

    fn log(&mut self, msg: impl Into<String>, color: Color32) {
        self.log.push((msg.into(), color));
        if self.log.len() > 200 {
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
        thread::spawn(move || {
            match which::which("adb") {
                Ok(_) => {
                    let out = run_adb(None, &["devices"]);
                    match out {
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
                    }
                }
                Err(_) => {
                    let _ = tx.send(WorkerMsg::Devices(Err(
                        "ADB not found in PATH. Install Platform Tools.".into(),
                    )));
                }
            }
        });
    }

    fn do_search(&mut self) {
        if self.search.trim().is_empty() {
            self.log("Enter a search term".into(), Color32::YELLOW);
            return;
        }
        if self.devices.is_empty() {
            self.log("No device connected".into(), Color32::from_rgb(255, 100, 100));
            return;
        }
        self.searching = true;
        self.packages.clear();
        self.selected_pkg = None;
        self.log(format!("Searching for \"{}\"…", self.search.trim()), Color32::LIGHT_BLUE);

        let search = self.search.trim().to_lowercase();
        let third = self.third_party_only;
        let serial = self.serial().map(|s| s.to_string());
        let tx = self.tx.clone();

        thread::spawn(move || {
            let mut args = vec!["shell", "pm", "list", "packages"];
            if third {
                args.push("-3");
            }
            match run_adb(serial.as_deref(), &args) {
                Ok(raw) => {
                    let mut matches: Vec<String> = raw
                        .lines()
                        .filter_map(|line| {
                            let line = line.trim();
                            if !line.starts_with("package:") {
                                return None;
                            }
                            let pkg = line.trim_start_matches("package:").trim();
                            if pkg.to_lowercase().contains(&search) {
                                Some(pkg.to_string())
                            } else {
                                None
                            }
                        })
                        .collect();
                    matches.sort();
                    matches.dedup();
                    let _ = tx.send(WorkerMsg::Packages(Ok(matches)));
                }
                Err(e) => {
                    let _ = tx.send(WorkerMsg::Packages(Err(e)));
                }
            }
        });
    }

    fn do_pull(&mut self) {
        let Some(idx) = self.selected_pkg else {
            self.log("Select a package first".into(), Color32::YELLOW);
            return;
        };
        let package = self.packages[idx].clone();
        self.pulling = true;
        self.log(format!("Pulling {}…", package), Color32::LIGHT_BLUE);

        let serial = self.serial().map(|s| s.to_string());
        let out_dir = self.output_dir.clone();
        let tx = self.tx.clone();

        thread::spawn(move || {
            // Get path
            let path_res = run_adb(serial.as_deref(), &["shell", "pm", "path", &package]);
            let apk_on_device = match path_res {
                Ok(text) => text
                    .lines()
                    .find(|l| l.starts_with("package:"))
                    .map(|l| l.trim_start_matches("package:").trim().to_string()),
                Err(e) => {
                    let _ = tx.send(WorkerMsg::PullDone(Err(e)));
                    return;
                }
            };

            let Some(apk_path) = apk_on_device else {
                let _ = tx.send(WorkerMsg::PullDone(Err("Could not resolve APK path".into())));
                return;
            };

            let safe = package.replace('.', "_");
            let local = out_dir.join(format!("{}.apk", safe));

            let status = run_adb_status(serial.as_deref(), &["pull", &apk_path, local.to_str().unwrap()]);
            match status {
                Ok(s) if s.success() => {
                    let _ = tx.send(WorkerMsg::PullDone(Ok(local)));
                }
                Ok(_) => {
                    let _ = tx.send(WorkerMsg::PullDone(Err(
                        "adb pull failed (permission? try root)".into(),
                    )));
                }
                Err(e) => {
                    let _ = tx.send(WorkerMsg::PullDone(Err(e)));
                }
            }
        });
    }

    fn do_install(&mut self) {
        let Some(path) = self.last_pulled.clone() else {
            self.log("Pull an APK first".into(), Color32::YELLOW);
            return;
        };
        if !path.exists() {
            self.log("Pulled file no longer exists".into(), Color32::from_rgb(255, 100, 100));
            return;
        }
        self.installing = true;
        self.log(format!("Installing {}…", path.display()), Color32::LIGHT_BLUE);

        let serial = self.serial().map(|s| s.to_string());
        let tx = self.tx.clone();

        thread::spawn(move || {
            // try with -r -t first
            let st = run_adb_status(
                serial.as_deref(),
                &["install", "-r", "-t", path.to_str().unwrap()],
            );
            match st {
                Ok(s) if s.success() => {
                    let _ = tx.send(WorkerMsg::InstallDone(Ok(())));
                    return;
                }
                _ => {}
            }
            // retry without -t
            let st2 = run_adb_status(
                serial.as_deref(),
                &["install", "-r", path.to_str().unwrap()],
            );
            match st2 {
                Ok(s) if s.success() => {
                    let _ = tx.send(WorkerMsg::InstallDone(Ok(())));
                }
                Ok(_) => {
                    let _ = tx.send(WorkerMsg::InstallDone(Err("Install failed".into())));
                }
                Err(e) => {
                    let _ = tx.send(WorkerMsg::InstallDone(Err(e)));
                }
            }
        });
    }

    fn poll_messages(&mut self) {
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                WorkerMsg::Devices(Ok(list)) => {
                    self.adb_ok = Some(true);
                    self.devices = list;
                    if self.selected_device.is_none() && !self.devices.is_empty() {
                        self.selected_device = Some(0);
                    }
                    if self.devices.is_empty() {
                        self.log("No online devices".into(), Color32::YELLOW);
                    } else {
                        self.log(
                            format!("Found {} device(s)", self.devices.len()),
                            Color32::from_rgb(100, 220, 140),
                        );
                    }
                }
                WorkerMsg::Devices(Err(e)) => {
                    self.adb_ok = Some(false);
                    self.log(e, Color32::from_rgb(255, 100, 100));
                }
                WorkerMsg::Packages(Ok(list)) => {
                    self.searching = false;
                    self.packages = list;
                    self.log(
                        format!("Found {} package(s)", self.packages.len()),
                        Color32::from_rgb(100, 220, 140),
                    );
                }
                WorkerMsg::Packages(Err(e)) => {
                    self.searching = false;
                    self.log(e, Color32::from_rgb(255, 100, 100));
                }
                WorkerMsg::PullDone(Ok(path)) => {
                    self.pulling = false;
                    self.last_pulled = Some(path.clone());
                    self.log(
                        format!("Downloaded → {}", path.display()),
                        Color32::from_rgb(100, 220, 140),
                    );
                }
                WorkerMsg::PullDone(Err(e)) => {
                    self.pulling = false;
                    self.log(e, Color32::from_rgb(255, 100, 100));
                }
                WorkerMsg::InstallDone(Ok(())) => {
                    self.installing = false;
                    self.log("Installed successfully!".into(), Color32::from_rgb(100, 220, 140));
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
        self.poll_messages();

        // Keep UI responsive while workers run
        if self.searching || self.pulling || self.installing {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }

        // ── Top bar ──────────────────────────────────────────────
        egui::TopBottomPanel::top("top").show(ctx, |ui| {
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.add_space(12.0);
                ui.heading(RichText::new("APK ADB Tool").strong());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.add_space(12.0);
                    if ui
                        .button(RichText::new("⟳ Refresh devices").size(14.0))
                        .clicked()
                    {
                        self.refresh_devices();
                    }
                });
            });
            ui.add_space(4.0);
        });

        // ── Bottom log ───────────────────────────────────────────
        egui::TopBottomPanel::bottom("log")
            .resizable(true)
            .default_height(140.0)
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

        // ── Left panel: search + list ─────────────────────────────
        egui::SidePanel::left("left")
            .resizable(true)
            .default_width(380.0)
            .min_width(280.0)
            .show(ctx, |ui| {
                ui.add_space(10.0);

                // Device
                ui.label(RichText::new("Device").strong().size(13.0).color(Color32::from_rgb(160, 160, 180)));
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

                ui.add_space(12.0);

                // Search
                ui.label(RichText::new("Search").strong().size(13.0).color(Color32::from_rgb(160, 160, 180)));
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    let resp = ui.add(
                        egui::TextEdit::singleline(&mut self.search)
                            .hint_text("package name…")
                            .desired_width(ui.available_width() - 90.0),
                    );
                    if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        self.do_search();
                    }
                    let search_btn = ui.add_enabled(
                        !self.searching,
                        egui::Button::new(if self.searching { "…" } else { "Search" })
                            .min_size(Vec2::new(70.0, 0.0)),
                    );
                    if search_btn.clicked() {
                        self.do_search();
                    }
                });

                ui.add_space(6.0);
                ui.checkbox(&mut self.third_party_only, "Only third-party apps (-3)");

                ui.add_space(12.0);
                ui.separator();
                ui.add_space(6.0);

                // Results header
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(format!("Packages ({})", self.packages.len()))
                            .strong()
                            .size(13.0)
                            .color(Color32::from_rgb(160, 160, 180)),
                    );
                });
                ui.add_space(4.0);

                // Package list
                ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        if self.packages.is_empty() && !self.searching {
                            ui.label(
                                RichText::new("No results yet")
                                    .italics()
                                    .color(Color32::from_rgb(120, 120, 140)),
                            );
                        }
                        for (i, pkg) in self.packages.iter().enumerate() {
                            let selected = self.selected_pkg == Some(i);
                            let resp = ui.selectable_label(selected, pkg);
                            if resp.clicked() {
                                self.selected_pkg = Some(i);
                            }
                            if resp.double_clicked() {
                                self.selected_pkg = Some(i);
                                self.do_pull();
                            }
                        }
                    });
            });

        // ── Central panel: actions ────────────────────────────────
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.add_space(16.0);
            ui.vertical_centered(|ui| {
                ui.label(
                    RichText::new("Selected package")
                        .size(13.0)
                        .color(Color32::from_rgb(160, 160, 180)),
                );
                ui.add_space(6.0);

                let pkg_text = self
                    .selected_pkg
                    .and_then(|i| self.packages.get(i))
                    .map(|s| s.as_str())
                    .unwrap_or("— none —");

                ui.label(
                    RichText::new(pkg_text)
                        .size(18.0)
                        .strong()
                        .color(if self.selected_pkg.is_some() {
                            Color32::from_rgb(130, 180, 255)
                        } else {
                            Color32::from_rgb(100, 100, 120)
                        }),
                );
            });

            ui.add_space(20.0);
            ui.separator();
            ui.add_space(16.0);

            // Output dir
            ui.horizontal(|ui| {
                ui.label("Save APKs to:");
                ui.label(
                    RichText::new(self.output_dir.display().to_string())
                        .color(Color32::from_rgb(180, 180, 200)),
                );
                if ui.button("Change…").clicked() {
                    if let Some(path) = rfd::FileDialog::new().pick_folder() {
                        self.output_dir = path;
                    }
                }
            });

            ui.add_space(24.0);

            // Big action buttons
            ui.vertical_centered(|ui| {
                let can_pull = self.selected_pkg.is_some() && !self.pulling && !self.installing;
                let pull_label = if self.pulling {
                    "Downloading…"
                } else {
                    "⬇  Download APK"
                };
                if ui
                    .add_enabled(
                        can_pull,
                        egui::Button::new(RichText::new(pull_label).size(16.0))
                            .min_size(Vec2::new(220.0, 40.0)),
                    )
                    .clicked()
                {
                    self.do_pull();
                }

                ui.add_space(12.0);

                let can_install = self.last_pulled.is_some() && !self.pulling && !self.installing;
                let install_label = if self.installing {
                    "Installing…"
                } else {
                    "⬆  Install / Reinstall"
                };
                if ui
                    .add_enabled(
                        can_install,
                        egui::Button::new(RichText::new(install_label).size(16.0))
                            .min_size(Vec2::new(220.0, 40.0)),
                    )
                    .clicked()
                {
                    self.do_install();
                }

                ui.add_space(20.0);

                if let Some(ref path) = self.last_pulled {
                    ui.label(
                        RichText::new(format!("Last file: {}", path.file_name().unwrap_or_default().to_string_lossy()))
                            .size(13.0)
                            .color(Color32::from_rgb(140, 200, 140)),
                    );
                    ui.add_space(6.0);
                    if ui.button("Open folder").clicked() {
                        let _ = open::that(&self.output_dir);
                    }
                }
            });

            ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
                ui.add_space(8.0);
                ui.label(
                    RichText::new("Double-click a package to download immediately")
                        .size(12.0)
                        .italics()
                        .color(Color32::from_rgb(110, 110, 130)),
                );
            });
        });
    }
}

// ── ADB helpers ───────────────────────────────────────────────────

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

fn run_adb_status(serial: Option<&str>, args: &[&str]) -> Result<std::process::ExitStatus, String> {
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
