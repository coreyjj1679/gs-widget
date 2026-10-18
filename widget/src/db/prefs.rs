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

#[derive(Clone, Debug, PartialEq)]
pub struct ViewPref {
    pub pin: i32,
    pub battle: bool,
    pub gallery: bool,
    pub animate: bool,
    pub hotkeys: bool,
    pub legacy: bool,
}

impl ViewPref {
    pub fn standard() -> Self {
        Self { pin: 0, battle: true, gallery: false, animate: true, hotkeys: true, legacy: false }
    }
}

pub(in crate::db) fn view_file() -> PathBuf {
    data_dir().join("view.txt")
}

pub(crate) fn parse_view(text: &str) -> ViewPref {
    let mut pref = ViewPref::standard();
    for line in text.lines() {
        if let Some(value) = line.strip_prefix("pin=") {
            pref.pin = match value.trim() {
                "on" => 1,
                "off" => 2,
                _ => 0,
            };
        } else if let Some(value) = line.strip_prefix("battle=") {
            pref.battle = value.trim() != "0";
        } else if let Some(value) = line.strip_prefix("gallery=") {
            pref.gallery = value.trim() == "1";
        } else if let Some(value) = line.strip_prefix("animate=") {
            pref.animate = value.trim() != "0";
        } else if let Some(value) = line.strip_prefix("hotkeys=") {
            pref.hotkeys = value.trim() != "0";
        } else if let Some(value) = line.strip_prefix("legacy=") {
            pref.legacy = value.trim() == "1";
        }
    }
    pref
}

pub(crate) fn format_view(pref: &ViewPref) -> String {
    let pin = match pref.pin {
        1 => "on",
        2 => "off",
        _ => "auto",
    };
    format!(
        "pin={pin}\nbattle={}\ngallery={}\nanimate={}\nhotkeys={}\nlegacy={}\n",
        if pref.battle { "1" } else { "0" },
        if pref.gallery { "1" } else { "0" },
        if pref.animate { "1" } else { "0" },
        if pref.hotkeys { "1" } else { "0" },
        if pref.legacy { "1" } else { "0" }
    )
}

pub fn load_view() -> ViewPref {
    std::fs::read_to_string(view_file())
        .map(|text| parse_view(&text))
        .unwrap_or_else(|_| ViewPref::standard())
}

pub fn save_view(pref: &ViewPref) {
    let _ = std::fs::write(view_file(), format_view(pref));
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

#[derive(Clone, Debug, PartialEq)]
pub struct WindowPref {
    pub width: f32,
    pub height: f32,
    pub x: i32,
    pub y: i32,
    pub full: bool,
    pub full_x: i32,
    pub full_y: i32,
    pub has_full_pos: bool,
    pub full_width: f32,
    pub full_height: f32,
    pub has_full_size: bool,
}

impl WindowPref {
    pub fn widget(width: f32, height: f32, x: i32, y: i32) -> Self {
        Self {
            width,
            height,
            x,
            y,
            full: false,
            full_x: 0,
            full_y: 0,
            has_full_pos: false,
            full_width: 0.0,
            full_height: 0.0,
            has_full_size: false,
        }
    }
}

pub(crate) fn parse_window(text: &str) -> Option<WindowPref> {
    let mut width = None;
    let mut height = None;
    let mut x = None;
    let mut y = None;
    let mut full = false;
    let mut full_x = None;
    let mut full_y = None;
    let mut full_width = None;
    let mut full_height = None;
    for line in text.lines() {
        if let Some(value) = line.strip_prefix("size=") {
            let mut parts = value.split(',');
            width = parts.next().and_then(|part| part.trim().parse::<f32>().ok());
            height = parts.next().and_then(|part| part.trim().parse::<f32>().ok());
        } else if let Some(value) = line.strip_prefix("full-size=") {
            let mut parts = value.split(',');
            full_width = parts.next().and_then(|part| part.trim().parse::<f32>().ok());
            full_height = parts.next().and_then(|part| part.trim().parse::<f32>().ok());
        } else if let Some(value) = line.strip_prefix("full-pos=") {
            let mut parts = value.split(',');
            full_x = parts.next().and_then(|part| part.trim().parse::<i32>().ok());
            full_y = parts.next().and_then(|part| part.trim().parse::<i32>().ok());
        } else if let Some(value) = line.strip_prefix("pos=") {
            let mut parts = value.split(',');
            x = parts.next().and_then(|part| part.trim().parse::<i32>().ok());
            y = parts.next().and_then(|part| part.trim().parse::<i32>().ok());
        } else if let Some(value) = line.strip_prefix("mode=") {
            full = value.trim() == "full";
        }
    }
    let (Some(width), Some(height), Some(x), Some(y)) = (width, height, x, y) else {
        return None;
    };
    if width < 280.0 || height < 360.0 {
        return None;
    }
    let has_full_pos = full_x.is_some() && full_y.is_some();
    Some(WindowPref {
        width,
        height,
        x,
        y,
        full,
        full_x: full_x.unwrap_or(0),
        full_y: full_y.unwrap_or(0),
        has_full_pos,
        full_width: full_width.unwrap_or(0.0),
        full_height: full_height.unwrap_or(0.0),
        has_full_size: full_width.is_some() && full_height.is_some(),
    })
}

pub(crate) fn format_window(pref: &WindowPref) -> String {
    let mut text = format!(
        "size={},{}\npos={},{}\nmode={}\n",
        pref.width,
        pref.height,
        pref.x,
        pref.y,
        if pref.full { "full" } else { "widget" }
    );
    if pref.has_full_pos {
        text.push_str(&format!("full-pos={},{}\n", pref.full_x, pref.full_y));
    }
    if pref.has_full_size {
        text.push_str(&format!("full-size={},{}\n", pref.full_width, pref.full_height));
    }
    text
}

#[cfg(not(target_os = "macos"))]
pub fn load_window() -> Option<WindowPref> {
    let text = std::fs::read_to_string(data_dir().join("window.txt")).ok()?;
    parse_window(&text)
}

#[cfg(target_os = "macos")]
pub fn load_window() -> Option<WindowPref> {
    let text = std::fs::read_to_string(data_dir().join("window-macos.txt")).ok()?;
    parse_macos_window(&text, crate::window::position_scale())
}

#[cfg(any(target_os = "macos", test))]
fn parse_macos_window(text: &str, current_scale: f64) -> Option<WindowPref> {
    let mut pref = parse_window(text)?;
    let saved_scale = text
        .lines()
        .find_map(|line| line.strip_prefix("scale="))
        .and_then(|value| value.trim().parse::<f64>().ok());
    if let Some(saved_scale) = saved_scale.filter(|scale| scale.is_finite() && *scale > 0.0) {
        if current_scale.is_finite() && current_scale > 0.0 {
            // Winit's macOS positions use the current window's backing scale,
            // including coordinates on other screens. Sizes are already logical.
            let rescale = |position: i32| {
                (position as f64 / saved_scale * current_scale).round() as i32
            };
            pref.x = rescale(pref.x);
            pref.y = rescale(pref.y);
            pref.full_x = rescale(pref.full_x);
            pref.full_y = rescale(pref.full_y);
        }
    }
    Some(pref)
}

#[cfg(not(target_os = "macos"))]
pub fn save_window(pref: &WindowPref) {
    if pref.width < 280.0 || pref.height < 360.0 {
        return;
    }
    let _ = std::fs::write(data_dir().join("window.txt"), format_window(pref));
}

#[cfg(target_os = "macos")]
pub fn save_window(pref: &WindowPref) {
    if pref.width < 280.0 || pref.height < 360.0 {
        return;
    }
    let mut text = format_window(pref);
    text.push_str(&format!("scale={}\n", crate::window::position_scale()));
    let _ = std::fs::write(data_dir().join("window-macos.txt"), text);
}

pub fn save_mode(full: bool, full_pos: Option<(i32, i32)>) {
    let mut pref = load_window().unwrap_or(WindowPref::widget(560.0, 740.0, 0, 0));
    pref.full = full;
    if let Some((x, y)) = full_pos {
        pref.full_x = x;
        pref.full_y = y;
        pref.has_full_pos = true;
    }
    save_window(&pref);
}

pub fn save_full_geometry(width: f32, height: f32, x: i32, y: i32) {
    if width < 280.0 || height < 360.0 {
        return;
    }
    let mut pref = load_window().unwrap_or(WindowPref::widget(560.0, 740.0, 0, 0));
    pref.full = true;
    pref.full_width = width;
    pref.full_height = height;
    pref.has_full_size = true;
    pref.full_x = x;
    pref.full_y = y;
    pref.has_full_pos = true;
    save_window(&pref);
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

#[cfg(test)]
mod tests {
    use super::{format_view, format_window, parse_macos_window, parse_view, parse_window, ViewPref};

    #[test]
    fn macos_positions_follow_the_current_display_scale() {
        let text = "size=560,740\npos=-1920,100\nmode=full\nfull-pos=1600,-900\nfull-size=1280,800\nscale=1\n";
        let retina = parse_macos_window(text, 2.0).expect("Retina window");
        assert_eq!((retina.x, retina.y), (-3840, 200));
        assert_eq!((retina.full_x, retina.full_y), (3200, -1800));
        assert_eq!((retina.width, retina.height), (560.0, 740.0));
        assert_eq!((retina.full_width, retina.full_height), (1280.0, 800.0));

        let saved = format!("{}scale=2\n", format_window(&retina));
        let external = parse_macos_window(&saved, 1.0).expect("external display window");
        assert_eq!(external, parse_window(text).expect("original window"));
    }

    #[test]
    fn macos_invalid_or_missing_scale_keeps_existing_coordinates() {
        let text = "size=560,740\npos=-1920,100\nfull-pos=1600,-900\n";
        let original = parse_window(text).expect("window");
        for scale in ["", "scale=0\n", "scale=-2\n", "scale=NaN\n", "scale=inf\n", "scale=invalid\n"] {
            assert_eq!(parse_macos_window(&format!("{text}{scale}"), 2.0), Some(original.clone()));
        }
        for current_scale in [0.0, -2.0, f64::NAN, f64::INFINITY] {
            assert_eq!(parse_macos_window(&format!("{text}scale=1\n"), current_scale), Some(original.clone()));
        }
    }

    #[test]
    fn old_window_file_stays_in_widget_mode() {
        let pref = parse_window("size=480,640\npos=1,2\n").expect("window");
        assert!(!pref.full);
        assert!(!pref.has_full_pos);
        assert_eq!((pref.width, pref.height, pref.x, pref.y), (480.0, 640.0, 1, 2));
    }

    #[test]
    fn full_mode_keeps_the_widget_size() {
        let mut pref = parse_window("size=560,740\npos=12,40\nmode=full\nfull-pos=80,24\n").expect("window");
        assert!(pref.full);
        assert_eq!((pref.full_x, pref.full_y), (80, 24));
        pref.width = 600.0;
        pref.height = 780.0;
        pref.x = 4;
        pref.y = 8;
        let again = parse_window(&format_window(&pref)).expect("round trip");
        assert_eq!(again, pref);
        assert!(!again.has_full_size);
        let grown = parse_window("size=560,740\npos=12,40\nmode=full\nfull-pos=80,24\nfull-size=1440,900\n").expect("grown");
        assert_eq!((grown.full_width, grown.full_height), (1440.0, 900.0));
        assert!(grown.has_full_size);
        assert_eq!((grown.width, grown.height), (560.0, 740.0));
    }

    #[test]
    fn view_settings_keep_the_usual_defaults() {
        let fresh = parse_view("");
        assert_eq!(fresh, ViewPref::standard());
        let custom = parse_view("pin=on\nbattle=0\ngallery=1\nanimate=0\nhotkeys=0\n");
        assert_eq!(custom.pin, 1);
        assert!(!custom.battle);
        assert!(custom.gallery);
        assert!(!custom.animate);
        assert!(!custom.hotkeys);
        assert!(!custom.legacy);
        assert!(fresh.animate);
        assert!(fresh.hotkeys);
        assert!(!fresh.legacy);
        assert_eq!(parse_view(&format_view(&custom)), custom);
        assert_eq!(parse_view("pin=later\n").pin, 0);
    }

    #[test]
    fn legacy_mode_round_trips_without_changing_other_view_settings() {
        let mut expected = parse_view("pin=off\nbattle=0\ngallery=1\nanimate=0\nhotkeys=0\n");
        expected.legacy = true;
        let saved = format_view(&expected);
        assert_eq!(parse_view(&saved), expected);
        assert!(saved.contains("legacy=1\n"));
        for value in ["0", "", "invalid", "true"] {
            assert!(!parse_view(&format!("legacy={value}\n")).legacy);
        }
        assert!(parse_view("legacy= 1 \n").legacy);
    }
}
