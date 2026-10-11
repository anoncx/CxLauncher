use image::{imageops, Rgba, RgbaImage};

fn over(dst: &mut RgbaImage, src: &RgbaImage, x: i64, y: i64) {
    imageops::overlay(dst, src, x, y);
}

fn part(skin: &RgbaImage, x: u32, y: u32, w: u32, h: u32) -> RgbaImage {
    imageops::crop_imm(skin, x, y, w, h).to_image()
}

fn opaque(img: &mut RgbaImage) {
    for p in img.pixels_mut() {
        p.0[3] = 255;
    }
}

pub fn decode(png: &[u8]) -> Option<RgbaImage> {
    let img = image::load_from_memory_with_format(png, image::ImageFormat::Png).ok()?.to_rgba8();
    (img.width() == 64 && (img.height() == 64 || img.height() == 32)).then_some(img)
}

pub fn head(skin: &RgbaImage) -> RgbaImage {
    let mut face = part(skin, 8, 8, 8, 8);
    opaque(&mut face);
    over(&mut face, &part(skin, 40, 8, 8, 8), 0, 0);
    face
}

pub fn body(skin: &RgbaImage, slim: bool) -> RgbaImage {
    let modern = skin.height() == 64;
    let arm = if slim { 3 } else { 4 };
    let mut out = RgbaImage::from_pixel(16, 32, Rgba([0, 0, 0, 0]));
    let mut layer = |base: (u32, u32, u32, u32), overlay: Option<(u32, u32)>, at: (i64, i64), mirror: bool| {
        let (x, y, w, h) = base;
        let mut piece = part(skin, x, y, w, h);
        opaque(&mut piece);
        if let (Some((ox, oy)), true) = (overlay, modern) {
            over(&mut piece, &part(skin, ox, oy, w, h), 0, 0);
        }
        if mirror {
            piece = imageops::flip_horizontal(&piece);
        }
        over(&mut out, &piece, at.0, at.1);
    };
    layer((8, 8, 8, 8), Some((40, 8)), (4, 0), false);
    layer((20, 20, 8, 12), Some((20, 36)), (4, 8), false);
    layer((44, 20, arm, 12), Some((44, 36)), (4 - arm as i64, 8), false);
    layer((4, 20, 4, 12), Some((4, 36)), (4, 20), false);
    if modern {
        layer((36, 52, arm, 12), Some((52, 52)), (12, 8), false);
        layer((20, 52, 4, 12), Some((4, 52)), (8, 20), false);
    } else {
        layer((44, 20, arm, 12), None, (12, 8), true);
        layer((4, 20, 4, 12), None, (8, 20), true);
    }
    out
}

pub fn steve() -> RgbaImage {
    let skin = [0x6a, 0x4a, 0x38];
    let hair = [0x2b, 0x1e, 0x13];
    let shirt = [0x2f, 0x9e, 0xa6];
    let pants = [0x3b, 0x3a, 0x8f];
    let mut img = RgbaImage::from_pixel(64, 64, Rgba([0, 0, 0, 0]));
    let mut fill = |x0: u32, y0: u32, w: u32, h: u32, c: [u8; 3]| {
        for y in y0..y0 + h {
            for x in x0..x0 + w {
                img.put_pixel(x, y, Rgba([c[0], c[1], c[2], 255]));
            }
        }
    };
    fill(8, 8, 8, 8, skin);
    fill(8, 8, 8, 2, hair);
    fill(9, 12, 2, 1, [0xff, 0xff, 0xff]);
    fill(13, 12, 2, 1, [0xff, 0xff, 0xff]);
    fill(10, 12, 1, 1, [0x4a, 0x3a, 0x8f]);
    fill(13, 12, 1, 1, [0x4a, 0x3a, 0x8f]);
    fill(20, 20, 8, 12, shirt);
    fill(44, 20, 4, 4, shirt);
    fill(44, 24, 4, 8, skin);
    fill(36, 52, 4, 4, shirt);
    fill(36, 56, 4, 8, skin);
    fill(4, 20, 4, 12, pants);
    fill(20, 52, 4, 12, pants);
    img
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn composes_front_view() {
        let skin = steve();
        let b = body(&skin, false);
        assert_eq!((b.width(), b.height()), (16, 32));
        assert_eq!(b.get_pixel(5, 9).0[3], 255);
        assert_eq!(b.get_pixel(0, 0).0[3], 0);
        let slim = body(&skin, true);
        assert_eq!(slim.get_pixel(0, 9).0[3], 0);
        assert_eq!(head(&skin).dimensions(), (8, 8));
    }
}
