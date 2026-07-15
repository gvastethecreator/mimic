use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread;
use std::time::{Duration, Instant};

use eframe::egui;
use rfd::FileDialog;

use crate::compositor::{FrameCompositor, PipPosition};
use crate::config::{self, AppConfig, MEDIA_EXTENSIONS};
use crate::decoder::MediaDecoder;
use crate::setup::{self, VirtualCameraBackend};
use crate::webcam::{self, WebcamCapture};

const BACKGROUND: egui::Color32 = egui::Color32::from_rgb(14, 16, 20);
const PANEL: egui::Color32 = egui::Color32::from_rgb(20, 23, 29);
const CARD: egui::Color32 = egui::Color32::from_rgb(27, 31, 39);
const BORDER: egui::Color32 = egui::Color32::from_rgb(48, 55, 68);
const TEXT: egui::Color32 = egui::Color32::from_rgb(232, 235, 241);
const MUTED: egui::Color32 = egui::Color32::from_rgb(146, 154, 171);
const ACCENT: egui::Color32 = egui::Color32::from_rgb(74, 164, 255);
const SUCCESS: egui::Color32 = egui::Color32::from_rgb(80, 204, 145);
const WARNING: egui::Color32 = egui::Color32::from_rgb(244, 180, 76);
const DANGER: egui::Color32 = egui::Color32::from_rgb(244, 102, 110);

pub enum SetupMessage {
    FfmpegProgress(f32),
    FfmpegSuccess(PathBuf),
    FfmpegFailure(String),
    DriverProgress(f32),
    DriverSuccess,
    DriverFailure(String),
}

#[derive(Clone, Copy)]
enum NoticeLevel {
    Info,
    Success,
    Warning,
    Error,
}

#[derive(Clone)]
struct Notice {
    level: NoticeLevel,
    title: String,
    message: String,
}

pub struct MimicApp {
    config: AppConfig,
    config_path: PathBuf,
    ffmpeg_path: Option<PathBuf>,
    virtual_camera_backends: Vec<VirtualCameraBackend>,
    downloading_ffmpeg: bool,
    ffmpeg_progress: f32,
    installing_driver: bool,
    driver_progress: f32,
    notice: Option<Notice>,
    decoder: Option<MediaDecoder>,
    webcam_capture: Option<WebcamCapture>,
    compositor: FrameCompositor,
    available_webcams: Vec<String>,
    preview_texture: Option<egui::TextureHandle>,
    last_main_frame: Option<Vec<u8>>,
    last_webcam_frame: Option<Vec<u8>>,
    blank_frame: Vec<u8>,
    preview_dirty: bool,
    seek_drag_value: Option<f32>,
    last_frame_tick: Instant,
    setup_tx: Sender<SetupMessage>,
    setup_rx: Receiver<SetupMessage>,
}

impl MimicApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        configure_style(&cc.egui_ctx);
        let config_path = setup::get_app_dir().join("config.json");
        let loaded = config::load(&config_path);
        let ffmpeg_path = setup::get_ffmpeg_path();
        let virtual_camera_backends = setup::available_virtual_camera_backends();
        let (width, height) = loaded.config.output_dimensions();
        let compositor = FrameCompositor::new(width, height, loaded.config.output_fps());
        let blank_frame = vec![0_u8; width as usize * height as usize * 3];
        let (setup_tx, setup_rx) = channel::<SetupMessage>();

        let mut app = Self {
            config: loaded.config,
            config_path,
            ffmpeg_path,
            virtual_camera_backends,
            downloading_ffmpeg: false,
            ffmpeg_progress: 0.0,
            installing_driver: false,
            driver_progress: 0.0,
            notice: loaded.warning.map(|message| Notice {
                level: NoticeLevel::Warning,
                title: "Settings recovered".to_string(),
                message,
            }),
            decoder: None,
            webcam_capture: None,
            compositor,
            available_webcams: Vec::new(),
            preview_texture: None,
            last_main_frame: None,
            last_webcam_frame: None,
            blank_frame,
            preview_dirty: false,
            seek_drag_value: None,
            last_frame_tick: Instant::now(),
            setup_tx,
            setup_rx,
        };
        app.refresh_webcam_list(false);
        app.start_selected_webcam(false);
        if app.config.current_index.is_some() {
            app.play_current_playlist_item();
        }
        app
    }

    fn persist_config(&mut self) {
        if let Err(error) = config::save(&self.config_path, &self.config) {
            self.set_notice(NoticeLevel::Error, "Settings were not saved", error);
        }
    }

    fn set_notice(
        &mut self,
        level: NoticeLevel,
        title: impl Into<String>,
        message: impl Into<String>,
    ) {
        self.notice = Some(Notice {
            level,
            title: title.into(),
            message: message.into(),
        });
    }

    fn refresh_environment(&mut self) {
        self.ffmpeg_path = setup::get_ffmpeg_path();
        self.virtual_camera_backends = setup::available_virtual_camera_backends();
    }

    fn refresh_setup(&mut self) {
        self.refresh_environment();
        if self.ffmpeg_path.is_some() {
            self.refresh_webcam_list(false);
            if self.decoder.is_none() {
                self.play_current_playlist_item();
            }
        }

        if self.ffmpeg_path.is_some() && !self.virtual_camera_backends.is_empty() {
            self.set_notice(
                NoticeLevel::Success,
                "Setup is ready",
                "FFmpeg and a supported virtual-camera backend were detected.",
            );
        } else {
            let mut missing = Vec::new();
            if self.ffmpeg_path.is_none() {
                missing.push("FFmpeg was not detected.");
            }
            if self.virtual_camera_backends.is_empty() {
                missing.push("No supported virtual-camera backend was detected.");
            }
            self.set_notice(
                NoticeLevel::Warning,
                "Setup still needs attention",
                missing.join(" "),
            );
        }
    }

    fn refresh_webcam_list(&mut self, report_result: bool) {
        let Some(ffmpeg) = self.ffmpeg_path.as_ref() else {
            self.available_webcams.clear();
            return;
        };
        match webcam::list_webcams(ffmpeg) {
            Ok(devices) => {
                self.available_webcams = devices;
                if report_result {
                    let message = if self.available_webcams.is_empty() {
                        "No physical cameras were reported by DirectShow.".to_string()
                    } else {
                        format!(
                            "Found {} physical camera{}.",
                            self.available_webcams.len(),
                            if self.available_webcams.len() == 1 {
                                ""
                            } else {
                                "s"
                            }
                        )
                    };
                    self.set_notice(NoticeLevel::Info, "Camera scan complete", message);
                }
            }
            Err(error) => {
                self.available_webcams.clear();
                self.set_notice(NoticeLevel::Error, "Camera scan failed", error);
            }
        }
    }

    fn start_selected_webcam(&mut self, report_missing_selection: bool) {
        if let Some(capture) = self.webcam_capture.take() {
            capture.stop();
        }
        self.last_webcam_frame = None;
        self.preview_dirty = true;
        if !self.config.pip_enabled {
            return;
        }

        let Some(device_name) = self.config.selected_webcam.clone() else {
            if report_missing_selection {
                self.set_notice(
                    NoticeLevel::Warning,
                    "Choose a physical camera",
                    "Picture-in-picture is enabled, but no webcam is selected.",
                );
            }
            return;
        };
        let Some(ffmpeg) = self.ffmpeg_path.as_ref() else {
            self.set_notice(
                NoticeLevel::Error,
                "FFmpeg is required",
                "Install FFmpeg before enabling physical-camera capture.",
            );
            return;
        };
        let (width, height) = self.config.pip_dimensions();
        match WebcamCapture::new(ffmpeg, &device_name, width, height, 30.0) {
            Ok(capture) => self.webcam_capture = Some(capture),
            Err(error) => self.set_notice(NoticeLevel::Error, "Webcam could not start", error),
        }
    }

    fn rebuild_pipeline(&mut self) {
        let was_streaming = self.compositor.is_active();
        if let Some(decoder) = self.decoder.take() {
            decoder.stop();
        }
        self.compositor.release();
        let (width, height) = self.config.output_dimensions();
        self.compositor = FrameCompositor::new(width, height, self.config.output_fps());
        self.blank_frame = vec![0_u8; width as usize * height as usize * 3];
        self.preview_texture = None;
        self.last_main_frame = None;
        self.last_webcam_frame = None;
        self.preview_dirty = false;
        self.start_selected_webcam(false);
        self.play_current_playlist_item();

        if was_streaming {
            match self.compositor.init_camera() {
                Ok(details) => self.set_notice(
                    NoticeLevel::Success,
                    "Output restarted",
                    format!("Sending to {} through {}.", details.device, details.backend),
                ),
                Err(error) => {
                    self.set_notice(NoticeLevel::Error, "Output could not restart", error)
                }
            }
        }
    }

    fn play_current_playlist_item(&mut self) {
        if let Some(decoder) = self.decoder.take() {
            decoder.stop();
        }
        self.last_main_frame = None;
        self.preview_texture = None;
        self.preview_dirty = false;

        let Some(index) = self.config.current_index else {
            return;
        };
        let Some(path) = self.config.playlist.get(index).cloned() else {
            self.config.current_index = None;
            self.persist_config();
            return;
        };
        if !path.exists() {
            self.set_notice(
                NoticeLevel::Error,
                "Media is unavailable",
                format!(
                    "{} could not be found. Reconnect its drive or remove it from the playlist.",
                    path.display()
                ),
            );
            return;
        }
        let Some(ffmpeg) = self.ffmpeg_path.as_ref() else {
            self.set_notice(
                NoticeLevel::Error,
                "FFmpeg is required",
                "Install FFmpeg to inspect and decode playlist media.",
            );
            return;
        };
        let (width, height) = self.config.output_dimensions();
        match MediaDecoder::new(ffmpeg, &path, width, height, self.config.output_fps()) {
            Ok(decoder) => self.decoder = Some(decoder),
            Err(error) => self.set_notice(NoticeLevel::Error, "Media could not load", error),
        }
    }

    fn select_playlist_item(&mut self, index: usize) {
        if index >= self.config.playlist.len() || self.config.current_index == Some(index) {
            return;
        }
        self.config.current_index = Some(index);
        self.persist_config();
        self.play_current_playlist_item();
    }

    fn remove_playlist_item(&mut self, index: usize) {
        let removed_current = self.config.current_index == Some(index);
        if self.config.remove_media(index) {
            self.persist_config();
            if removed_current {
                self.play_current_playlist_item();
            }
        }
    }

    fn add_media(&mut self, paths: Vec<PathBuf>) {
        let report = self.config.add_media(paths);
        if report.added > 0 {
            self.persist_config();
            if self.decoder.is_none() {
                self.play_current_playlist_item();
            }
        }
        if report.unsupported > 0 || report.duplicates > 0 {
            self.set_notice(
                NoticeLevel::Info,
                "Playlist updated",
                format!(
                    "Added {}. Skipped {} duplicate{} and {} unsupported file{}.",
                    report.added,
                    report.duplicates,
                    plural(report.duplicates),
                    report.unsupported,
                    plural(report.unsupported)
                ),
            );
        }
    }

    fn start_stream(&mut self) {
        self.refresh_environment();
        let blockers = self.stream_blockers();
        if !blockers.is_empty() {
            self.set_notice(
                NoticeLevel::Warning,
                "Output is not ready",
                blockers.join(" "),
            );
            return;
        }
        match self.compositor.init_camera() {
            Ok(details) => self.set_notice(
                NoticeLevel::Success,
                "Virtual camera is live",
                format!("Sending to {} through {}.", details.device, details.backend),
            ),
            Err(error) => {
                self.refresh_environment();
                self.set_notice(NoticeLevel::Error, "Virtual camera could not start", error);
            }
        }
    }

    fn stop_stream(&mut self, show_notice: bool) {
        let was_active = self.compositor.is_active();
        self.compositor.release();
        if show_notice && was_active {
            self.set_notice(
                NoticeLevel::Info,
                "Virtual camera stopped",
                "Preview playback remains available inside Mimic.",
            );
        }
    }

    fn stream_blockers(&self) -> Vec<String> {
        let mut blockers = Vec::new();
        if self.ffmpeg_path.is_none() {
            blockers.push("FFmpeg is not ready.".to_string());
        }
        if self.virtual_camera_backends.is_empty() {
            blockers.push("Install OBS Virtual Camera or Unity Capture.".to_string());
        }
        if self.config.current_index.is_none() || self.config.playlist.is_empty() {
            blockers.push("Add and select a media file.".to_string());
        } else if self
            .config
            .current_index
            .and_then(|index| self.config.playlist.get(index))
            .is_some_and(|path| !path.exists())
        {
            blockers.push("The selected media file is unavailable.".to_string());
        }
        if self.decoder.is_none() {
            blockers.push("Wait for the selected media to load.".to_string());
        }
        blockers
    }

    fn handle_setup_messages(&mut self) {
        while let Ok(message) = self.setup_rx.try_recv() {
            match message {
                SetupMessage::FfmpegProgress(progress) => self.ffmpeg_progress = progress,
                SetupMessage::FfmpegSuccess(path) => {
                    self.ffmpeg_path = Some(path);
                    self.downloading_ffmpeg = false;
                    self.refresh_webcam_list(false);
                    self.start_selected_webcam(false);
                    self.play_current_playlist_item();
                    self.set_notice(
                        NoticeLevel::Success,
                        "FFmpeg is ready",
                        "Media inspection and decoding are now available.",
                    );
                }
                SetupMessage::FfmpegFailure(error) => {
                    self.downloading_ffmpeg = false;
                    self.set_notice(NoticeLevel::Error, "FFmpeg setup failed", error);
                }
                SetupMessage::DriverProgress(progress) => self.driver_progress = progress,
                SetupMessage::DriverSuccess => {
                    self.installing_driver = false;
                    self.virtual_camera_backends = setup::available_virtual_camera_backends();
                    self.set_notice(
                        NoticeLevel::Success,
                        "Virtual camera is ready",
                        "Unity Video Capture was registered successfully.",
                    );
                }
                SetupMessage::DriverFailure(error) => {
                    self.installing_driver = false;
                    self.refresh_environment();
                    self.set_notice(NoticeLevel::Error, "Driver setup failed", error);
                }
            }
        }
    }

    fn begin_ffmpeg_download(&mut self, context: &egui::Context) {
        if self.downloading_ffmpeg {
            return;
        }
        self.downloading_ffmpeg = true;
        self.ffmpeg_progress = 0.0;
        let sender = self.setup_tx.clone();
        let context = context.clone();
        thread::spawn(move || {
            let progress_sender = sender.clone();
            let progress_context = context.clone();
            let result = setup::download_ffmpeg(move |progress| {
                let _ = progress_sender.send(SetupMessage::FfmpegProgress(progress));
                progress_context.request_repaint();
            });
            let _ = sender.send(match result {
                Ok(path) => SetupMessage::FfmpegSuccess(path),
                Err(error) => SetupMessage::FfmpegFailure(error),
            });
            context.request_repaint();
        });
    }

    fn begin_driver_install(&mut self, context: &egui::Context) {
        if self.installing_driver {
            return;
        }
        self.installing_driver = true;
        self.driver_progress = 0.0;
        let sender = self.setup_tx.clone();
        let context = context.clone();
        thread::spawn(move || {
            let _ = sender.send(SetupMessage::DriverProgress(0.05));
            let progress_sender = sender.clone();
            let progress_context = context.clone();
            let downloaded = setup::download_driver(move |progress| {
                let _ = progress_sender.send(SetupMessage::DriverProgress(0.05 + progress * 0.75));
                progress_context.request_repaint();
            });
            let path = match downloaded {
                Ok(path) => path,
                Err(error) => {
                    let _ = sender.send(SetupMessage::DriverFailure(error));
                    context.request_repaint();
                    return;
                }
            };
            let _ = sender.send(SetupMessage::DriverProgress(0.85));
            let message = match setup::register_driver_elevated(&path) {
                Ok(true) => SetupMessage::DriverSuccess,
                Ok(false) => SetupMessage::DriverFailure(
                    "The registration process ended without confirming a device.".to_string(),
                ),
                Err(error) => SetupMessage::DriverFailure(error),
            };
            let _ = sender.send(message);
            context.request_repaint();
        });
    }

    fn handle_dropped_files(&mut self, context: &egui::Context) {
        let dropped = context.input(|input| input.raw.dropped_files.clone());
        let paths: Vec<PathBuf> = dropped.into_iter().filter_map(|file| file.path).collect();
        if !paths.is_empty() {
            self.add_media(paths);
        }
    }

    fn update_runtime(&mut self, context: &egui::Context) {
        let decoder_update = self.decoder.as_ref().map(MediaDecoder::poll);
        if let Some(update) = decoder_update {
            if let Some(frame) = update.latest_frame {
                self.last_main_frame = Some(frame);
                self.preview_dirty = true;
            }
            if let Some(error) = update.error {
                if let Some(decoder) = self.decoder.take() {
                    decoder.stop();
                }
                self.stop_stream(false);
                self.set_notice(NoticeLevel::Error, "Playback stopped", error);
            } else if update.ended {
                if let Some(decoder) = self.decoder.take() {
                    decoder.stop();
                }
                if self.config.advance_after_end() {
                    self.persist_config();
                    self.play_current_playlist_item();
                } else {
                    self.stop_stream(false);
                    self.set_notice(
                        NoticeLevel::Info,
                        "Playlist complete",
                        "Playback reached the final item. Enable playlist looping to restart automatically.",
                    );
                }
            }
        }

        if let Some(capture) = self.webcam_capture.as_ref() {
            let update = capture.poll();
            if let Some(frame) = update.latest_frame {
                self.last_webcam_frame = Some(frame);
                self.preview_dirty = true;
            }
            if let Some(error) = update.error {
                if let Some(capture) = self.webcam_capture.take() {
                    capture.stop();
                }
                self.last_webcam_frame = None;
                self.preview_dirty = true;
                self.set_notice(NoticeLevel::Error, "Webcam disconnected", error);
            }
        }

        let frame_interval = Duration::from_secs_f32(1.0 / self.config.output_fps());
        let elapsed = self.last_frame_tick.elapsed();
        if elapsed >= frame_interval {
            let is_streaming = self.compositor.is_active();
            let should_render = self.last_main_frame.is_some()
                && (self.preview_dirty || self.preview_texture.is_none());
            if is_streaming || should_render {
                let main_frame = self
                    .last_main_frame
                    .as_deref()
                    .unwrap_or(self.blank_frame.as_slice());
                let pip_frame = if self.config.pip_enabled {
                    self.webcam_capture.as_ref().and_then(|capture| {
                        let (width, height) = capture.dimensions();
                        self.last_webcam_frame
                            .as_deref()
                            .map(|frame| (frame, width, height))
                    })
                } else {
                    None
                };
                match self.compositor.process_and_send(
                    main_frame,
                    pip_frame,
                    self.config.pip_position,
                    self.config.pip_border_radius,
                ) {
                    Ok(composited) if should_render => {
                        self.update_preview_texture(context, &composited);
                        self.preview_dirty = false;
                    }
                    Ok(_) => {}
                    Err(error) => {
                        self.set_notice(NoticeLevel::Error, "Virtual camera output failed", error)
                    }
                }
            }
            self.last_frame_tick = Instant::now();
        }
        context
            .request_repaint_after(frame_interval.saturating_sub(self.last_frame_tick.elapsed()));
    }

    fn update_preview_texture(&mut self, context: &egui::Context, frame: &[u8]) {
        let (width, height) = self.config.output_dimensions();
        let image = egui::ColorImage::from_rgb([width as usize, height as usize], frame);
        if let Some(texture) = self.preview_texture.as_mut() {
            texture.set(image, egui::TextureOptions::LINEAR);
        } else {
            self.preview_texture = Some(context.load_texture(
                "mimic-live-preview",
                image,
                egui::TextureOptions::LINEAR,
            ));
        }
    }

    fn show_top_bar(&mut self, context: &egui::Context) {
        egui::TopBottomPanel::top("top_bar")
            .exact_height(54.0)
            .frame(
                egui::Frame::none()
                    .fill(egui::Color32::from_rgb(17, 19, 24))
                    .inner_margin(egui::Margin::symmetric(18.0, 10.0))
                    .stroke(egui::Stroke::new(1.0, BORDER)),
            )
            .show(context, |ui| {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("MIMIC").size(20.0).strong().color(TEXT));
                    ui.label(egui::RichText::new("Virtual camera studio").color(MUTED));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let (label, color) = if self.compositor.is_active() {
                            ("LIVE OUTPUT", SUCCESS)
                        } else if self.virtual_camera_backends.is_empty() {
                            ("SETUP NEEDED", WARNING)
                        } else {
                            ("BACKEND READY", ACCENT)
                        };
                        status_badge(ui, label, color);
                        if let Some(details) = self.compositor.camera_details() {
                            ui.label(
                                egui::RichText::new(format!(
                                    "{} / {}",
                                    details.device, details.backend
                                ))
                                .small()
                                .color(MUTED),
                            );
                        }
                    });
                });
            });
    }

    fn show_setup_banner(&mut self, context: &egui::Context) {
        if self.ffmpeg_path.is_some() && !self.virtual_camera_backends.is_empty() {
            return;
        }
        egui::TopBottomPanel::top("setup_banner")
            .frame(
                egui::Frame::none()
                    .fill(egui::Color32::from_rgb(40, 31, 20))
                    .inner_margin(egui::Margin::symmetric(18.0, 10.0))
                    .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(93, 68, 31))),
            )
            .show(context, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label(egui::RichText::new("Finish setup").strong().color(WARNING));
                    if self.ffmpeg_path.is_none() {
                        ui.label(egui::RichText::new("FFmpeg is required for media playback.").color(TEXT));
                        if self.downloading_ffmpeg {
                            ui.add(
                                egui::ProgressBar::new(self.ffmpeg_progress)
                                    .desired_width(170.0)
                                    .text(format!("FFmpeg {:.0}%", self.ffmpeg_progress * 100.0)),
                            );
                        } else if ui.button("Install verified FFmpeg").clicked() {
                            self.begin_ffmpeg_download(context);
                        }
                    }
                    if self.virtual_camera_backends.is_empty() {
                        ui.label(
                            egui::RichText::new(
                                "Install OBS Virtual Camera or the verified Unity Capture driver for output.",
                            )
                            .color(TEXT),
                        );
                        if self.installing_driver {
                            ui.add(
                                egui::ProgressBar::new(self.driver_progress)
                                    .desired_width(190.0)
                                    .text(if self.driver_progress >= 0.85 {
                                        "Waiting for administrator approval"
                                    } else {
                                        "Preparing Unity Capture"
                                    }),
                            );
                        } else if ui.button("Install Unity Capture").clicked() {
                            self.begin_driver_install(context);
                        }
                    }
                    if !self.downloading_ffmpeg
                        && !self.installing_driver
                        && ui.button("Refresh detection").clicked()
                    {
                        self.refresh_setup();
                    }
                });
            });
    }

    fn show_left_panel(&mut self, context: &egui::Context) {
        egui::SidePanel::left("controls")
            .exact_width(278.0)
            .resizable(false)
            .frame(
                egui::Frame::none()
                    .fill(PANEL)
                    .inner_margin(egui::Margin::same(16.0))
                    .stroke(egui::Stroke::new(1.0, BORDER)),
            )
            .show(context, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    section_heading(ui, "OUTPUT");
                    card(ui, |ui| self.show_output_controls(ui));
                    ui.add_space(16.0);
                    section_heading(ui, "CAMERA OVERLAY");
                    card(ui, |ui| self.show_webcam_controls(ui));
                });
            });
    }

    fn show_output_controls(&mut self, ui: &mut egui::Ui) {
        let streaming = self.compositor.is_active();
        if streaming {
            if ui
                .add_sized(
                    [ui.available_width(), 38.0],
                    egui::Button::new(egui::RichText::new("Stop virtual camera").strong())
                        .fill(egui::Color32::from_rgb(122, 42, 49)),
                )
                .clicked()
            {
                self.stop_stream(true);
            }
        } else {
            let blockers = self.stream_blockers();
            let response = ui.add_enabled(
                blockers.is_empty(),
                egui::Button::new(egui::RichText::new("Start virtual camera").strong())
                    .fill(egui::Color32::from_rgb(36, 106, 178))
                    .min_size(egui::vec2(ui.available_width(), 38.0)),
            );
            if response.clicked() {
                self.start_stream();
            }
            if let Some(blocker) = blockers.first() {
                ui.add_space(6.0);
                ui.label(egui::RichText::new(blocker).small().color(MUTED));
            }
        }

        ui.add_space(14.0);
        ui.label(egui::RichText::new("Format").strong().color(TEXT));
        let old_resolution = self.config.output_resolution_index;
        let old_fps = self.config.output_fps_index;
        ui.add_enabled_ui(!streaming, |ui| {
            egui::ComboBox::from_id_source("resolution")
                .selected_text(match self.config.output_resolution_index {
                    1 => "1920 × 1080",
                    2 => "640 × 480",
                    _ => "1280 × 720",
                })
                .width(ui.available_width())
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.config.output_resolution_index, 0, "1280 × 720");
                    ui.selectable_value(&mut self.config.output_resolution_index, 1, "1920 × 1080");
                    ui.selectable_value(&mut self.config.output_resolution_index, 2, "640 × 480");
                });
            egui::ComboBox::from_id_source("frame_rate")
                .selected_text(if self.config.output_fps_index == 1 {
                    "60 frames per second"
                } else {
                    "30 frames per second"
                })
                .width(ui.available_width())
                .show_ui(ui, |ui| {
                    ui.selectable_value(
                        &mut self.config.output_fps_index,
                        0,
                        "30 frames per second",
                    );
                    ui.selectable_value(
                        &mut self.config.output_fps_index,
                        1,
                        "60 frames per second",
                    );
                });
        });
        if old_resolution != self.config.output_resolution_index
            || old_fps != self.config.output_fps_index
        {
            self.persist_config();
            self.rebuild_pipeline();
        }
        if streaming {
            ui.label(
                egui::RichText::new("Stop output to change format.")
                    .small()
                    .color(MUTED),
            );
        }

        ui.add_space(12.0);
        ui.label(egui::RichText::new("Detected output").strong().color(TEXT));
        if self.virtual_camera_backends.is_empty() {
            ui.label(egui::RichText::new("No backend detected").color(WARNING));
        } else {
            for backend in &self.virtual_camera_backends {
                ui.label(egui::RichText::new(backend.label()).color(SUCCESS));
            }
        }
    }

    fn show_webcam_controls(&mut self, ui: &mut egui::Ui) {
        let enabled_response = ui.add_enabled(
            self.ffmpeg_path.is_some(),
            egui::Checkbox::new(&mut self.config.pip_enabled, "Enable picture-in-picture"),
        );
        if enabled_response.changed() {
            self.persist_config();
            self.start_selected_webcam(true);
        }
        if !self.config.pip_enabled {
            ui.label(
                egui::RichText::new("Add a physical camera over the selected media.")
                    .small()
                    .color(MUTED),
            );
            return;
        }

        ui.add_space(10.0);
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Source").strong().color(TEXT));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button("Refresh").clicked() {
                    self.refresh_webcam_list(true);
                }
            });
        });
        let selected_text = self
            .config
            .selected_webcam
            .as_deref()
            .unwrap_or("Choose a camera");
        let mut selected_camera = None;
        egui::ComboBox::from_id_source("physical_camera")
            .selected_text(selected_text)
            .width(ui.available_width())
            .show_ui(ui, |ui| {
                if self.available_webcams.is_empty() {
                    ui.label(egui::RichText::new("No cameras detected").color(MUTED));
                }
                for camera in &self.available_webcams {
                    if ui
                        .selectable_label(
                            self.config.selected_webcam.as_ref() == Some(camera),
                            camera,
                        )
                        .clicked()
                    {
                        selected_camera = Some(camera.clone());
                    }
                }
            });
        if let Some(camera) = selected_camera {
            self.config.selected_webcam = Some(camera);
            self.persist_config();
            self.start_selected_webcam(false);
        }

        ui.add_space(10.0);
        ui.label(egui::RichText::new("Placement").strong().color(TEXT));
        let previous_position = self.config.pip_position;
        egui::ComboBox::from_id_source("pip_position")
            .selected_text(pip_position_label(self.config.pip_position))
            .width(ui.available_width())
            .show_ui(ui, |ui| {
                ui.selectable_value(
                    &mut self.config.pip_position,
                    PipPosition::TopLeft,
                    "Top left",
                );
                ui.selectable_value(
                    &mut self.config.pip_position,
                    PipPosition::TopRight,
                    "Top right",
                );
                ui.selectable_value(
                    &mut self.config.pip_position,
                    PipPosition::BottomLeft,
                    "Bottom left",
                );
                ui.selectable_value(
                    &mut self.config.pip_position,
                    PipPosition::BottomRight,
                    "Bottom right",
                );
            });
        if previous_position != self.config.pip_position {
            self.persist_config();
            self.preview_dirty = true;
        }

        let scale_response = ui.add(
            egui::Slider::new(&mut self.config.pip_scale, 0.15..=0.45)
                .text("Size")
                .custom_formatter(|value, _| format!("{:.0}%", value * 100.0)),
        );
        if scale_response.changed() {
            self.persist_config();
        }
        if scale_response.drag_stopped() || (scale_response.changed() && !scale_response.dragged())
        {
            self.start_selected_webcam(false);
        }
        if ui
            .add(
                egui::Slider::new(&mut self.config.pip_border_radius, 0..=96).text("Corner radius"),
            )
            .changed()
        {
            self.persist_config();
            self.preview_dirty = true;
        }
    }

    fn show_playlist(&mut self, context: &egui::Context) {
        egui::SidePanel::right("playlist")
            .exact_width(252.0)
            .resizable(false)
            .frame(
                egui::Frame::none()
                    .fill(PANEL)
                    .inner_margin(egui::Margin::same(14.0))
                    .stroke(egui::Stroke::new(1.0, BORDER)),
            )
            .show(context, |ui| {
                section_heading(ui, "PLAYLIST");
                ui.add_space(6.0);
                if ui
                    .add_sized([ui.available_width(), 32.0], egui::Button::new("Add media"))
                    .clicked()
                {
                    let files = FileDialog::new()
                        .add_filter("Supported media", MEDIA_EXTENSIONS)
                        .pick_files();
                    if let Some(files) = files {
                        self.add_media(files);
                    }
                }
                ui.add_space(10.0);

                if self.config.playlist.is_empty() {
                    let available = ui.available_size();
                    ui.allocate_ui_with_layout(
                        egui::vec2(available.x, available.y.max(220.0)),
                        egui::Layout::top_down(egui::Align::Center),
                        |ui| {
                            ui.add_space(70.0);
                            ui.label(egui::RichText::new("No media yet").strong().color(TEXT));
                            ui.label(
                                egui::RichText::new(
                                    "Drop files onto the preview\nor use Add media.",
                                )
                                .color(MUTED),
                            );
                        },
                    );
                    return;
                }

                let mut selected = None;
                let mut removed = None;
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for (index, path) in self.config.playlist.clone().iter().enumerate() {
                        let active = self.config.current_index == Some(index);
                        let exists = path.exists();
                        let fill = if active {
                            egui::Color32::from_rgb(28, 52, 78)
                        } else {
                            CARD
                        };
                        egui::Frame::none()
                            .fill(fill)
                            .rounding(7.0)
                            .inner_margin(egui::Margin::same(9.0))
                            .stroke(egui::Stroke::new(1.0, if active { ACCENT } else { BORDER }))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    let color = if exists { TEXT } else { DANGER };
                                    let name = path
                                        .file_name()
                                        .and_then(|name| name.to_str())
                                        .unwrap_or("Unnamed media");
                                    let name_width = (ui.available_width() - 72.0).max(60.0);
                                    let label = ui
                                        .add_sized(
                                            [name_width, 24.0],
                                            egui::SelectableLabel::new(
                                                active,
                                                egui::RichText::new(name).color(color),
                                            ),
                                        )
                                        .on_hover_text(path.display().to_string());
                                    if label.clicked() {
                                        selected = Some(index);
                                    }
                                    if ui.small_button("Remove").clicked() {
                                        removed = Some(index);
                                    }
                                });
                                if !exists {
                                    ui.label(
                                        egui::RichText::new("File unavailable")
                                            .small()
                                            .color(DANGER),
                                    );
                                }
                            });
                        ui.add_space(7.0);
                    }
                });
                if let Some(index) = selected {
                    self.select_playlist_item(index);
                }
                if let Some(index) = removed {
                    self.remove_playlist_item(index);
                }

                ui.separator();
                ui.label(
                    egui::RichText::new(format!(
                        "{} item{}",
                        self.config.playlist.len(),
                        plural(self.config.playlist.len())
                    ))
                    .small()
                    .color(MUTED),
                );
            });
    }

    fn show_transport(&mut self, context: &egui::Context) {
        egui::TopBottomPanel::bottom("transport")
            .exact_height(86.0)
            .frame(
                egui::Frame::none()
                    .fill(PANEL)
                    .inner_margin(egui::Margin::symmetric(18.0, 10.0))
                    .stroke(egui::Stroke::new(1.0, BORDER)),
            )
            .show(context, |ui| {
                let current_time = self
                    .decoder
                    .as_ref()
                    .map(MediaDecoder::current_time)
                    .unwrap_or(Duration::ZERO);
                let duration = self
                    .decoder
                    .as_ref()
                    .map(|decoder| decoder.metadata().duration)
                    .unwrap_or(Duration::ZERO);
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(format_time(current_time))
                            .monospace()
                            .color(MUTED),
                    );
                    let mut seek = self.seek_drag_value.unwrap_or(current_time.as_secs_f32());
                    let slider =
                        egui::Slider::new(&mut seek, 0.0..=duration.as_secs_f32().max(0.01))
                            .show_value(false);
                    let response =
                        ui.add_enabled(self.decoder.is_some() && !duration.is_zero(), slider);
                    if response.dragged() {
                        self.seek_drag_value = Some(seek);
                    }
                    if response.drag_stopped() || (response.changed() && !response.dragged()) {
                        if let Some(decoder) = self.decoder.as_ref() {
                            decoder.seek(Duration::from_secs_f32(seek));
                        }
                        self.seek_drag_value = None;
                    }
                    ui.label(
                        egui::RichText::new(format_time(duration))
                            .monospace()
                            .color(MUTED),
                    );
                });
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    let current = self.config.current_index;
                    if ui
                        .add_enabled(
                            current.is_some_and(|index| index > 0),
                            egui::Button::new("Previous"),
                        )
                        .clicked()
                        && let Some(index) = current
                    {
                        self.config.current_index = Some(index - 1);
                        self.persist_config();
                        self.play_current_playlist_item();
                    }

                    let paused = self.decoder.as_ref().is_some_and(MediaDecoder::is_paused);
                    let finished = self.decoder.as_ref().is_some_and(MediaDecoder::is_finished);
                    let play_label = if self.decoder.is_none() || paused || finished {
                        "Play"
                    } else {
                        "Pause"
                    };
                    if ui
                        .add_enabled(
                            self.config.current_index.is_some(),
                            egui::Button::new(play_label),
                        )
                        .clicked()
                    {
                        if let Some(decoder) = self.decoder.as_ref() {
                            if paused {
                                decoder.play();
                            } else {
                                decoder.pause();
                            }
                        } else {
                            self.play_current_playlist_item();
                        }
                    }

                    if ui
                        .add_enabled(
                            current.is_some_and(|index| index + 1 < self.config.playlist.len()),
                            egui::Button::new("Next"),
                        )
                        .clicked()
                        && let Some(index) = current
                    {
                        self.config.current_index = Some(index + 1);
                        self.persist_config();
                        self.play_current_playlist_item();
                    }
                    ui.separator();
                    if ui
                        .checkbox(&mut self.config.loop_playlist, "Loop playlist")
                        .changed()
                    {
                        self.persist_config();
                    }
                });
            });
    }

    fn show_workspace(&mut self, context: &egui::Context) {
        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(BACKGROUND).inner_margin(18.0))
            .show(context, |ui| {
                self.show_notice(ui);
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("Preview").size(18.0).strong().color(TEXT));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let label = if self.decoder.is_some() {
                            if self.decoder.as_ref().is_some_and(MediaDecoder::is_paused) {
                                "PAUSED"
                            } else {
                                "PLAYING"
                            }
                        } else {
                            "IDLE"
                        };
                        status_badge(ui, label, if self.decoder.is_some() { ACCENT } else { MUTED });
                    });
                });
                ui.add_space(10.0);

                let (width, height) = self.config.output_dimensions();
                let aspect = width as f32 / height as f32;
                let available = ui.available_size();
                let canvas_width = available.x;
                let canvas_height = canvas_width / aspect;
                let size = if canvas_height > available.y - 44.0 {
                    egui::vec2((available.y - 44.0).max(120.0) * aspect, (available.y - 44.0).max(120.0))
                } else {
                    egui::vec2(canvas_width, canvas_height)
                };
                ui.vertical_centered(|ui| {
                    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
                    ui.painter().rect_filled(rect, 9.0, egui::Color32::from_rgb(7, 9, 12));
                    if context.input(|input| !input.raw.hovered_files.is_empty()) {
                        ui.painter().rect_filled(
                            rect,
                            9.0,
                            egui::Color32::from_rgba_unmultiplied(31, 103, 172, 215),
                        );
                        ui.painter().text(
                            rect.center(),
                            egui::Align2::CENTER_CENTER,
                            "Drop media to add it to the playlist",
                            egui::FontId::proportional(17.0),
                            egui::Color32::WHITE,
                        );
                    } else if self.last_main_frame.is_some() {
                        if let Some(texture) = self.preview_texture.as_ref() {
                            ui.painter().image(
                                texture.id(),
                                rect,
                                egui::Rect::from_min_max(
                                    egui::pos2(0.0, 0.0),
                                    egui::pos2(1.0, 1.0),
                                ),
                                egui::Color32::WHITE,
                            );
                        } else {
                            draw_empty_preview(ui, rect, "Preparing preview", "Decoding the first frame…");
                        }
                    } else if self.config.current_index.is_some() && self.decoder.is_some() {
                        draw_empty_preview(ui, rect, "Preparing preview", "Decoding the first frame…");
                    } else {
                        draw_empty_preview(
                            ui,
                            rect,
                            "Build your camera scene",
                            "Add a video or image, then start the virtual camera when output is ready.",
                        );
                    }
                });

                ui.add_space(10.0);
                ui.horizontal_wrapped(|ui| {
                    if let Some(index) = self.config.current_index
                        && let Some(path) = self.config.playlist.get(index)
                    {
                        ui.label(
                            egui::RichText::new(
                                path.file_name()
                                    .and_then(|name| name.to_str())
                                    .unwrap_or("Selected media"),
                            )
                            .strong()
                            .color(if path.exists() { TEXT } else { DANGER }),
                        );
                    } else {
                        ui.label(egui::RichText::new("No media selected").color(MUTED));
                    }
                    if let Some(decoder) = self.decoder.as_ref() {
                        let metadata = decoder.metadata();
                        ui.label(
                            egui::RichText::new(format!(
                                "{} × {} source · {:.2} fps · {} output",
                                metadata.width,
                                metadata.height,
                                metadata.fps,
                                if self.config.output_fps_index == 1 { "60 fps" } else { "30 fps" }
                            ))
                            .small()
                            .color(MUTED),
                        );
                    }
                });
            });
    }

    fn show_notice(&mut self, ui: &mut egui::Ui) {
        let Some(notice) = self.notice.clone() else {
            return;
        };
        let color = match notice.level {
            NoticeLevel::Info => ACCENT,
            NoticeLevel::Success => SUCCESS,
            NoticeLevel::Warning => WARNING,
            NoticeLevel::Error => DANGER,
        };
        let mut dismiss = false;
        egui::Frame::none()
            .fill(color.gamma_multiply(0.12))
            .rounding(8.0)
            .inner_margin(egui::Margin::symmetric(12.0, 9.0))
            .stroke(egui::Stroke::new(1.0, color.gamma_multiply(0.7)))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.label(egui::RichText::new(&notice.title).strong().color(color));
                        ui.label(egui::RichText::new(&notice.message).color(TEXT));
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button("Dismiss").clicked() {
                            dismiss = true;
                        }
                    });
                });
            });
        ui.add_space(10.0);
        if dismiss {
            self.notice = None;
        }
    }
}

impl eframe::App for MimicApp {
    fn update(&mut self, context: &egui::Context, _frame: &mut eframe::Frame) {
        self.handle_setup_messages();
        self.handle_dropped_files(context);
        self.update_runtime(context);
        self.show_top_bar(context);
        self.show_setup_banner(context);
        self.show_left_panel(context);
        self.show_playlist(context);
        self.show_transport(context);
        self.show_workspace(context);
    }
}

fn configure_style(context: &egui::Context) {
    let mut style = (*context.style()).clone();
    style.spacing.item_spacing = egui::vec2(8.0, 8.0);
    style.spacing.button_padding = egui::vec2(10.0, 6.0);
    style.spacing.slider_width = 160.0;
    style.visuals.dark_mode = true;
    style.visuals.panel_fill = PANEL;
    style.visuals.window_fill = PANEL;
    style.visuals.extreme_bg_color = egui::Color32::from_rgb(10, 12, 16);
    style.visuals.faint_bg_color = CARD;
    style.visuals.widgets.noninteractive.bg_fill = CARD;
    style.visuals.widgets.noninteractive.fg_stroke.color = TEXT;
    style.visuals.widgets.inactive.bg_fill = egui::Color32::from_rgb(34, 39, 48);
    style.visuals.widgets.inactive.fg_stroke.color = TEXT;
    style.visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(45, 55, 69);
    style.visuals.widgets.hovered.fg_stroke.color = egui::Color32::WHITE;
    style.visuals.widgets.active.bg_fill = egui::Color32::from_rgb(40, 93, 146);
    style.visuals.widgets.active.fg_stroke.color = egui::Color32::WHITE;
    style.visuals.selection.bg_fill = egui::Color32::from_rgb(37, 91, 146);
    style.visuals.selection.stroke.color = egui::Color32::WHITE;
    style.visuals.override_text_color = Some(TEXT);
    context.set_style(style);
}

fn card(ui: &mut egui::Ui, content: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::none()
        .fill(CARD)
        .rounding(8.0)
        .inner_margin(egui::Margin::same(12.0))
        .stroke(egui::Stroke::new(1.0, BORDER))
        .show(ui, content);
}

fn section_heading(ui: &mut egui::Ui, text: &str) {
    ui.label(egui::RichText::new(text).size(11.0).strong().color(MUTED));
}

fn status_badge(ui: &mut egui::Ui, label: &str, color: egui::Color32) {
    egui::Frame::none()
        .fill(color.gamma_multiply(0.14))
        .rounding(99.0)
        .inner_margin(egui::Margin::symmetric(9.0, 4.0))
        .stroke(egui::Stroke::new(1.0, color.gamma_multiply(0.65)))
        .show(ui, |ui| {
            ui.label(egui::RichText::new(label).size(10.0).strong().color(color));
        });
}

fn draw_empty_preview(ui: &egui::Ui, rect: egui::Rect, title: &str, description: &str) {
    ui.painter().text(
        rect.center() - egui::vec2(0.0, 12.0),
        egui::Align2::CENTER_CENTER,
        title,
        egui::FontId::proportional(18.0),
        TEXT,
    );
    ui.painter().text(
        rect.center() + egui::vec2(0.0, 16.0),
        egui::Align2::CENTER_CENTER,
        description,
        egui::FontId::proportional(13.0),
        MUTED,
    );
}

fn format_time(duration: Duration) -> String {
    let total_seconds = duration.as_secs();
    let hours = total_seconds / 3600;
    let minutes = (total_seconds % 3600) / 60;
    let seconds = total_seconds % 60;
    if hours > 0 {
        format!("{hours:02}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes:02}:{seconds:02}")
    }
}

fn pip_position_label(position: PipPosition) -> &'static str {
    match position {
        PipPosition::TopLeft => "Top left",
        PipPosition::TopRight => "Top right",
        PipPosition::BottomLeft => "Bottom left",
        PipPosition::BottomRight => "Bottom right",
    }
}

fn plural(count: usize) -> &'static str {
    if count == 1 { "" } else { "s" }
}
