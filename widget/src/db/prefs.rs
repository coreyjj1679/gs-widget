use std::collections::HashSet;
use std::path::PathBuf;

use super::paths::data_dir;

pub fn load_favourites() -> HashSet<String> {
    std::fs::read_to_string(favourites_file())
        .unwrap_or_default()
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect()
}

pub fn save_favourites(codes: &HashSet<String>) {
    let mut lines: Vec<&str> = codes.iter().map(String::as_str).collect();
    lines.sort_unstable();
    let _ = std::fs::write(favourites_file(), lines.join("\n"));
}

pub(in crate::db) fn favourites_file() -> PathBuf {
    data_dir().join("favourites.txt")
}

pub fn load_lang() -> String {
    let text = std::fs::read_to_string(lang_file()).unwrap_or_default();
    match text.trim() {
        "en-zh" | "en" => "en-zh".into(),
        _ => "zh-en".into(),
    }
}

pub fn save_lang(lang: &str) {
    if matches!(lang, "zh-en" | "en-zh") {
        let _ = std::fs::write(lang_file(), lang);
    }
}

pub(in crate::db) fn set_extra_file() -> PathBuf {
    data_dir().join("set-extra.txt")
}

pub fn load_set_extra() -> bool {
    std::fs::read_to_string(set_extra_file())
        .ok()
        .is_some_and(|text| text.trim() == "1")
}

pub fn save_set_extra(on: bool) {
    let _ = std::fs::write(set_extra_file(), if on { "1" } else { "0" });
}

pub(in crate::db) fn set_extra_name(name: &str) -> bool {
    if name.contains("[이벤트]") || name.contains("[활동]") {
        return true;
    }
    let chars: Vec<char> = name.chars().collect();
    chars.windows(2).any(|pair| pair[0].is_ascii_digit() && pair[1] == '일')
}

pub fn load_opacity() -> f32 {
    let text = std::fs::read_to_string(opacity_file()).unwrap_or_default();
    let percent = text.trim().parse::<f32>().unwrap_or(100.0);
    (percent / 100.0).clamp(0.25, 1.0)
}

pub fn save_opacity(value: f32) {
    let percent = (value.clamp(0.25, 1.0) * 100.0).round() as i32;
    let _ = std::fs::write(opacity_file(), percent.to_string());
}

pub struct DockPref {
    pub vertical: bool,
    pub show: [bool; 9],
    pub order: [i32; 9],
    pub width: f32,
    pub height: f32,
}

pub fn load_dock() -> DockPref {
    let mut pref = DockPref {
        vertical: false,
        show: [true; 9],
        order: [0, 1, 2, 3, 4, 5, 6, 7, 8],
        width: 580.0,
        height: 84.0,
    };
    let text = std::fs::read_to_string(data_dir().join("dock.txt")).unwrap_or_default();
    for line in text.lines() {
        if let Some(value) = line.strip_prefix("v=") {
            pref.vertical = value.trim() == "1";
        } else if let Some(value) = line.strip_prefix("show=") {
            for (index, bit) in value.trim().chars().take(9).enumerate() {
                pref.show[index] = bit != '0';
            }
        } else if let Some(value) = line.strip_prefix("size=") {
            let mut parts = value.split(',');
            let width = parts.next().and_then(|part| part.trim().parse::<f32>().ok());
            let height = parts.next().and_then(|part| part.trim().parse::<f32>().ok());
            if let (Some(width), Some(height)) = (width, height) {
                if width >= 48.0 && height >= 48.0 {
                    pref.width = width;
                    pref.height = height;
                }
            }
        } else if let Some(value) = line.strip_prefix("order=") {
            let nums: Vec<i32> = value.split(',').filter_map(|part| part.trim().parse().ok()).collect();
            if let Some(order) = permutation(&nums) {
                pref.order = order;
            }
        }
    }
    pref
}

pub fn save_dock(pref: &DockPref) {
    let show: String = pref.show.iter().map(|on| if *on { '1' } else { '0' }).collect();
    let order = pref.order.iter().map(|n| n.to_string()).collect::<Vec<_>>().join(",");
    let text = format!(
        "v={}\nshow={show}\norder={order}\nsize={},{}\n",
        if pref.vertical { 1 } else { 0 },
        pref.width,
        pref.height
    );
    let _ = std::fs::write(data_dir().join("dock.txt"), text);
}

pub struct HomePref {
    pub size: i32,
    pub size_chosen: bool,
    pub order: [i32; 9],
}

pub fn load_home() -> HomePref {
    let mut pref = HomePref {
        size: 1,
        size_chosen: false,
        order: [0, 1, 2, 3, 4, 5, 6, 7, 8],
    };
    let text = std::fs::read_to_string(data_dir().join("home.txt")).unwrap_or_default();
    for line in text.lines() {
        if let Some(value) = line.strip_prefix("size=") {
            if let Ok(size) = value.trim().parse::<i32>() {
                if (0..=2).contains(&size) {
                    pref.size = size;
                    pref.size_chosen = true;
                }
            }
        } else if let Some(value) = line.strip_prefix("order=") {
            let nums: Vec<i32> = value.split(',').filter_map(|part| part.trim().parse().ok()).collect();
            if let Some(order) = permutation(&nums) {
                pref.order = order;
            }
        }
    }
    pref
}

pub fn save_home(pref: &HomePref) {
    let order = pref.order.iter().map(|n| n.to_string()).collect::<Vec<_>>().join(",");
    let text = format!("size={}\norder={order}\n", pref.size.clamp(0, 2));
    let _ = std::fs::write(data_dir().join("home.txt"), text);
}

pub struct WindowPref {
    pub width: f32,
    pub height: f32,
    pub x: i32,
    pub y: i32,
}

pub fn load_window() -> Option<WindowPref> {
    let text = std::fs::read_to_string(data_dir().join("window.txt")).ok()?;
    let mut width = None;
    let mut height = None;
    let mut x = None;
    let mut y = None;
    for line in text.lines() {
        if let Some(value) = line.strip_prefix("size=") {
            let mut parts = value.split(',');
            width = parts.next().and_then(|part| part.trim().parse::<f32>().ok());
            height = parts.next().and_then(|part| part.trim().parse::<f32>().ok());
        } else if let Some(value) = line.strip_prefix("pos=") {
            let mut parts = value.split(',');
            x = parts.next().and_then(|part| part.trim().parse::<i32>().ok());
            y = parts.next().and_then(|part| part.trim().parse::<i32>().ok());
        }
    }
    let (Some(width), Some(height), Some(x), Some(y)) = (width, height, x, y) else {
        return None;
    };
    if width < 280.0 || height < 360.0 {
        return None;
    }
    Some(WindowPref { width, height, x, y })
}

pub fn save_window(pref: &WindowPref) {
    if pref.width < 280.0 || pref.height < 360.0 {
        return;
    }
    let text = format!("size={},{}\npos={},{}\n", pref.width, pref.height, pref.x, pref.y);
    let _ = std::fs::write(data_dir().join("window.txt"), text);
}

pub(in crate::db) fn permutation(nums: &[i32]) -> Option<[i32; 9]> {
    if nums.len() != 9 {
        return None;
    }
    let mut seen = [false; 9];
    for n in nums {
        if *n < 0 || *n > 8 || seen[*n as usize] {
            return None;
        }
        seen[*n as usize] = true;
    }
    let mut order = [0; 9];
    order.copy_from_slice(nums);
    Some(order)
}

pub(in crate::db) fn lang_file() -> PathBuf {
    data_dir().join("ui-lang.txt")
}

pub(in crate::db) fn opacity_file() -> PathBuf {
    data_dir().join("opacity.txt")
}
