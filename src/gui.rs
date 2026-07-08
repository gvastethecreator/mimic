use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use eframe::egui;
use rfd::FileDialog;

use crate::compositor::{FrameCompositor, PipPosition};
use crate::decoder::MediaDecoder;
use crate::setup;
use crate::webcam::{self, WebcamCapture};


pub enum SetupMessage {
    FfmpegProgress(f32),
    FfmpegSuccess(PathBuf),
    FfmpegFailure(String),
    DriverProgress(f32),
    DriverSuccess(PathBuf),
    DriverFailure(String),
}

#[derive(serde::Serialize, serde::Deserialize, Clone)]
pub struct AppConfig {
    pub playlist: Vec<PathBuf>,
    pub selected_webcam: Option<String>,
    pub pip_enabled: bool,
    pub pip_position: PipPosition,
    pub pip_border_radius: u32,
    pub pip_scale: f32,
    pub loop_playlist: bool,
    pub output_resolution_index: usize, // 0: 1280x720, 1: 1920x1080, 2: 640x480
    pub output_fps_index: usize,        // 0: 30 FPS, 1: 60 FPS
    pub current_index: Option<usize>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            playlist: Vec::new(),
            selected_webcam: None,
            pip_enabled: false,
            pip_position: PipPosition::BottomRight,
            pip_border_radius: 16,
            pip_scale: 0.25,
            loop_playlist: true,
            output_resolution_index: 0, // 1280x720
            output_fps_index: 0,        // 30 FPS
            current_index: None,
        }
    }
}

pub struct VicamApp {
    config: AppConfig,
    config_path: PathBuf,
    
    // Environment states
    ffmpeg_path: Option<PathBuf>,
    driver_registered: bool,
    
    // Async download indicators
    downloading_ffmpeg: bool,
    ffmpeg_progress: f32,
    downloading_driver: bool,
    driver_progress: f32,
    error_msg: Option<String>,
    
    // Active streams & pipeline
    decoder: Option<MediaDecoder>,
    webcam_capture: Option<WebcamCapture>,
    compositor: FrameCompositor,
    is_streaming: bool,
    
    // UI states
    available_webcams: Vec<String>,
    preview_texture: Option<egui::TextureHandle>,
    last_composited_frame: Option<Vec<u8>>,
    
    // Timeline support
    seek_drag_val: Option<f32>,
    last_ui_update: Instant,

    // Background thread channels
    setup_tx: Sender<SetupMessage>,
    setup_rx: Receiver<SetupMessage>,
}

impl VicamApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // Set beautiful dark styling
        let mut visuals = egui::Visuals::dark();
        visuals.widgets.noninteractive.bg_fill = egui::Color32::from_rgb(20, 20, 25);
        visuals.widgets.inactive.bg_fill = egui::Color32::from_rgb(30, 30, 40);
        visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(45, 45, 60);
        visuals.widgets.active.bg_fill = egui::Color32::from_rgb(60, 60, 80);
        visuals.widgets.noninteractive.fg_stroke.color = egui::Color32::from_rgb(200, 200, 210);
        visuals.window_fill = egui::Color32::from_rgb(20, 20, 25);
        cc.egui_ctx.set_visuals(visuals);

        // Load configuration
        let config_path = setup::get_app_dir().join("config.json");
        let config = if config_path.exists() {
            std::fs::read_to_string(&config_path)
                .ok()
                .and_then(|s| serde_json::from_str::<AppConfig>(&s).ok())
                .unwrap_or_default()
        } else {
            AppConfig::default()
        };

        let ffmpeg_path = setup::get_ffmpeg_path();
        let driver_registered = setup::is_driver_registered();

        let (target_w, target_h, target_fps) = Self::get_res_fps_values(&config);
        let compositor = FrameCompositor::new(target_w, target_h, target_fps);

        let (setup_tx, setup_rx) = channel::<SetupMessage>();

        let mut app = Self {
            config,
            config_path,
            ffmpeg_path,
            driver_registered,
            downloading_ffmpeg: false,
            ffmpeg_progress: 0.0,
            downloading_driver: false,
            driver_progress: 0.0,
            error_msg: None,
            decoder: None,
            webcam_capture: None,
            compositor,
            is_streaming: false,
            available_webcams: Vec::new(),
            preview_texture: None,
            last_composited_frame: None,
            seek_drag_val: None,
            last_ui_update: Instant::now(),
            setup_tx,
            setup_rx,
        };

        app.refresh_webcam_list();
        app.start_selected_webcam();

        app
    }

    fn get_res_fps_values(config: &AppConfig) -> (u32, u32, f32) {
        let (w, h) = match config.output_resolution_index {
            1 => (1920, 1080),
            2 => (640, 480),
            _ => (1280, 720),
        };
        let fps = match config.output_fps_index {
            1 => 60.0,
            _ => 30.0,
        };
        (w, h, fps)
    }

    fn save_config(&self) {
        if let Ok(json) = serde_json::to_string_pretty(&self.config) {
            let _ = std::fs::write(&self.config_path, json);
        }
    }

    fn refresh_webcam_list(&mut self) {
        if let Some(ref path) = self.ffmpeg_path {
            self.available_webcams = webcam::list_webcams(path);
        }
    }

    fn start_selected_webcam(&mut self) {
        // Stop current webcam
        if let Some(wc) = self.webcam_capture.take() {
            wc.stop();
        }

        if !self.config.pip_enabled {
            return;
        }

        let device_name = match &self.config.selected_webcam {
            Some(name) => name,
            None => return,
        };

        if let Some(ref ffmpeg) = self.ffmpeg_path {
            // Let's capture webcam PIP at 320x240 @ 30 FPS
            match WebcamCapture::new(ffmpeg, device_name, 320, 240, 30.0) {
                Ok(capture) => {
                    self.webcam_capture = Some(capture);
                }
                Err(e) => {
                    self.error_msg = Some(format!("Webcam capture error: {}", e));
                }
            }
        }
    }

    fn update_pipeline_dimensions(&mut self) {
        let (w, h, fps) = Self::get_res_fps_values(&self.config);
        
        // Stop currently playing
        if let Some(dec) = self.decoder.take() {
            dec.stop();
        }
        
        self.compositor.release();
        self.compositor = FrameCompositor::new(w, h, fps);
        
        if self.is_streaming {
            if let Err(e) = self.compositor.init_camera() {
                self.error_msg = Some(e);
                self.is_streaming = false;
            }
        }

        self.play_current_playlist_item();
    }

    fn play_current_playlist_item(&mut self) {
        if let Some(dec) = self.decoder.take() {
            dec.stop();
        }

        let idx = match self.config.current_index {
            Some(i) => i,
            None => return,
        };

        if idx >= self.config.playlist.len() {
            return;
        }

        let file_path = &self.config.playlist[idx];
        if !file_path.exists() {
            self.error_msg = Some(format!("File not found: {:?}", file_path));
            return;
        }

        let (target_w, target_h, target_fps) = Self::get_res_fps_values(&self.config);
        
        if let Some(ref ffmpeg) = self.ffmpeg_path {
            match MediaDecoder::new(ffmpeg, file_path, target_w, target_h, target_fps) {
                Ok(dec) => {
                    dec.play();
                    self.decoder = Some(dec);
                }
                Err(e) => {
                    self.error_msg = Some(format!("Failed to load media: {}", e));
                }
            }
        }
    }
}

impl eframe::App for VicamApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Process background setup messages
        while let Ok(msg) = self.setup_rx.try_recv() {
            match msg {
                SetupMessage::FfmpegProgress(p) => {
                    self.ffmpeg_progress = p;
                }
                SetupMessage::FfmpegSuccess(path) => {
                    self.ffmpeg_path = Some(path);
                    self.downloading_ffmpeg = false;
                }
                SetupMessage::FfmpegFailure(err) => {
                    self.error_msg = Some(format!("FFmpeg download failed: {}", err));
                    self.downloading_ffmpeg = false;
                }
                SetupMessage::DriverProgress(p) => {
                    self.driver_progress = p;
                }
                SetupMessage::DriverSuccess(_path) => {
                    self.driver_registered = true;
                    self.downloading_driver = false;
                }
                SetupMessage::DriverFailure(err) => {
                    self.error_msg = Some(format!("Driver installation failed: {}", err));
                    self.downloading_driver = false;
                }
            }
        }

        // Request continuous rendering for live video playback
        ctx.request_repaint();

        // 1. Process new video/image frames & webcam frames
        let mut main_frame_data: Option<Vec<u8>> = None;
        let mut current_pos = Duration::ZERO;
        let mut duration = Duration::ZERO;

        if let Some(ref dec) = self.decoder {
            if let Some(frame) = dec.next_frame() {
                main_frame_data = Some(frame.data);
            } else if let Some(ref last) = self.last_composited_frame {
                // If decoder hasn't produced a new frame, keep displaying last frame
                main_frame_data = Some(last.clone());
            }
            current_pos = dec.current_time();
            duration = dec.metadata().duration;
        } else {
            // Draw a black screen if no media is playing
            let (w, h, _) = Self::get_res_fps_values(&self.config);
            main_frame_data = Some(vec![0u8; (w * h * 3) as usize]);
        }

        let mut webcam_frame: Option<Vec<u8>> = None;
        if self.config.pip_enabled {
            if let Some(ref wc) = self.webcam_capture {
                if let Some(frame) = wc.next_frame() {
                    webcam_frame = Some(frame);
                }
            }
        }

        // Composite main frame + PIP overlay
        if let Some(ref main_bytes) = main_frame_data {
            let pip_frame_opt = if let Some(ref web_bytes) = webcam_frame {
                // PIP is at 320x240
                Some((web_bytes.as_slice(), 320u32, 240u32))
            } else {
                None
            };
            
            let composited = self.compositor.process_and_send(
                main_bytes,
                pip_frame_opt,
                self.config.pip_position,
                self.config.pip_border_radius,
            );

            // Re-upload to GPU texture for live preview canvas
            let (target_w, target_h, _) = Self::get_res_fps_values(&self.config);
            let mut preview_image = egui::ColorImage::new([target_w as usize, target_h as usize], egui::Color32::BLACK);
            
            for (i, rgb) in composited.chunks_exact(3).enumerate() {
                if i < preview_image.pixels.len() {
                    preview_image.pixels[i] = egui::Color32::from_rgb(rgb[0], rgb[1], rgb[2]);
                }
            }

            self.preview_texture = Some(ctx.load_texture(
                "live_preview",
                preview_image,
                egui::TextureOptions::LINEAR,
            ));
            
            self.last_composited_frame = Some(composited);
        }

        // 2. Drag & Drop file listener
        if !ctx.input(|i| i.raw.hovered_files.is_empty()) {
            ctx.request_repaint(); // Redraw immediately to show drop indicator
        }

        if !ctx.input(|i| i.raw.dropped_files.is_empty()) {
            let dropped = ctx.input(|i| i.raw.dropped_files.clone());
            for f in dropped {
                if let Some(path) = f.path {
                    self.config.playlist.push(path);
                    self.save_config();
                }
            }
            if self.config.current_index.is_none() && !self.config.playlist.is_empty() {
                self.config.current_index = Some(0);
                self.play_current_playlist_item();
            }
        }

        // 3. Layout Rendering
        egui::TopBottomPanel::top("top_bar").frame(egui::Frame::none().fill(egui::Color32::from_rgb(15, 15, 20)).inner_margin(8.0)).show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading("⚡ Vicam");
                ui.label("• Virtual Webcam Simulator");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    // Status Badge
                    if self.is_streaming {
                        ui.colored_label(egui::Color32::from_rgb(50, 220, 100), "● STREAMING ACTIVE");
                    } else {
                        ui.colored_label(egui::Color32::from_rgb(220, 100, 50), "○ STANDBY");
                    }
                });
            });
        });

        // Setup guide warning banners if dependencies are missing
        if self.ffmpeg_path.is_none() || !self.driver_registered {
            egui::TopBottomPanel::top("setup_banner").frame(egui::Frame::none().fill(egui::Color32::from_rgb(40, 20, 20)).inner_margin(12.0)).show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label("⚠️");
                    ui.vertical(|ui| {
                        if self.ffmpeg_path.is_none() {
                            ui.horizontal(|ui| {
                                ui.label("FFmpeg is required to decode video. ");
                                if self.downloading_ffmpeg {
                                    ui.add(egui::ProgressBar::new(self.ffmpeg_progress).text(format!("Downloading... {:.0}%", self.ffmpeg_progress * 100.0)));
                                } else {
                                    if ui.button("Download FFmpeg Automatically").clicked() {
                                        self.downloading_ffmpeg = true;
                                        self.ffmpeg_progress = 0.0;
                                        let tx = self.setup_tx.clone();
                                        let ctx_clone = ctx.clone();
                                        thread::spawn(move || {
                                            let tx_progress = tx.clone();
                                            let ctx_progress = ctx_clone.clone();
                                            let res = setup::download_ffmpeg(move |progress| {
                                                let _ = tx_progress.send(SetupMessage::FfmpegProgress(progress));
                                                ctx_progress.request_repaint();
                                            });
                                            match res {
                                                Ok(path) => {
                                                    let _ = tx.send(SetupMessage::FfmpegSuccess(path));
                                                }
                                                Err(e) => {
                                                    let _ = tx.send(SetupMessage::FfmpegFailure(e));
                                                }
                                            }
                                            ctx_clone.request_repaint();
                                        });
                                    }
                                }
                            });
                        }
                        if !self.driver_registered {
                            ui.horizontal(|ui| {
                                ui.label("Virtual Camera driver is not registered. ");
                                if self.downloading_driver {
                                    ui.add(egui::ProgressBar::new(self.driver_progress).text(format!("Registering... {:.0}%", self.driver_progress * 100.0)));
                                } else {
                                    if ui.button("Install & Register Virtual Camera (Unity Capture)").clicked() {
                                        self.downloading_driver = true;
                                        self.driver_progress = 0.0;
                                        let tx = self.setup_tx.clone();
                                        let ctx_clone = ctx.clone();
                                        thread::spawn(move || {
                                            let _ = tx.send(SetupMessage::DriverProgress(0.1));
                                            ctx_clone.request_repaint();
                                            
                                            let tx_progress = tx.clone();
                                            let ctx_progress = ctx_clone.clone();
                                            let res = setup::download_driver(move |progress| {
                                                let scaled = 0.1 + progress * 0.7;
                                                let _ = tx_progress.send(SetupMessage::DriverProgress(scaled));
                                                ctx_progress.request_repaint();
                                            });
                                            
                                            let dll_path = match res {
                                                Ok(path) => path,
                                                Err(e) => {
                                                    let _ = tx.send(SetupMessage::DriverFailure(e));
                                                    ctx_clone.request_repaint();
                                                    return;
                                                }
                                            };
                                            
                                            let _ = tx.send(SetupMessage::DriverProgress(0.9));
                                            ctx_clone.request_repaint();
                                            
                                            match setup::register_driver_elevated(&dll_path) {
                                                Ok(_) => {
                                                    let _ = tx.send(SetupMessage::DriverSuccess(dll_path));
                                                }
                                                Err(e) => {
                                                    let _ = tx.send(SetupMessage::DriverFailure(e));
                                                }
                                            }
                                            ctx_clone.request_repaint();
                                        });
                                    }
                                }
                            });
                        }
                    });
                });
            });
        }

        // Left sidebar for parameters & webcam overlay controls
        egui::SidePanel::left("left_sidebar").width_range(250.0..=300.0).frame(egui::Frame::none().fill(egui::Color32::from_rgb(25, 25, 30)).inner_margin(16.0)).show(ctx, |ui| {
            ui.heading("Configuration");
            ui.add_space(8.0);
            
            // Output device control
            ui.group(|ui| {
                ui.label("🖥️ OUTPUT STREAM");
                let btn_text = if self.is_streaming { "STOP VIRTUAL CAMERA" } else { "START VIRTUAL CAMERA" };
                let color = if self.is_streaming { egui::Color32::from_rgb(200, 50, 50) } else { egui::Color32::from_rgb(50, 150, 250) };
                
                if ui.add(egui::Button::new(btn_text).fill(color).min_size(egui::vec2(ui.available_width(), 32.0))).clicked() {
                    if self.is_streaming {
                        self.compositor.release();
                        self.is_streaming = false;
                    } else {
                        self.driver_registered = setup::is_driver_registered();
                        if !self.driver_registered {
                            self.error_msg = Some("Driver not installed! Please click the top banner installer first.".to_string());
                        } else {
                            match self.compositor.init_camera() {
                                Ok(_) => self.is_streaming = true,
                                Err(e) => self.error_msg = Some(e),
                            }
                        }
                    }
                }
                
                ui.add_space(8.0);
                ui.label("Resolution:");
                let res_prev = match self.config.output_resolution_index {
                    1 => "1920x1080 (1080p)",
                    2 => "640x480 (VGA)",
                    _ => "1280x720 (720p)",
                };
                
                egui::ComboBox::from_id_source("res_combo").selected_text(res_prev).show_ui(ui, |ui| {
                    if ui.selectable_value(&mut self.config.output_resolution_index, 0, "1280x720 (720p)").changed() {
                        self.save_config();
                        self.update_pipeline_dimensions();
                    }
                    if ui.selectable_value(&mut self.config.output_resolution_index, 1, "1920x1080 (1080p)").changed() {
                        self.save_config();
                        self.update_pipeline_dimensions();
                    }
                    if ui.selectable_value(&mut self.config.output_resolution_index, 2, "640x480 (VGA)").changed() {
                        self.save_config();
                        self.update_pipeline_dimensions();
                    }
                });

                ui.add_space(4.0);
                ui.label("Frame Rate:");
                let fps_prev = match self.config.output_fps_index {
                    1 => "60 FPS",
                    _ => "30 FPS",
                };
                
                egui::ComboBox::from_id_source("fps_combo").selected_text(fps_prev).show_ui(ui, |ui| {
                    if ui.selectable_value(&mut self.config.output_fps_index, 0, "30 FPS").changed() {
                        self.save_config();
                        self.update_pipeline_dimensions();
                    }
                    if ui.selectable_value(&mut self.config.output_fps_index, 1, "60 FPS").changed() {
                        self.save_config();
                        self.update_pipeline_dimensions();
                    }
                });
            });
            
            ui.add_space(12.0);

            // Webcam PIP Config
            ui.group(|ui| {
                ui.label("📷 PHYSICAL WEBCAM (PIP)");
                if ui.checkbox(&mut self.config.pip_enabled, "Overlay Physical Camera").changed() {
                    self.save_config();
                    self.start_selected_webcam();
                }
                
                if self.config.pip_enabled {
                    ui.add_space(6.0);
                    ui.label("Webcam source:");
                    
                    let webcam_prev = self.config.selected_webcam.clone().unwrap_or_else(|| "Select device...".to_string());
                    
                    ui.horizontal(|ui| {
                        let available_webcams = self.available_webcams.clone();
                        let current_selected = self.config.selected_webcam.clone();
                        egui::ComboBox::from_id_source("webcam_combo").selected_text(&webcam_prev).show_ui(ui, |ui| {
                            for cam in &available_webcams {
                                if ui.selectable_label(current_selected.as_ref() == Some(cam), cam).clicked() {
                                    self.config.selected_webcam = Some(cam.clone());
                                    self.save_config();
                                    self.start_selected_webcam();
                                }
                            }
                        });
                        
                        if ui.button("🔄").on_hover_text("Refresh device list").clicked() {
                            self.refresh_webcam_list();
                        }
                    });

                    ui.add_space(6.0);
                    ui.label("Position Corner:");
                    
                    ui.horizontal(|ui| {
                        if ui.selectable_label(self.config.pip_position == PipPosition::TopLeft, "↖").clicked() {
                            self.config.pip_position = PipPosition::TopLeft;
                            self.save_config();
                        }
                        if ui.selectable_label(self.config.pip_position == PipPosition::TopRight, "↗").clicked() {
                            self.config.pip_position = PipPosition::TopRight;
                            self.save_config();
                        }
                        if ui.selectable_label(self.config.pip_position == PipPosition::BottomLeft, "↙").clicked() {
                            self.config.pip_position = PipPosition::BottomLeft;
                            self.save_config();
                        }
                        if ui.selectable_label(self.config.pip_position == PipPosition::BottomRight, "↘").clicked() {
                            self.config.pip_position = PipPosition::BottomRight;
                            self.save_config();
                        }
                    });

                    ui.add_space(6.0);
                    ui.label("Corner Rounded Radius:");
                    if ui.add(egui::Slider::new(&mut self.config.pip_border_radius, 0..=30).text("px")).changed() {
                        self.save_config();
                    }
                }
            });

            // Display error box if present
            let mut dismiss_error = false;
            if let Some(ref err) = self.error_msg {
                ui.add_space(16.0);
                ui.group(|ui| {
                    ui.colored_label(egui::Color32::from_rgb(240, 80, 80), "⚠️ Error:");
                    ui.label(err);
                    if ui.small_button("Dismiss").clicked() {
                        dismiss_error = true;
                    }
                });
            }
            if dismiss_error {
                self.error_msg = None;
            }
        });

        // Right sidebar for Playlist Management
        egui::SidePanel::right("right_sidebar").width_range(200.0..=250.0).frame(egui::Frame::none().fill(egui::Color32::from_rgb(25, 25, 30)).inner_margin(16.0)).show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading("🎬 Playlist");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("➕ Add").clicked() {
                        let files = FileDialog::new()
                            .add_filter("Multimedia", &["mp4", "mkv", "avi", "mov", "gif", "png", "jpg", "jpeg"])
                            .pick_files();
                        if let Some(picked) = files {
                            for p in picked {
                                self.config.playlist.push(p);
                            }
                            self.save_config();
                            if self.config.current_index.is_none() && !self.config.playlist.is_empty() {
                                self.config.current_index = Some(0);
                                self.play_current_playlist_item();
                            }
                        }
                    }
                });
            });

            ui.add_space(8.0);
            
            // Scrollable list
            egui::ScrollArea::vertical().show(ui, |ui| {
                if self.config.playlist.is_empty() {
                    ui.centered_and_justified(|ui| {
                        ui.label("Drag files here\nor click Add to start.");
                    });
                } else {
                    let mut to_delete = None;
                    let playlist = self.config.playlist.clone();
                    let current_index = self.config.current_index;
                    for (i, path) in playlist.iter().enumerate() {
                        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("Unknown File");
                        let is_active = current_index == Some(i);
                        
                        ui.horizontal(|ui| {
                            let _text_color = if is_active {
                                egui::Color32::from_rgb(50, 180, 250)
                            } else {
                                egui::Color32::from_rgb(180, 180, 180)
                            };
                            
                            let label = ui.selectable_label(is_active, name);
                            if label.clicked() {
                                self.config.current_index = Some(i);
                                self.save_config();
                                self.play_current_playlist_item();
                            }
                            
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if ui.small_button("❌").clicked() {
                                    to_delete = Some(i);
                                }
                            });
                        });
                        ui.separator();
                    }

                    if let Some(del_idx) = to_delete {
                        self.config.playlist.remove(del_idx);
                        if self.config.playlist.is_empty() {
                            self.config.current_index = None;
                            if let Some(dec) = self.decoder.take() {
                                dec.stop();
                            }
                        } else if self.config.current_index == Some(del_idx) {
                            self.config.current_index = Some(del_idx.min(self.config.playlist.len() - 1));
                            self.play_current_playlist_item();
                        } else if let Some(curr) = self.config.current_index {
                            if curr > del_idx {
                                self.config.current_index = Some(curr - 1);
                            }
                        }
                        self.save_config();
                    }
                }
            });
        });

        // Bottom control timeline bar
        egui::TopBottomPanel::bottom("bottom_bar").frame(egui::Frame::none().fill(egui::Color32::from_rgb(25, 25, 30)).inner_margin(16.0)).show(ctx, |ui| {
            // Timeline Seeker
            ui.horizontal(|ui| {
                let current_sec = current_pos.as_secs_f32();
                let duration_sec = duration.as_secs_f32();
                
                ui.label(format!(
                    "{:02}:{:02}",
                    (current_sec / 60.0) as i32,
                    (current_sec % 60.0) as i32
                ));
                
                let mut seek_val = current_sec;
                let slider = egui::Slider::new(&mut seek_val, 0.0..=duration_sec).show_value(false);
                let response = ui.add_sized(egui::vec2(ui.available_width() - 80.0, 16.0), slider);
                
                if response.drag_started() {
                    self.seek_drag_val = Some(seek_val);
                } else if response.drag_stopped() {
                    if let Some(s) = self.seek_drag_val {
                        if let Some(ref dec) = self.decoder {
                            dec.seek(Duration::from_secs_f32(s));
                        }
                    }
                    self.seek_drag_val = None;
                } else if response.dragged() {
                    self.seek_drag_val = Some(seek_val);
                }
                
                ui.label(format!(
                    "{:02}:{:02}",
                    (duration_sec / 60.0) as i32,
                    (duration_sec % 60.0) as i32
                ));
            });

            ui.add_space(4.0);

            // Controls (Pause, Play, Previous, Next, Loop)
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    if ui.button("⏮").on_hover_text("Previous").clicked() {
                        if let Some(curr) = self.config.current_index {
                            if curr > 0 {
                                self.config.current_index = Some(curr - 1);
                                self.save_config();
                                self.play_current_playlist_item();
                            }
                        }
                    }
                    
                    let play_pause_icon = if let Some(ref dec) = self.decoder {
                        if dec.is_paused() { "▶" } else { "⏸" }
                    } else {
                        "▶"
                    };
                    
                    if ui.add_sized(egui::vec2(40.0, 24.0), egui::Button::new(play_pause_icon)).clicked() {
                        if let Some(ref dec) = self.decoder {
                            if dec.is_paused() {
                                dec.play();
                            } else {
                                dec.pause();
                            }
                        } else {
                            self.play_current_playlist_item();
                        }
                    }

                    if ui.button("⏭").on_hover_text("Next").clicked() {
                        if let Some(curr) = self.config.current_index {
                            if curr + 1 < self.config.playlist.len() {
                                self.config.current_index = Some(curr + 1);
                                self.save_config();
                                self.play_current_playlist_item();
                            }
                        }
                    }
                    
                    ui.checkbox(&mut self.config.loop_playlist, "Loop media");
                });
            });
        });

        // Central central workspace for live preview
        egui::CentralPanel::default().frame(egui::Frame::none().fill(egui::Color32::from_rgb(20, 20, 25)).inner_margin(24.0)).show(ctx, |ui| {
            // Check if hovered for drop
            let is_hovered = ctx.input(|i| !i.raw.hovered_files.is_empty());
            
            ui.vertical_centered(|ui| {
                ui.heading("Live Feed Monitor");
                ui.add_space(8.0);
                
                let rect_size = ui.available_size();
                // Maintain aspect ratio 16:9 for visual preview wrapper
                let canvas_w = rect_size.x;
                let canvas_h = rect_size.x * 9.0 / 16.0;
                
                let final_size = if canvas_h > rect_size.y {
                    egui::vec2(rect_size.y * 16.0 / 9.0, rect_size.y)
                } else {
                    egui::vec2(canvas_w, canvas_h)
                };

                let (rect, _) = ui.allocate_exact_size(final_size, egui::Sense::hover());
                
                // Draw drop indicator overlay or dynamic preview texture
                if is_hovered {
                    ui.painter().rect_filled(rect, 4.0, egui::Color32::from_rgba_unmultiplied(30, 80, 150, 100));
                    ui.painter().text(
                        rect.center(),
                        egui::Align2::CENTER_CENTER,
                        "📥 DROP MULTIMEDIA FILE TO ADD",
                        egui::FontId::proportional(18.0),
                        egui::Color32::WHITE,
                    );
                } else if let Some(ref texture) = self.preview_texture {
                    ui.painter().image(
                        texture.id(),
                        rect,
                        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                        egui::Color32::WHITE,
                    );
                } else {
                    ui.painter().rect_filled(rect, 4.0, egui::Color32::BLACK);
                    ui.painter().text(
                        rect.center(),
                        egui::Align2::CENTER_CENTER,
                        "NO MEDIA LOADED\nDrag and drop an image or video file",
                        egui::FontId::proportional(16.0),
                        egui::Color32::GRAY,
                    );
                }
            });
        });
    }
}
