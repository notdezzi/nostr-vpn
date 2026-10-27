// QR rendering (qrcode crate -> Slint image) and decoding (rqrr), matching
// the WPF QrCodeView rendering (4-module quiet zone, white background).

use slint::SharedPixelBuffer;

pub fn render_image(text: &str) -> slint::Image {
    let code = [qrcode::EcLevel::M, qrcode::EcLevel::L]
        .into_iter()
        .find_map(|level| {
            qrcode::QrCode::with_error_correction_level(text.as_bytes(), level).ok()
        });
    let Some(code) = code else {
        return placeholder_image();
    };
    let modules = code.width();
    if modules == 0 {
        return placeholder_image();
    }
    let dark: Vec<bool> = code
        .to_colors()
        .into_iter()
        .map(|color| matches!(color, qrcode::Color::Dark))
        .collect();

    let quiet = 4usize;
    let scale = 4usize; // rendered modules per QR module; UI scales the rest
    let side = ((modules + quiet * 2) * scale) as u32;
    let mut canvas = image::RgbaImage::from_pixel(side, side, image::Rgba([255, 255, 255, 255]));
    for y in 0..modules {
        for x in 0..modules {
            if !dark[y * modules + x] {
                continue;
            }
            for dy in 0..scale {
                for dx in 0..scale {
                    canvas.put_pixel(
                        ((quiet + x) * scale + dx) as u32,
                        ((quiet + y) * scale + dy) as u32,
                        image::Rgba([0, 0, 0, 255]),
                    );
                }
            }
        }
    }
    image_to_slint(&canvas)
}

fn placeholder_image() -> slint::Image {
    let side = 32u32;
    let mut canvas = image::RgbaImage::from_pixel(side, side, image::Rgba([248, 248, 248, 255]));
    for y in 0..side {
        for x in 0..side {
            if x == 0 || y == 0 || x == side - 1 || y == side - 1 {
                canvas.put_pixel(x, y, image::Rgba([190, 199, 209, 255]));
            }
        }
    }
    image_to_slint(&canvas)
}

fn image_to_slint(canvas: &image::RgbaImage) -> slint::Image {
    let buffer = SharedPixelBuffer::<slint::Rgba8Pixel>::clone_from_slice(
        canvas.as_raw(),
        canvas.width(),
        canvas.height(),
    );
    slint::Image::from_rgba8(buffer)
}

pub fn decode_file(path: &str) -> Result<String, String> {
    let image = image::open(path).map_err(|error| format!("Could not read image: {error}"))?;
    let luma = image.to_luma8();
    let mut prepared = rqrr::PreparedImage::prepare(luma);
    for grid in prepared.detect_grids() {
        if let Ok((_, value)) = grid.decode() {
            return Ok(value);
        }
    }
    Err("No QR code found in image".to_string())
}
