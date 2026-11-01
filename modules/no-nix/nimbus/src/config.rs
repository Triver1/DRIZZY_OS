//! Tiny persisted preferences: `~/.config/nimbus/config`, `key = value`
//! lines. Environment variables (NIMBUS_DESIGN, ...) still win.

use std::{collections::BTreeMap, path::PathBuf};

fn path() -> PathBuf {
    // Tests never see the real config.
    #[cfg(test)]
    return std::env::temp_dir().join(format!("nimbus-test-config-{}", std::process::id()));
    #[cfg(not(test))]
    path_real()
}

#[cfg(not(test))]
fn path_real() -> PathBuf {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".config"));
    base.join("nimbus").join("config")
}

fn read_all() -> BTreeMap<String, String> {
    std::fs::read_to_string(path())
        .unwrap_or_default()
        .lines()
        .filter_map(|l| {
            let (k, v) = l.split_once('=')?;
            Some((k.trim().to_string(), v.trim().to_string()))
        })
        .filter(|(k, _)| !k.starts_with('#'))
        .collect()
}

pub fn get(key: &str) -> Option<String> {
    read_all().remove(key)
}

pub fn set(key: &str, value: &str) {
    let mut all = read_all();
    all.insert(key.to_string(), value.to_string());
    let p = path();
    if let Some(dir) = p.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let body: String = all.iter().map(|(k, v)| format!("{k} = {v}\n")).collect();
    let _ = std::fs::write(p, body);
}
