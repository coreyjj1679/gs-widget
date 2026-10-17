use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use flate2::read::ZlibDecoder;

pub struct Icon {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

struct Sheet {
    raw: Vec<u8>,
    offsets: Vec<usize>,
    frames: Vec<(i32, i32, usize, usize)>,
}

fn sheets() -> &'static Mutex<HashMap<PathBuf, Sheet>> {
    static SHEETS: OnceLock<Mutex<HashMap<PathBuf, Sheet>>> = OnceLock::new();
    SHEETS.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn frame_rgba(path: &Path, frame: i32) -> Result<Icon, String> {
    let mut cache = sheets().lock().map_err(|_| "圖集快取無法使用".to_string())?;
    if !cache.contains_key(path) {
        cache.insert(path.to_path_buf(), load_sheet(path)?);
    }
    let sheet = cache.get(path).ok_or("圖集快取遺失")?;
    if frame < 0 || frame as usize >= sheet.frames.len() {
        return Err("圖格超出範圍".into());
    }
    let frame = frame as usize;
    let (_origin_x, _origin_y, frame_w, frame_h) = sheet.frames[frame];
    if frame_w == 0 || frame_h == 0 {
        return Err("空白圖格".into());
    }
    let start = sheet.offsets[frame];
    let end = sheet.offsets[frame + 1];
    let raw = &sheet.raw;
    let mut pixels = Vec::new();
    let mut pos = start;
    let target = end.saturating_sub(start) / 4;
    let mut produced = 0;
    while produced < target && pos + 4 <= raw.len() {
        let alpha = raw[pos];
        let red = raw[pos + 1];
        let green = raw[pos + 2];
        let blue = raw[pos + 3];
        pos += 4;
        produced += 1;
        if alpha == 0 && green == 0 && blue == 0 {
            pixels.extend(std::iter::repeat((0, 0, 0, 0)).take(red as usize));
        } else {
            pixels.push((red, green, blue, alpha));
        }
    }
    let mut rgba = vec![0u8; frame_w * frame_h * 4];
    let count = pixels.len().min(frame_w * frame_h);
    for (index, pixel) in pixels.into_iter().take(count).enumerate() {
        let at = index * 4;
        rgba[at..at + 4].copy_from_slice(&[pixel.0, pixel.1, pixel.2, pixel.3]);
    }
    Ok(Icon {
        width: frame_w as u32,
        height: frame_h as u32,
        rgba,
    })
}

fn load_sheet(path: &Path) -> Result<Sheet, String> {
    let mut file = File::open(path).map_err(|err| err.to_string())?;
    let mut data = Vec::new();
    file.read_to_end(&mut data).map_err(|err| err.to_string())?;
    if data.len() < 16 || data[..4] != [0x23, 0, 0, 0] {
        return Err("不是道具圖集".into());
    }
    let count = u32_at(&data, 12) as usize;
    let unzip_size = u32_at(&data, count * 8 + 0x970) as usize;
    let zip_at = count * 8 + 0x99C;
    let zip_size = u32_at(&data, zip_at) as usize;
    let zip_start = zip_at + 4;
    let zip_end = zip_start + zip_size;
    if zip_end > data.len() {
        return Err("圖集被截斷".into());
    }
    let mut decoder = ZlibDecoder::new(&data[zip_start..zip_end]);
    let mut raw = Vec::new();
    decoder.read_to_end(&mut raw).map_err(|err| err.to_string())?;
    if raw.len() != unzip_size {
        return Err("圖集解壓長度不符".into());
    }
    let mut frames = Vec::with_capacity(count);
    let mut offsets = Vec::with_capacity(count + 1);
    for frame in 0..count {
        frames.push(frame_info(&data, frame)?);
        offsets.push(u32_at(&data, 0x4C0 + frame * 4) as usize);
    }
    offsets.push(raw.len());
    Ok(Sheet { raw, offsets, frames })
}

fn frame_info(data: &[u8], frame: usize) -> Result<(i32, i32, usize, usize), String> {
    let offset = (frame + 1) * 8 + 0x968;
    if offset + 8 > data.len() {
        return Err("圖格資訊超出範圍".into());
    }
    let x = i16::from_le_bytes([data[offset], data[offset + 1]]) as i32;
    let y = i16::from_le_bytes([data[offset + 2], data[offset + 3]]) as i32;
    let w = i16::from_le_bytes([data[offset + 4], data[offset + 5]]) as usize;
    let h = i16::from_le_bytes([data[offset + 6], data[offset + 7]]) as usize;
    Ok((x, y, w, h))
}

fn u32_at(data: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([data[offset], data[offset + 1], data[offset + 2], data[offset + 3]])
}
