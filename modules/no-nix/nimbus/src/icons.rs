//! Adwaita symbolic icon registry. Every icon the bar can show is resolved
//! and parsed once at startup; the scene refers to them by small integer id.

use std::{collections::HashMap, path::PathBuf};

pub struct IconSet {
    pub paths: Vec<Option<PathBuf>>,
    ids: HashMap<String, u16>,
}

fn names() -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    for l in (0..=100).step_by(10) {
        v.push(format!("battery-level-{l}-symbolic"));
        v.push(format!("battery-level-{l}-charging-symbolic"));
    }
    v.push("battery-level-100-charged-symbolic".into());
    for n in [
        "battery-missing",
        "audio-volume-muted",
        "audio-volume-low",
        "audio-volume-medium",
        "audio-volume-high",
        "network-wireless-signal-none",
        "network-wireless-signal-weak",
        "network-wireless-signal-ok",
        "network-wireless-signal-good",
        "network-wireless-signal-excellent",
        "network-wired",
        "network-offline",
        // settings panel
        "display-brightness",
        "network-wireless",
        "network-wireless-disabled",
        "bluetooth-active",
        "bluetooth-disabled",
        "system-lock-screen",
        "weather-clear-night",
        "system-reboot",
        "system-shutdown",
        "go-previous",
        "network-wireless-encrypted",
        "object-select",
        "bluetooth",
        "audio-headphones",
        "audio-headset",
        "audio-speakers",
        "input-mouse",
        "input-keyboard",
        "input-gaming",
        "phone",
        "computer",
        // settings window
        "emblem-system",
        "preferences-desktop-wallpaper",
        "window-close",
        "preferences-desktop-appearance",
        "video-display",
        "battery-full",
        "help-about",
        "avatar-default",
        "document-edit",
        "audio-input-microphone",
        // now playing
        "media-playback-start",
        "media-playback-pause",
        "media-skip-forward",
        "media-skip-backward",
        "audio-x-generic",
        // popout extras
        "night-light",
        "airplane-mode",
        "camera-photo",
        "media-record",
        "x-office-calendar",
        "go-next",
        "weather-clear",
        "weather-clear-night",
        "weather-few-clouds",
        "weather-few-clouds-night",
        "weather-overcast",
        "weather-fog",
        "weather-showers",
        "weather-showers-scattered",
        "weather-snow",
        "weather-storm",
        // file browser and file search
        "user-home",
        "folder",
        "folder-documents",
        "folder-download",
        "folder-pictures",
        "folder-music",
        "folder-videos",
        "text-x-generic",
        "image-x-generic",
        "video-x-generic",
        "package-x-generic",
        "font-x-generic",
        "application-x-executable",
        "x-office-document",
        "x-office-spreadsheet",
        "x-office-presentation",
    ] {
        v.push(format!("{n}-symbolic"));
    }
    v
}

impl IconSet {
    pub fn discover() -> Self {
        let data_dirs = std::env::var("XDG_DATA_DIRS")
            .unwrap_or_else(|_| "/run/current-system/sw/share:/usr/share".into());
        let mut index: HashMap<String, PathBuf> = HashMap::new();
        for dir in data_dirs.split(':').filter(|d| !d.is_empty()) {
            let root = PathBuf::from(dir).join("icons/Adwaita/symbolic");
            let Ok(cats) = std::fs::read_dir(&root) else { continue };
            for cat in cats.flatten() {
                let Ok(files) = std::fs::read_dir(cat.path()) else { continue };
                for f in files.flatten() {
                    let name = f.file_name().to_string_lossy().trim_end_matches(".svg").to_string();
                    index.entry(name).or_insert_with(|| f.path());
                }
            }
        }
        let names = names();
        let paths = names.iter().map(|n| index.get(n).cloned()).collect();
        let ids = names.into_iter().enumerate().map(|(i, n)| (n, i as u16)).collect();
        Self { paths, ids }
    }

    /// First name in `candidates` that resolved on disk.
    pub fn id(&self, candidates: &[&str]) -> u16 {
        for c in candidates {
            if let Some(&i) = self.ids.get(*c)
                && self.paths[i as usize].is_some()
            {
                return i;
            }
        }
        self.ids[candidates[candidates.len() - 1]]
    }

    pub fn battery(&self, percent: u8, charging: bool) -> u16 {
        let l = ((percent as u32 + 5) / 10 * 10).min(100);
        if charging {
            if l == 100 {
                return self.id(&["battery-level-100-charged-symbolic", "battery-level-100-symbolic"]);
            }
            return self.id(&[&format!("battery-level-{l}-charging-symbolic")]);
        }
        self.id(&[&format!("battery-level-{l}-symbolic")])
    }
}
