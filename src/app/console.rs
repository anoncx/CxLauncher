use super::{background, later, App};
use crate::game::Level;
use crate::LogLine;

const KEEP: usize = 20_000;
const TRIM: usize = 4_000;

#[derive(Default)]
pub struct Console {
    lines: Vec<(Level, String)>,
    shown: Vec<u32>,
    filter: i32,
    first: usize,
    rows: usize,
}

fn passes(filter: i32, level: Level) -> bool {
    match filter {
        1 => matches!(level, Level::Warn | Level::Error),
        2 => level == Level::Error,
        _ => true,
    }
}

impl Console {
    pub fn clear(&mut self) {
        self.lines.clear();
        self.shown.clear();
    }

    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    pub fn push(&mut self, level: Level, text: String) {
        if self.lines.len() >= KEEP {
            self.lines.drain(..TRIM);
            self.reindex();
        }
        if passes(self.filter, level) {
            self.shown.push(self.lines.len() as u32);
        }
        self.lines.push((level, text));
    }

    fn reindex(&mut self) {
        let filter = self.filter;
        self.shown = self.lines.iter().enumerate().filter(|(_, (l, _))| passes(filter, *l)).map(|(i, _)| i as u32).collect();
    }

    fn window(&self) -> Vec<LogLine> {
        self.shown
            .iter()
            .skip(self.first)
            .take(self.rows)
            .map(|&i| {
                let (level, text) = &self.lines[i as usize];
                LogLine { level: level.index(), text: text.as_str().into() }
            })
            .collect()
    }

    pub fn recent(&self, count: usize) -> impl Iterator<Item = &str> {
        self.lines[self.lines.len().saturating_sub(count)..].iter().map(|(_, text)| text.as_str())
    }

    pub fn full(&self) -> String {
        self.lines.iter().map(|(_, text)| text.as_str()).collect::<Vec<_>>().join("\n")
    }

    pub fn text(&self) -> String {
        self.shown.iter().map(|&i| self.lines[i as usize].1.as_str()).collect::<Vec<_>>().join("\n")
    }
}

impl App {
    pub fn push_log(&mut self) {
        let Some(ui) = self.ui() else { return };
        let c = &mut self.console;
        if c.rows == 0 {
            c.rows = 60;
        }
        c.first = c.first.min(c.shown.len().saturating_sub(1));
        ui.set_log_count(c.shown.len() as i32);
        ui.set_log_first(c.first as i32);
        self.models.log.set_vec(c.window());
    }

    pub fn log_changed(&mut self) {
        if self.page() == 3 {
            self.push_log();
        }
    }

    pub fn view_log(&mut self, first: usize, rows: usize) {
        let c = &mut self.console;
        if c.first == first && c.rows == rows {
            return;
        }
        c.first = first;
        c.rows = rows;
        self.push_log();
    }

    pub fn filter_log(&mut self, filter: i32) {
        self.console.filter = filter;
        self.console.reindex();
        self.console.first = self.console.shown.len().saturating_sub(self.console.rows);
        self.push_log();
    }

    pub fn share_open(&mut self) {
        if self.console.is_empty() {
            self.toast("Nothing to share yet");
            return;
        }
        if let Some(ui) = self.ui() {
            ui.set_share_open(true);
        }
    }

    pub fn share_close(&mut self) {
        if self.sharing {
            return;
        }
        if let Some(ui) = self.ui() {
            ui.set_share_open(false);
        }
    }

    pub fn share_send(&mut self) {
        if self.sharing {
            return;
        }
        self.sharing = true;
        if let Some(ui) = self.ui() {
            ui.set_share_busy(true);
        }
        let home = dirs::home_dir().map(|h| h.to_string_lossy().into_owned());
        let text = crate::share::prepare(&self.console.full(), home.as_deref());
        let core = self.core.clone();
        background(move || {
            let result = crate::share::upload(&core.http, &core.ep, &text);
            later(move |app| {
                app.sharing = false;
                if let Some(ui) = app.ui() {
                    ui.set_share_busy(false);
                }
                match result {
                    Ok(url) => {
                        if let Some(ui) = app.ui() {
                            ui.set_share_open(false);
                        }
                        app.copy(url, "Link copied. Anyone with it can read the log");
                    }
                    Err(e) => app.toast(format!("Could not upload the log: {e}")),
                }
            });
        });
    }

    pub fn copy_log(&mut self) {
        let text = self.console.text();
        if text.is_empty() {
            self.toast("Nothing to copy yet");
            return;
        }
        self.copy(text, "Log copied");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filters_and_trims() {
        let mut c = Console { rows: 10, ..Default::default() };
        c.push(Level::Info, "a".into());
        c.push(Level::Warn, "b".into());
        c.push(Level::Error, "c".into());
        assert_eq!(c.shown.len(), 3);
        c.filter = 2;
        c.reindex();
        assert_eq!(c.text(), "c");
        c.filter = 1;
        c.reindex();
        assert_eq!(c.text(), "b\nc");
        for i in 0..KEEP {
            c.push(Level::Warn, i.to_string());
        }
        assert!(c.lines.len() <= KEEP);
        assert_eq!(c.shown.len(), c.lines.iter().filter(|(l, _)| passes(1, *l)).count());
        assert_eq!(c.window().len(), 10);
    }
}
