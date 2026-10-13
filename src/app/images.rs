use image::{imageops::FilterType, RgbaImage};
use slint::{Image, Rgba8Pixel, SharedPixelBuffer};

pub type Pixels = SharedPixelBuffer<Rgba8Pixel>;

pub fn pixels(img: &RgbaImage) -> Pixels {
    SharedPixelBuffer::clone_from_slice(img.as_raw(), img.width(), img.height())
}

pub fn thumb(bytes: &[u8], size: u32) -> Option<Pixels> {
    let img = image::load_from_memory(bytes).ok()?;
    let img = if img.width() > size || img.height() > size { img.resize_to_fill(size, size, FilterType::Triangle) } else { img };
    Some(pixels(&img.to_rgba8()))
}

pub fn fit(bytes: &[u8], edge: u32) -> Option<Pixels> {
    let img = image::load_from_memory(bytes).ok()?;
    let img = if img.width() > edge || img.height() > edge { img.resize(edge, edge, FilterType::Triangle) } else { img };
    Some(pixels(&img.to_rgba8()))
}

pub fn dot_tile(step: u32) -> Image {
    let mut buf = Pixels::new(step, step);
    let px = buf.make_mut_slice();
    let c = (step as f32 - 1.0) / 2.0;
    for y in 0..step {
        for x in 0..step {
            let d = ((x as f32 - c).powi(2) + (y as f32 - c).powi(2)).sqrt();
            let a = (1.6 - d).clamp(0.0, 1.0);
            px[(y * step + x) as usize] = Rgba8Pixel { r: 255, g: 255, b: 255, a: (a * 255.0) as u8 };
        }
    }
    Image::from_rgba8(buf)
}
