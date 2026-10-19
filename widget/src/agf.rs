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
    decoded: Vec<Option<Icon>>,
}

fn sheets() -> &'static Mutex<HashMap<PathBuf, Sheet>> {
    static SHEETS: OnceLock<Mutex<HashMap<PathBuf, Sheet>>> = OnceLock::new();
    SHEETS.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn playable_frames(path: &Path) -> Result<Vec<i32>, String> {
    let mut cache = sheets().lock().map_err(|_| "圖集快取無法使用".to_string())?;
    if !cache.contains_key(path) {
        cache.insert(path.to_path_buf(), load_sheet(path)?);
    }
    let sheet = cache.get(path).ok_or("圖集快取遺失")?;
    let frames = sheet
        .frames
        .iter()
        .enumerate()
        .filter(|(index, frame)| {
            // Some action sheets retain fully transparent, positive-size tail
            // frames. Their encoded alpha is enough to skip them without
            // decoding or changing the cached frame pixels.
            frame.2 > 0 && frame.3 > 0
                && sheet.raw.get(sheet.offsets[*index]..sheet.offsets[*index + 1])
                    .is_some_and(|pixels| pixels.chunks_exact(4).any(|pixel| pixel[0] != 0))
        })
        .map(|(index, _)| index as i32)
        .collect();
    Ok(frames)
}

pub fn frame_rgba(path: &Path, frame: i32) -> Result<Icon, String> {
    let mut cache = sheets().lock().map_err(|_| "圖集快取無法使用".to_string())?;
    if !cache.contains_key(path) {
        cache.insert(path.to_path_buf(), load_sheet(path)?);
    }
    let icon = {
        let sheet = cache.get(path).ok_or("圖集快取遺失")?;
        if let Some(icon) = sheet.decoded.get(frame as usize).and_then(|stored| stored.as_ref()) {
            return Ok(copy_icon(icon));
        }
        decode_frame(sheet, frame)?
    };
    if let Some(slot) = cache.get_mut(path).and_then(|sheet| sheet.decoded.get_mut(frame as usize)) {
        *slot = Some(copy_icon(&icon));
    }
    Ok(icon)
}

fn copy_icon(icon: &Icon) -> Icon {
    Icon {
        width: icon.width,
        height: icon.height,
        rgba: icon.rgba.clone(),
    }
}

fn decode_frame(sheet: &Sheet, frame: i32) -> Result<Icon, String> {
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
    let raw = sheet.raw.get(start..end).ok_or("圖格資料超出範圍")?;
    let pixel_count = frame_w.checked_mul(frame_h).ok_or("圖格尺寸超出範圍")?;
    let byte_count = pixel_count.checked_mul(4).ok_or("圖格尺寸超出範圍")?;
    let mut rgba = vec![0u8; byte_count];
    let mut pixel = 0;
    for encoded in raw.chunks_exact(4) {
        if pixel >= pixel_count {
            break;
        }
        let [alpha, red, green, blue] = [encoded[0], encoded[1], encoded[2], encoded[3]];
        if alpha == 0 && green == 0 && blue == 0 {
            // The output is already transparent: a run needs no temporary pixels or copy.
            pixel = pixel.saturating_add(red as usize);
        } else {
            let at = pixel * 4;
            rgba[at..at + 4].copy_from_slice(&[red, green, blue, alpha]);
            pixel += 1;
        }
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
    Ok(Sheet {
        decoded: (0..frames.len()).map(|_| None).collect(),
        raw,
        offsets,
        frames,
    })
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

#[cfg(test)]
mod tests {
    use super::{decode_frame, Icon, Sheet};

    fn sheet(raw: Vec<u8>, width: usize, height: usize) -> Sheet {
        Sheet {
            offsets: vec![0, raw.len()],
            raw,
            frames: vec![(0, 0, width, height)],
            decoded: vec![None],
        }
    }

    #[test]
    fn decodes_colors_transparent_runs_and_short_frames_directly() {
        let data = sheet(vec![255, 1, 2, 3, 0, 2, 0, 0, 127, 4, 5, 6], 5, 1);
        let icon = decode_frame(&data, 0).unwrap();
        assert_eq!((icon.width, icon.height), (5, 1));
        assert_eq!(icon.rgba, vec![1, 2, 3, 255, 0, 0, 0, 0, 0, 0, 0, 0, 4, 5, 6, 127, 0, 0, 0, 0]);
    }

    #[test]
    fn clips_runs_and_extra_pixels_at_the_frame_boundary() {
        let data = sheet(vec![0, 255, 0, 0, 255, 1, 2, 3], 2, 1);
        assert_eq!(decode_frame(&data, 0).unwrap().rgba, vec![0; 8]);
        let data = sheet(vec![255, 1, 2, 3, 255, 4, 5, 6], 1, 1);
        assert_eq!(decode_frame(&data, 0).unwrap().rgba, vec![1, 2, 3, 255]);
        assert!(decode_frame(&data, -1).is_err());
        assert!(decode_frame(&data, 1).is_err());
        assert!(decode_frame(&sheet(vec![], 0, 1), 0).is_err());
    }

    // Keep the previous decoder only in this manual comparison so benchmarks use
    // exactly the same checked-in AGF assets and verify pixel-for-pixel output.
    fn legacy_decode(sheet: &Sheet, frame: i32) -> Icon {
        let frame = frame as usize;
        let (_, _, width, height) = sheet.frames[frame];
        let mut pixels = Vec::new();
        for data in sheet.raw[sheet.offsets[frame]..sheet.offsets[frame + 1]].chunks_exact(4) {
            let (alpha, red, green, blue) = (data[0], data[1], data[2], data[3]);
            if alpha == 0 && green == 0 && blue == 0 {
                pixels.extend(std::iter::repeat((0, 0, 0, 0)).take(red as usize));
            } else {
                pixels.push((red, green, blue, alpha));
            }
        }
        let mut rgba = vec![0; width * height * 4];
        for (index, pixel) in pixels.into_iter().take(width * height).enumerate() {
            rgba[index * 4..index * 4 + 4].copy_from_slice(&[pixel.0, pixel.1, pixel.2, pixel.3]);
        }
        Icon { width: width as u32, height: height as u32, rgba }
    }

    #[test]
    #[ignore = "manual benchmark: decodes all bundled item frames with both implementations"]
    fn profile_item_frame_decode() {
        use std::time::{Duration, Instant};
        let assets = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../data/client/Assets/Textures/Items");
        let mut baseline = Duration::ZERO;
        let mut optimized = Duration::ZERO;
        let mut frame_count = 0;
        for entry in std::fs::read_dir(assets).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().and_then(|ext| ext.to_str()).is_none_or(|ext| !ext.eq_ignore_ascii_case("agf")) {
                continue;
            }
            let sheet = super::load_sheet(&path).unwrap();
            for (frame, info) in sheet.frames.iter().enumerate() {
                if info.2 == 0 || info.3 == 0 { continue; }
                let started = Instant::now();
                let expected = legacy_decode(&sheet, frame as i32);
                baseline += started.elapsed();
                let started = Instant::now();
                let actual = decode_frame(&sheet, frame as i32).unwrap();
                optimized += started.elapsed();
                assert_eq!(expected.rgba, actual.rgba, "{} frame {frame}", path.display());
                frame_count += 1;
            }
        }
        eprintln!("AGF frames={frame_count} baseline_ms={:.3} direct_ms={:.3}", baseline.as_secs_f64() * 1000.0, optimized.as_secs_f64() * 1000.0);
    }
}
