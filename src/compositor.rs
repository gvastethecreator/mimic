use std::path::Path;
use virtualcam::{Camera, PixelFormat};

#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum PipPosition {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

pub struct FrameCompositor {
    camera: Option<Camera>,
    width: u32,
    height: u32,
    fps: f32,
}

impl FrameCompositor {
    pub fn new(width: u32, height: u32, fps: f32) -> Self {
        Self {
            camera: None,
            width,
            height,
            fps,
        }
    }

    /// Tries to initialize the virtual camera backend.
    /// Returns true if successful.
    pub fn init_camera(&mut self) -> Result<(), String> {
        if self.camera.is_some() {
            return Ok(());
        }

        let camera = Camera::builder(self.width, self.height, self.fps as f64)
            .format(PixelFormat::RGB)
            .build()
            .map_err(|e| format!("Failed to create virtual camera device: {:?}", e))?;
            
        self.camera = Some(camera);
        Ok(())
    }

    /// Blends the main media frame with the PIP webcam frame and sends it to the virtual camera.
    /// Also returns the composited frame for GUI preview.
    pub fn process_and_send(
        &mut self,
        main_frame: &[u8],
        pip_frame: Option<(&[u8], u32, u32)>, // (data, width, height)
        position: PipPosition,
        border_radius: u32,
    ) -> Vec<u8> {
        let mut composited = main_frame.to_vec();
        
        // If main_frame is smaller than expected, resize buffer
        let expected_size = (self.width * self.height * 3) as usize;
        if composited.len() < expected_size {
            composited.resize(expected_size, 0);
        }

        // Apply PIP overlay if webcam frame is provided
        if let Some((pip_data, pip_w, pip_h)) = pip_frame {
            if pip_data.len() >= (pip_w * pip_h * 3) as usize {
                composite_frames(
                    &mut composited,
                    self.width,
                    self.height,
                    pip_data,
                    pip_w,
                    pip_h,
                    position,
                    border_radius,
                );
            }
        }

        // Send to virtual camera if active
        if let Some(ref mut cam) = self.camera {
            let _ = cam.send(&composited);
        }

        composited
    }

    pub fn is_active(&self) -> bool {
        self.camera.is_some()
    }
    
    pub fn release(&mut self) {
        self.camera = None;
    }
}

/// Blends pip_buf onto main_buf at the specified corner, applying a rounded corners mask.
pub fn composite_frames(
    main_buf: &mut [u8],
    main_w: u32,
    main_h: u32,
    pip_buf: &[u8],
    pip_w: u32,
    pip_h: u32,
    position: PipPosition,
    border_radius: u32,
) {
    let padding = 20i32;
    let r_i32 = border_radius as i32;
    let pip_w_i32 = pip_w as i32;
    let pip_h_i32 = pip_h as i32;

    let (start_x, start_y) = match position {
        PipPosition::TopLeft => (padding, padding),
        PipPosition::TopRight => (main_w as i32 - pip_w_i32 - padding, padding),
        PipPosition::BottomLeft => (padding, main_h as i32 - pip_h_i32 - padding),
        PipPosition::BottomRight => (
            main_w as i32 - pip_w_i32 - padding,
            main_h as i32 - pip_h_i32 - padding,
        ),
    };

    for y in 0..pip_h_i32 {
        let dest_y = start_y + y;
        if dest_y < 0 || dest_y >= main_h as i32 {
            continue;
        }

        for x in 0..pip_w_i32 {
            let dest_x = start_x + x;
            if dest_x < 0 || dest_x >= main_w as i32 {
                continue;
            }

            // Apply rounded corners masking math
            let mut is_masked = false;

            if r_i32 > 0 {
                // Top-Left corner
                if x < r_i32 && y < r_i32 {
                    let dx = x - r_i32;
                    let dy = y - r_i32;
                    if dx * dx + dy * dy > r_i32 * r_i32 {
                        is_masked = true;
                    }
                }
                // Top-Right corner
                else if x >= pip_w_i32 - r_i32 && y < r_i32 {
                    let dx = x - (pip_w_i32 - r_i32);
                    let dy = y - r_i32;
                    if dx * dx + dy * dy > r_i32 * r_i32 {
                        is_masked = true;
                    }
                }
                // Bottom-Left corner
                else if x < r_i32 && y >= pip_h_i32 - r_i32 {
                    let dx = x - r_i32;
                    let dy = y - (pip_h_i32 - r_i32);
                    if dx * dx + dy * dy > r_i32 * r_i32 {
                        is_masked = true;
                    }
                }
                // Bottom-Right corner
                else if x >= pip_w_i32 - r_i32 && y >= pip_h_i32 - r_i32 {
                    let dx = x - (pip_w_i32 - r_i32);
                    let dy = y - (pip_h_i32 - r_i32);
                    if dx * dx + dy * dy > r_i32 * r_i32 {
                        is_masked = true;
                    }
                }
            }

            if !is_masked {
                let dest_idx = (dest_y as u32 * main_w + dest_x as u32) as usize * 3;
                let src_idx = (y as u32 * pip_w + x as u32) as usize * 3;

                if dest_idx + 2 < main_buf.len() && src_idx + 2 < pip_buf.len() {
                    main_buf[dest_idx] = pip_buf[src_idx];
                    main_buf[dest_idx + 1] = pip_buf[src_idx + 1];
                    main_buf[dest_idx + 2] = pip_buf[src_idx + 2];
                }
            }
        }
    }
}
