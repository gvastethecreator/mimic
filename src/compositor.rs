use virtualcam::{Camera, PixelFormat};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum PipPosition {
    TopLeft,
    TopRight,
    BottomLeft,
    #[default]
    BottomRight,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CameraDetails {
    pub backend: String,
    pub device: String,
}

pub struct PipOverlay<'a> {
    pub buffer: &'a [u8],
    pub width: u32,
    pub height: u32,
    pub position: PipPosition,
    pub border_radius: u32,
}

pub struct FrameCompositor {
    camera: Option<Camera>,
    camera_details: Option<CameraDetails>,
    width: u32,
    height: u32,
    fps: f32,
}

impl FrameCompositor {
    pub fn new(width: u32, height: u32, fps: f32) -> Self {
        Self {
            camera: None,
            camera_details: None,
            width,
            height,
            fps,
        }
    }

    pub fn init_camera(&mut self) -> Result<CameraDetails, String> {
        if let Some(details) = &self.camera_details {
            return Ok(details.clone());
        }

        let camera = Camera::builder(self.width, self.height, self.fps as f64)
            .format(PixelFormat::RGB)
            .build()
            .map_err(|error| format!("Could not open a virtual camera: {error}"))?;
        let details = CameraDetails {
            backend: camera.backend().to_string(),
            device: camera.device().to_string(),
        };
        self.camera = Some(camera);
        self.camera_details = Some(details.clone());
        Ok(details)
    }

    pub fn process_and_send(
        &mut self,
        main_frame: &[u8],
        pip_frame: Option<(&[u8], u32, u32)>,
        position: PipPosition,
        border_radius: u32,
    ) -> Result<Vec<u8>, String> {
        let mut composited = normalize_rgb_frame(main_frame, self.width, self.height);

        if let Some((pip_data, pip_width, pip_height)) = pip_frame {
            composite_frames(
                &mut composited,
                self.width,
                self.height,
                PipOverlay {
                    buffer: pip_data,
                    width: pip_width,
                    height: pip_height,
                    position,
                    border_radius,
                },
            );
        }

        let send_result = self.camera.as_mut().map(|camera| camera.send(&composited));
        if let Some(Err(error)) = send_result {
            self.release();
            return Err(format!("Virtual camera stopped accepting frames: {error}"));
        }

        Ok(composited)
    }

    pub fn is_active(&self) -> bool {
        self.camera.is_some()
    }

    pub fn camera_details(&self) -> Option<&CameraDetails> {
        self.camera_details.as_ref()
    }

    pub fn release(&mut self) {
        if let Some(mut camera) = self.camera.take() {
            let _ = camera.close();
        }
        self.camera_details = None;
    }
}

impl Drop for FrameCompositor {
    fn drop(&mut self) {
        self.release();
    }
}

fn normalize_rgb_frame(frame: &[u8], width: u32, height: u32) -> Vec<u8> {
    let expected_size = width as usize * height as usize * 3;
    let mut normalized = vec![0_u8; expected_size];
    let copy_size = frame.len().min(expected_size);
    normalized[..copy_size].copy_from_slice(&frame[..copy_size]);
    normalized
}

pub fn composite_frames(
    main_buffer: &mut [u8],
    main_width: u32,
    main_height: u32,
    overlay: PipOverlay<'_>,
) {
    let PipOverlay {
        buffer: pip_buffer,
        width: pip_width,
        height: pip_height,
        position,
        border_radius,
    } = overlay;
    let main_size = main_width as usize * main_height as usize * 3;
    let pip_size = pip_width as usize * pip_height as usize * 3;
    if main_width == 0
        || main_height == 0
        || pip_width == 0
        || pip_height == 0
        || main_buffer.len() < main_size
        || pip_buffer.len() < pip_size
    {
        return;
    }

    let padding = 20_i64;
    let pip_width = i64::from(pip_width);
    let pip_height = i64::from(pip_height);
    let main_width_i64 = i64::from(main_width);
    let main_height_i64 = i64::from(main_height);
    let radius = i64::from(border_radius)
        .min(pip_width / 2)
        .min(pip_height / 2);

    let (start_x, start_y) = match position {
        PipPosition::TopLeft => (padding, padding),
        PipPosition::TopRight => (main_width_i64 - pip_width - padding, padding),
        PipPosition::BottomLeft => (padding, main_height_i64 - pip_height - padding),
        PipPosition::BottomRight => (
            main_width_i64 - pip_width - padding,
            main_height_i64 - pip_height - padding,
        ),
    };

    for y in 0..pip_height {
        let destination_y = start_y + y;
        if !(0..main_height_i64).contains(&destination_y) {
            continue;
        }

        for x in 0..pip_width {
            let destination_x = start_x + x;
            if !(0..main_width_i64).contains(&destination_x)
                || is_outside_rounded_corner(x, y, pip_width, pip_height, radius)
            {
                continue;
            }

            let destination_index =
                (destination_y as usize * main_width as usize + destination_x as usize) * 3;
            let source_index = (y as usize * pip_width as usize + x as usize) * 3;
            main_buffer[destination_index..destination_index + 3]
                .copy_from_slice(&pip_buffer[source_index..source_index + 3]);
        }
    }
}

fn is_outside_rounded_corner(x: i64, y: i64, width: i64, height: i64, radius: i64) -> bool {
    if radius == 0 {
        return false;
    }

    let center = if x < radius && y < radius {
        Some((radius, radius))
    } else if x >= width - radius && y < radius {
        Some((width - radius - 1, radius))
    } else if x < radius && y >= height - radius {
        Some((radius, height - radius - 1))
    } else if x >= width - radius && y >= height - radius {
        Some((width - radius - 1, height - radius - 1))
    } else {
        None
    };

    center.is_some_and(|(center_x, center_y)| {
        let delta_x = x - center_x;
        let delta_y = y - center_y;
        delta_x * delta_x + delta_y * delta_y >= radius * radius
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn process_normalizes_short_and_long_main_frames() {
        let mut compositor = FrameCompositor::new(2, 1, 30.0);

        let short = compositor
            .process_and_send(&[1, 2, 3], None, PipPosition::TopLeft, 0)
            .unwrap();
        let long = compositor
            .process_and_send(&[9; 12], None, PipPosition::TopLeft, 0)
            .unwrap();

        assert_eq!(short, vec![1, 2, 3, 0, 0, 0]);
        assert_eq!(long, vec![9; 6]);
    }

    #[test]
    fn pip_is_composited_at_requested_corner() {
        let mut main = vec![0_u8; 64 * 64 * 3];
        let pip = vec![255_u8; 8 * 8 * 3];

        composite_frames(
            &mut main,
            64,
            64,
            PipOverlay {
                buffer: &pip,
                width: 8,
                height: 8,
                position: PipPosition::BottomRight,
                border_radius: 0,
            },
        );

        let inside = ((64 - 20 - 8) * 64 + (64 - 20 - 8)) as usize * 3;
        assert_eq!(&main[inside..inside + 3], &[255, 255, 255]);
        assert_eq!(&main[..3], &[0, 0, 0]);
    }

    #[test]
    fn rounded_mask_clamps_radius_without_overflow() {
        let mut main = vec![0_u8; 64 * 64 * 3];
        let pip = vec![255_u8; 8 * 8 * 3];

        composite_frames(
            &mut main,
            64,
            64,
            PipOverlay {
                buffer: &pip,
                width: 8,
                height: 8,
                position: PipPosition::TopLeft,
                border_radius: u32::MAX,
            },
        );

        let corner = (20 * 64 + 20) * 3;
        let center = (24 * 64 + 24) * 3;
        assert_eq!(&main[corner..corner + 3], &[0, 0, 0]);
        assert_eq!(&main[center..center + 3], &[255, 255, 255]);
    }

    #[test]
    fn invalid_pip_buffer_is_ignored() {
        let mut main = vec![7_u8; 2 * 2 * 3];
        composite_frames(
            &mut main,
            2,
            2,
            PipOverlay {
                buffer: &[1, 2],
                width: 2,
                height: 2,
                position: PipPosition::TopLeft,
                border_radius: 0,
            },
        );
        assert_eq!(main, vec![7_u8; 12]);
    }
}
