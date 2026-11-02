use std::cell::RefCell;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use aegis_ipc::Favicon;
use eframe::egui::{self, TextureHandle, TextureId};

const PIXELS: usize = 32 * 32 * 4;
const MAX_TEXTURES: usize = 256;

struct Entry<T> {
    value: Option<T>,
    used: u64,
}

struct IconCache<T> {
    entries: HashMap<String, Entry<T>>,
    clock: u64,
    textures: usize,
}

impl<T> IconCache<T> {
    fn new() -> Self {
        Self {
            entries: HashMap::new(),
            clock: 0,
            textures: 0,
        }
    }

    fn get(&mut self, host: &str) -> Option<Option<&T>> {
        self.clock = self.clock.wrapping_add(1);
        let entry = self.entries.get_mut(host)?;
        entry.used = self.clock;
        Some(entry.value.as_ref())
    }

    fn missing(&mut self, host: &str) {
        self.clock = self.clock.wrapping_add(1);
        self.entries.insert(
            host.to_owned(),
            Entry {
                value: None,
                used: self.clock,
            },
        );
    }

    fn insert(&mut self, host: &str, value: T) {
        self.clock = self.clock.wrapping_add(1);
        if self
            .entries
            .get(host)
            .is_none_or(|entry| entry.value.is_none())
        {
            self.textures += 1;
        }
        self.entries.insert(
            host.to_owned(),
            Entry {
                value: Some(value),
                used: self.clock,
            },
        );
        if self.textures > MAX_TEXTURES {
            let oldest = self
                .entries
                .iter()
                .filter(|(_, entry)| entry.value.is_some())
                .min_by_key(|(_, entry)| entry.used)
                .map(|(host, _)| host.clone());
            if let Some(oldest) = oldest {
                self.entries.remove(&oldest);
                self.textures -= 1;
            }
        }
    }

    fn clear(&mut self) {
        self.entries.clear();
        self.textures = 0;
    }
}

pub struct FaviconStore {
    dir: PathBuf,
    cache: IconCache<TextureHandle>,
}

impl FaviconStore {
    fn new(data_dir: PathBuf) -> Self {
        Self {
            dir: data_dir.join("favicons"),
            cache: IconCache::new(),
        }
    }

    fn texture(&mut self, ctx: &egui::Context, host: &str) -> Option<TextureId> {
        if host.is_empty() || host == "about:blank" {
            return None;
        }
        if let Some(cached) = self.cache.get(host) {
            return cached.map(TextureHandle::id);
        }
        let path = self.dir.join(filename(host));
        if !fs::symlink_metadata(&path)
            .is_ok_and(|metadata| metadata.is_file() && metadata.len() == PIXELS as u64)
        {
            self.cache.missing(host);
            return None;
        }
        let Ok(bytes) = fs::read(path) else {
            self.cache.missing(host);
            return None;
        };
        if bytes.len() != PIXELS {
            self.cache.missing(host);
            return None;
        }
        self.upload(ctx, host, &bytes)
    }

    fn upload(&mut self, ctx: &egui::Context, host: &str, rgba: &[u8]) -> Option<TextureId> {
        if rgba.len() != PIXELS {
            return None;
        }
        let image = egui::ColorImage::from_rgba_unmultiplied([32, 32], rgba);
        let texture = ctx.load_texture(filename(host), image, egui::TextureOptions::LINEAR);
        let id = texture.id();
        self.cache.insert(host, texture);
        Some(id)
    }

    fn save(&mut self, ctx: &egui::Context, host: &str, icon: Favicon) {
        if host.is_empty() || host == "about:blank" {
            return;
        }
        self.upload(ctx, host, icon.rgba.as_slice());
        if fs::create_dir_all(&self.dir).is_err() {
            return;
        }
        let path = self.dir.join(filename(host));
        let temp = path.with_extension("rgba.tmp");
        if fs::write(&temp, icon.rgba.as_slice()).is_ok() {
            let _ = fs::rename(&temp, &path);
        }
    }

    fn clear(&mut self) {
        self.cache.clear();
        if let Ok(files) = fs::read_dir(&self.dir) {
            for file in files.flatten() {
                if file.file_type().is_ok_and(|kind| kind.is_file()) {
                    let _ = fs::remove_file(file.path());
                }
            }
        }
    }
}

fn filename(host: &str) -> String {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in host.bytes() {
        hash = (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}.rgba")
}

thread_local! {
    static STORE: RefCell<Option<FaviconStore>> = const { RefCell::new(None) };
}

pub fn init(data_dir: &Path) {
    STORE.with(|store| *store.borrow_mut() = Some(FaviconStore::new(data_dir.to_path_buf())));
}

pub fn texture(ctx: &egui::Context, host: &str) -> Option<TextureId> {
    STORE.with(|store| {
        store
            .borrow_mut()
            .as_mut()
            .and_then(|store| store.texture(ctx, host))
    })
}

pub fn save(ctx: &egui::Context, host: &str, icon: Favicon) {
    STORE.with(|store| {
        if let Some(store) = store.borrow_mut().as_mut() {
            store.save(ctx, host, icon);
        }
    });
}

pub fn clear() {
    STORE.with(|store| {
        if let Some(store) = store.borrow_mut().as_mut() {
            store.clear();
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evicts_oldest_texture_and_remembers_misses() {
        let mut cache = IconCache::new();
        cache.missing("absent");
        for index in 0..MAX_TEXTURES {
            cache.insert(&index.to_string(), index);
        }
        assert_eq!(cache.get("0").and_then(|value| value.copied()), Some(0));
        cache.insert("new", 999);
        assert!(cache.get("1").is_none());
        assert_eq!(cache.get("absent"), Some(None));
        assert_eq!(cache.textures, MAX_TEXTURES);
    }

    #[test]
    fn filenames_are_hex_only() {
        let name = filename("../../Example.com/../");
        assert_eq!(name.len(), 21);
        assert!(name.ends_with(".rgba"));
        assert!(name[..16].bytes().all(|byte| byte.is_ascii_hexdigit()));
    }
}
