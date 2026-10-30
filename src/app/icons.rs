use super::images::pixels;
use super::{background, later, App};
use crate::paths::write_atomic;
use image::{imageops::FilterType, ImageFormat};
use slint::Image;
use std::collections::HashMap;
use std::io::Cursor;

const EDGE: u32 = 128;
const SOURCE_LIMIT: u64 = 16 << 20;
const DOWNLOAD_LIMIT: u64 = 4 << 20;

#[derive(Default)]
pub enum Choice {
    #[default]
    Keep,
    Clear,
    Set(Vec<u8>),
}

#[derive(Default)]
pub struct Icons {
    cache: HashMap<String, Option<Image>>,
    pub choice: Choice,
}

pub fn render(bytes: &[u8]) -> Option<Vec<u8>> {
    let icon = image::load_from_memory(bytes).ok()?.resize_to_fill(EDGE, EDGE, FilterType::CatmullRom);
    let mut out = Cursor::new(Vec::new());
    icon.write_to(&mut out, ImageFormat::Png).ok()?;
    Some(out.into_inner())
}

fn picture(png: &[u8]) -> Option<Image> {
    let icon = image::load_from_memory(png).ok()?.to_rgba8();
    Some(Image::from_rgba8(pixels(&icon)))
}

impl Icons {
    pub fn image(&mut self, paths: &crate::paths::Paths, id: &str) -> Option<Image> {
        self.cache.entry(id.to_string()).or_insert_with(|| std::fs::read(paths.icon(id)).ok().and_then(|bytes| picture(&bytes))).clone()
    }

    pub fn forget(&mut self, id: &str) {
        self.cache.remove(id);
    }
}

impl App {
    fn show_icon(&self, picture: Option<Image>) {
        let Some(ui) = self.ui() else { return };
        ui.set_edit_has_icon(picture.is_some());
        ui.set_edit_icon(picture.unwrap_or_default());
    }

    pub fn begin_icon_edit(&mut self, id: &str) {
        self.icons.choice = Choice::Keep;
        let current = if id.is_empty() { None } else { self.icons.image(&self.core.paths, id) };
        self.show_icon(current);
    }

    pub fn pick_icon(&mut self) {
        let filter = ["png", "jpg", "jpeg", "webp", "gif"];
        let Some(path) = rfd::FileDialog::new().set_title("Choose an icon").add_filter("Image", &filter).pick_file() else { return };
        let bytes = std::fs::metadata(&path).ok().filter(|m| m.len() <= SOURCE_LIMIT).and_then(|_| std::fs::read(&path).ok());
        match bytes.as_deref().and_then(render) {
            Some(png) => {
                self.show_icon(picture(&png));
                self.icons.choice = Choice::Set(png);
            }
            None => self.toast("That image could not be read"),
        }
    }

    pub fn clear_icon(&mut self) {
        self.icons.choice = Choice::Clear;
        self.show_icon(None);
    }

    pub fn commit_icon(&mut self, id: &str) {
        match std::mem::take(&mut self.icons.choice) {
            Choice::Keep => {}
            Choice::Clear => self.remove_icon(id),
            Choice::Set(png) => {
                let path = self.core.paths.icon(id);
                if let Some(dir) = path.parent() {
                    let _ = std::fs::create_dir_all(dir);
                }
                let _ = write_atomic(&path, &png);
                self.icons.forget(id);
            }
        }
    }

    pub fn remove_icon(&mut self, id: &str) {
        let _ = std::fs::remove_file(self.core.paths.icon(id));
        self.icons.forget(id);
    }

    pub fn copy_icon(&mut self, from: &str, to: &str) {
        let target = self.core.paths.icon(to);
        if let Some(dir) = target.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::copy(self.core.paths.icon(from), target);
        self.icons.forget(to);
    }

    pub fn fetch_pack_icon(&mut self, id: String, url: String) {
        let core = self.core.clone();
        background(move || {
            let saved = core.http.bytes(&url, DOWNLOAD_LIMIT).ok().and_then(|bytes| render(&bytes)).is_some_and(|png| {
                let path = core.paths.icon(&id);
                path.parent().is_some_and(|dir| std::fs::create_dir_all(dir).is_ok()) && write_atomic(&path, &png).is_ok()
            });
            if saved {
                later(move |app| {
                    app.icons.forget(&id);
                    app.push_profiles();
                });
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_square_png_icons_from_any_shape() {
        let wide = image::RgbaImage::from_pixel(300, 100, image::Rgba([200, 40, 40, 255]));
        let mut source = Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(wide).write_to(&mut source, ImageFormat::Png).unwrap();
        let png = render(&source.into_inner()).unwrap();
        let back = image::load_from_memory(&png).unwrap();
        assert_eq!((back.width(), back.height()), (EDGE, EDGE));
        assert!(render(b"not an image").is_none());
    }
}
