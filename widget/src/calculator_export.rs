//! Offline, fixed-size share cards. No native window or screen capture is involved.

use std::fmt::Write as _;
use std::fs::{self, OpenOptions};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use image::{imageops, RgbaImage};
use slint::Image;

const WIDTH: u32 = 1200;
const HEIGHT: u32 = 630;
const ROWS_TOP: f32 = 239.0;
const ROWS_BOTTOM: f32 = 538.0;

pub struct CardRow {
    pub title: String,
    pub quantity: String,
    pub unit_price: String,
    pub subtotal: String,
    pub icon: Image,
    pub unpriced: bool,
}

pub struct CardData {
    pub english: bool,
    pub title: String,
    pub subtitle: String,
    pub quantity: String,
    pub fee: String,
    pub total_label: String,
    pub total: String,
    pub per_unit: String,
    pub notice: String,
    pub metadata: String,
    pub icon: Image,
    pub rows: Vec<CardRow>,
}

struct RowLayout {
    index: usize,
    lines: Vec<String>,
    height: f32,
    continued: bool,
}

/// Save every material on one or more 1200 × 630 JPEG cards, without overwriting
/// earlier exports. The caller should show the returned path and page count.
pub fn export_card(data: &CardData) -> Result<Vec<PathBuf>, String> {
    validate_card(data)?;
    let directory = crate::db::calculator_export_dir();
    fs::create_dir_all(&directory).map_err(|e| format!("無法建立匯出資料夾：{e}"))?;
    let options = render_options();
    let pages = paginate(&data.rows);
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| format!("無法讀取匯出時間：{e}"))?
        .as_millis();
    let mut written = Vec::with_capacity(pages.len());
    for (index, rows) in pages.iter().enumerate() {
        let result = (|| {
            let raster = render_page(data, rows, index + 1, pages.len(), &options)?;
            let mut bytes = Vec::new();
            image::codecs::jpeg::JpegEncoder::new_with_quality(&mut bytes, 94)
                .encode_image(&image::DynamicImage::ImageRgba8(raster))
                .map_err(|e| format!("無法產生 JPG：{e}"))?;
            let stem = format!("gersang-cost-{stamp}-{:02}", index + 1);
            let (path, mut file) = create_unique(&directory, &stem)?;
            if let Err(error) = file.write_all(&bytes).and_then(|_| file.sync_all()) {
                drop(file);
                let _ = fs::remove_file(&path);
                return Err(format!("無法儲存 JPG：{error}"));
            }
            Ok(path)
        })();
        match result {
            Ok(path) => written.push(path),
            Err(error) => {
                // A partial batch is not a complete cost list. Roll back only
                // files created by this invocation, leaving previous exports.
                for path in &written {
                    let _ = fs::remove_file(path);
                }
                return Err(error);
            }
        }
    }
    Ok(written)
}

/// Open a completed export in the operating system's default image viewer.
pub fn open_export(path: &Path) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    let result = std::process::Command::new("open").arg(path).spawn();
    #[cfg(target_os = "windows")]
    let result = std::process::Command::new("explorer.exe").arg(path).spawn();
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let result = std::process::Command::new("xdg-open").arg(path).spawn();
    result
        .map(|_| ())
        .map_err(|e| format!("JPG 已儲存，但無法開啟圖片：{e}"))
}

fn create_unique(directory: &Path, stem: &str) -> Result<(PathBuf, fs::File), String> {
    for suffix in 0..1000 {
        let name = if suffix == 0 {
            format!("{stem}.jpg")
        } else {
            format!("{stem}-{suffix}.jpg")
        };
        let path = directory.join(name);
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(file) => return Ok((path, file)),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(format!("無法建立 JPG：{error}")),
        }
    }
    Err("同名匯出太多，請稍後重試。".into())
}

fn render_options() -> resvg::usvg::Options<'static> {
    let mut options = resvg::usvg::Options::default();
    // Shared labels are escaped text. Also disable SVG resource loading: game
    // icons are painted separately, so cards never need href files or data URLs.
    options.image_href_resolver = resvg::usvg::ImageHrefResolver {
        resolve_data: Box::new(|_, _, _| None),
        resolve_string: Box::new(|_, _| None),
    };
    options.fontdb_mut().load_system_fonts();
    #[cfg(target_os = "windows")]
    let family = "Microsoft JhengHei UI";
    #[cfg(target_os = "macos")]
    let family = "PingFang TC";
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let family = "Noto Sans CJK TC";
    options.font_family = family.into();
    options.fontdb_mut().set_sans_serif_family(family);
    options
}

fn paginate(rows: &[CardRow]) -> Vec<Vec<RowLayout>> {
    let mut pages = Vec::new();
    let mut page = Vec::new();
    let mut height = 0.0;
    for (index, row) in rows.iter().enumerate() {
        let lines = wrap(&row.title, 21.0);
        // Imported snapshots may contain a much longer name than the local
        // catalog. Continue its text on another page instead of clipping it.
        for (part, lines) in lines.chunks(13).enumerate() {
            let row_height = (lines.len() as f32 * 21.0 + 17.0).max(46.0);
            if !page.is_empty() && height + row_height > ROWS_BOTTOM - ROWS_TOP {
                pages.push(page);
                page = Vec::new();
                height = 0.0;
            }
            page.push(RowLayout {
                index,
                lines: lines.to_vec(),
                height: row_height,
                continued: part != 0,
            });
            height += row_height;
        }
    }
    if !page.is_empty() || pages.is_empty() {
        pages.push(page);
    }
    pages
}

fn validate_card(data: &CardData) -> Result<(), String> {
    if wrap(&data.title, 45.0).len() > 3
        || display_width(&data.subtitle) > 64.0
        || wrap(&data.notice, 23.0).len() > 4
        || display_width(&data.metadata) > 65.0
    {
        return Err(if data.english {
            "This calculation has text too long for a readable JPG card. Share its offline code instead."
        } else {
            "這份計算的文字太長，無法完整放進清晰的 JPG 卡片；請改用離線分享碼。"
        }.into());
    }
    Ok(())
}

fn render_page(
    data: &CardData,
    rows: &[RowLayout],
    page: usize,
    pages: usize,
    options: &resvg::usvg::Options<'_>,
) -> Result<RgbaImage, String> {
    let svg = page_svg(data, rows, page, pages);
    let tree =
        resvg::usvg::Tree::from_str(&svg, options).map_err(|e| format!("無法排版成本卡：{e}"))?;
    let mut pixels = resvg::tiny_skia::Pixmap::new(WIDTH, HEIGHT)
        .ok_or_else(|| "無法建立成本卡圖片。".to_string())?;
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::default(),
        &mut pixels.as_mut(),
    );
    // The SVG paints an opaque canvas, so its premultiplied RGBA is ordinary RGBA.
    let mut raster = RgbaImage::from_raw(WIDTH, HEIGHT, pixels.data().to_vec())
        .ok_or_else(|| "成本卡圖片尺寸不正確。".to_string())?;
    paint_icon(&mut raster, &data.icon, 50, 65, 64);
    let mut y = ROWS_TOP;
    for layout in rows {
        paint_icon(
            &mut raster,
            &data.rows[layout.index].icon,
            414,
            y as u32 + 9,
            26,
        );
        y += layout.height;
    }
    Ok(raster)
}

fn page_svg(data: &CardData, rows: &[RowLayout], page: usize, pages: usize) -> String {
    let mut svg = format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="{WIDTH}" height="{HEIGHT}" viewBox="0 0 {WIDTH} {HEIGHT}"><rect width="1200" height="630" fill="#15191f"/><rect width="1200" height="4" fill="#d8b77b"/><g font-family="sans-serif">"##
    );
    let label = |zh, en| if data.english { en } else { zh };
    text(
        &mut svg,
        44.0,
        35.0,
        14.0,
        "#c9d2df",
        500,
        "start",
        label("巨商圖鑑 · 合成成本", "GERSANG  /  CRAFTING COST"),
    );
    text(
        &mut svg,
        1156.0,
        35.0,
        13.0,
        "#929dad",
        400,
        "end",
        &format!("{page:02} / {pages:02}"),
    );
    let _ = write!(
        svg,
        r##"<rect x="44" y="59" width="76" height="76" rx="10" fill="#27303b"/>"##
    );
    let title_size = [31.0, 27.0, 24.0, 21.0]
        .into_iter()
        .find(|size| wrap(&data.title, 960.0 / size).len() <= 2)
        .unwrap_or(21.0);
    let title_lines = wrap(&data.title, 960.0 / title_size);
    let title_y = if title_lines.len() > 2 { 76.0 } else { 91.0 };
    let title_step = if title_lines.len() > 2 { 26.0 } else { 33.0 };
    for (index, line) in title_lines.iter().enumerate() {
        text(
            &mut svg,
            140.0,
            title_y + index as f32 * title_step,
            title_size,
            "#edf1f7",
            600,
            "start",
            line,
        );
    }
    let subtitle_y = if title_lines.len() > 1 { 149.0 } else { 123.0 };
    text(
        &mut svg,
        140.0,
        subtitle_y,
        15.0,
        "#a8b2c1",
        400,
        "start",
        &data.subtitle,
    );
    line(&mut svg, 44.0, 169.0, 1156.0, "#303945");

    let _ = write!(
        svg,
        r##"<rect x="44" y="192" width="332" height="351" rx="10" fill="#1d232c"/>"##
    );
    text(
        &mut svg,
        66.0,
        226.0,
        fitted_size(&data.total_label, 288.0, 15.0),
        "#c9d2df",
        500,
        "start",
        &data.total_label,
    );
    let total_size = fitted_size(&data.total, 288.0, 40.0);
    text(
        &mut svg,
        66.0,
        278.0,
        total_size,
        "#d8b77b",
        600,
        "start",
        &data.total,
    );
    text(
        &mut svg,
        66.0,
        305.0,
        13.0,
        "#929dad",
        400,
        "start",
        label(
            "遊戲幣 · 依輸入單價計算",
            "Game coins · supplied unit prices",
        ),
    );
    line(&mut svg, 66.0, 329.0, 354.0, "#303945");
    metric(
        &mut svg,
        357.0,
        label("預計產出", "Planned output"),
        &data.quantity,
    );
    metric(
        &mut svg,
        393.0,
        label("製作費用", "Crafting fees"),
        &data.fee,
    );
    metric(
        &mut svg,
        429.0,
        label("每件成本", "Cost per output"),
        &data.per_unit,
    );
    for (index, sentence) in wrap(&data.notice, 23.0).iter().enumerate() {
        text(
            &mut svg,
            66.0,
            465.0 + index as f32 * 19.0,
            12.0,
            "#a8b2c1",
            400,
            "start",
            sentence,
        );
    }

    text(
        &mut svg,
        414.0,
        213.0,
        13.0,
        "#929dad",
        500,
        "start",
        label("材料明細", "MATERIAL BREAKDOWN"),
    );
    text(
        &mut svg,
        884.0,
        213.0,
        13.0,
        "#929dad",
        400,
        "end",
        label("數量", "Qty"),
    );
    text(
        &mut svg,
        1018.0,
        213.0,
        13.0,
        "#929dad",
        400,
        "end",
        label("單價", "Unit price"),
    );
    text(
        &mut svg,
        1156.0,
        213.0,
        13.0,
        "#929dad",
        400,
        "end",
        label("小計", "Subtotal"),
    );
    line(&mut svg, 414.0, 228.0, 1156.0, "#303945");
    let mut y = ROWS_TOP;
    for layout in rows {
        let row = &data.rows[layout.index];
        for (index, name) in layout.lines.iter().enumerate() {
            text(
                &mut svg,
                452.0,
                y + 26.0 + index as f32 * 21.0,
                16.0,
                "#edf1f7",
                400,
                "start",
                name,
            );
        }
        text(
            &mut svg,
            884.0,
            y + 26.0,
            fitted_size(&row.quantity, 84.0, 15.0),
            "#c9d2df",
            400,
            "end",
            if layout.continued {
                label("續", "cont.")
            } else {
                &row.quantity
            },
        );
        if !layout.continued {
            text(
                &mut svg,
                1018.0,
                y + 26.0,
                fitted_size(&row.unit_price, 120.0, 15.0),
                if row.unpriced { "#d8b77b" } else { "#a8b2c1" },
                400,
                "end",
                &row.unit_price,
            );
            text(
                &mut svg,
                1156.0,
                y + 26.0,
                fitted_size(&row.subtotal, 124.0, 15.0),
                if row.unpriced { "#d8b77b" } else { "#edf1f7" },
                500,
                "end",
                &row.subtotal,
            );
        }
        y += layout.height;
        line(&mut svg, 414.0, y - 2.0, 1156.0, "#272f3a");
    }
    if rows.is_empty() {
        text(
            &mut svg,
            414.0,
            274.0,
            16.0,
            "#a8b2c1",
            400,
            "start",
            label("此配方沒有材料成本。", "This recipe has no material costs."),
        );
    }
    if data.rows.iter().any(|row| row.unpriced) {
        text(
            &mut svg,
            414.0,
            558.0,
            12.0,
            "#d8b77b",
            400,
            "start",
            label(
                "— 尚未輸入單價；已知成本不包括這些材料。",
                "— Price not supplied; known cost excludes these materials.",
            ),
        );
    }
    line(&mut svg, 44.0, 579.0, 1156.0, "#303945");
    text(
        &mut svg,
        44.0,
        605.0,
        12.0,
        "#929dad",
        400,
        "start",
        &data.metadata,
    );
    text(
        &mut svg,
        1156.0,
        605.0,
        12.0,
        "#a8b2c1",
        500,
        "end",
        label(
            "離線計算 · 價格由使用者提供",
            "OFFLINE · USER-SUPPLIED PRICES",
        ),
    );
    svg.push_str("</g></svg>");
    svg
}

fn metric(svg: &mut String, y: f32, name: &str, value: &str) {
    text(svg, 66.0, y, 13.0, "#a8b2c1", 400, "start", name);
    let size = fitted_size(value, 172.0, 15.0);
    text(svg, 354.0, y, size, "#edf1f7", 500, "end", value);
}

fn text(
    svg: &mut String,
    x: f32,
    y: f32,
    size: f32,
    color: &str,
    weight: u16,
    anchor: &str,
    value: &str,
) {
    let _ = write!(
        svg,
        r#"<text x="{x}" y="{y}" font-size="{size}" fill="{color}" font-weight="{weight}" text-anchor="{anchor}">{}</text>"#,
        escape(value)
    );
}

fn line(svg: &mut String, x: f32, y: f32, end: f32, color: &str) {
    let _ = write!(
        svg,
        r#"<path d="M{x} {y}H{end}" stroke="{color}" stroke-width="1"/>"#
    );
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn char_width(c: char) -> f32 {
    match c {
        ' ' => 0.32,
        'i' | 'l' | 'I' | '.' | ',' | ':' | ';' | '!' | '|' => 0.35,
        'W' | 'M' | 'w' | 'm' => 0.95,
        c if c.is_ascii() => 0.7,
        _ => 1.0,
    }
}

fn display_width(text: &str) -> f32 {
    text.chars().map(char_width).sum()
}

fn fitted_size(text: &str, width: f32, preferred: f32) -> f32 {
    preferred.min(width / display_width(text).max(1.0))
}

fn wrap(text: &str, width: f32) -> Vec<String> {
    let mut lines = Vec::new();
    let mut line = String::new();
    let mut occupied = 0.0;
    for c in text.chars() {
        let next = char_width(c);
        if c == '\n' {
            lines.push(std::mem::take(&mut line));
            occupied = 0.0;
            continue;
        }
        if !line.is_empty() && occupied + next > width {
            // Prefer word boundaries for English, while retaining every
            // character and allowing unspaced Chinese names to wrap naturally.
            if let Some(split) = line.rfind(' ').filter(|split| *split > line.len() / 3) {
                let remainder = line.split_off(split + 1);
                lines.push(std::mem::take(&mut line));
                line = remainder;
                occupied = display_width(&line);
            } else {
                lines.push(std::mem::take(&mut line));
                occupied = 0.0;
            }
        }
        line.push(c);
        occupied += next;
    }
    if !line.is_empty() || lines.is_empty() {
        lines.push(line);
    }
    lines
}

fn paint_icon(canvas: &mut RgbaImage, icon: &Image, x: u32, y: u32, side: u32) {
    let Some(buffer) = icon.to_rgba8() else {
        return;
    };
    if buffer.width() == 0 || buffer.height() == 0 {
        return;
    }
    let Some(source) =
        RgbaImage::from_raw(buffer.width(), buffer.height(), buffer.as_bytes().to_vec())
    else {
        return;
    };
    let scale = (side as f32 / source.width() as f32).min(side as f32 / source.height() as f32);
    let width = (source.width() as f32 * scale).round().max(1.0) as u32;
    let height = (source.height() as f32 * scale).round().max(1.0) as u32;
    let image = imageops::resize(&source, width, height, imageops::FilterType::Nearest);
    imageops::overlay(
        canvas,
        &image,
        (x + (side - width) / 2) as i64,
        (y + (side - height) / 2) as i64,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn card(count: usize) -> CardData {
        CardData {
            english: false,
            title: "強化的高級白水晶".into(),
            subtitle: "合成配方 #1040".into(),
            quantity: "2 件".into(),
            fee: "121,360,000".into(),
            total_label: "已知成本".into(),
            total: "163,320,000".into(),
            per_unit: "—".into(),
            notice: "單價未填齊。\n未計重試成本。".into(),
            metadata: "Catalog 23 · Recipe #1040".into(),
            icon: Image::default(),
            rows: (0..count)
                .map(|index| CardRow {
                    title: format!("材料 {index} <&> 強化的白水晶"),
                    quantity: "238".into(),
                    unit_price: "100,000".into(),
                    subtotal: "23,800,000".into(),
                    icon: Image::default(),
                    unpriced: index == 0,
                })
                .collect(),
        }
    }

    #[test]
    fn pages_preserve_every_row_in_order_and_repeat_totals() {
        let data = card(25);
        let pages = paginate(&data.rows);
        assert!(pages.len() > 1);
        assert_eq!(
            pages
                .iter()
                .flatten()
                .map(|row| row.index)
                .collect::<Vec<_>>(),
            (0..25).collect::<Vec<_>>()
        );
        for (index, page) in pages.iter().enumerate() {
            assert!(page.iter().map(|row| row.height).sum::<f32>() <= ROWS_BOTTOM - ROWS_TOP);
            let svg = page_svg(&data, page, index + 1, pages.len());
            assert!(svg.contains("163,320,000"));
            assert!(svg.contains(&format!("{:02} / {:02}", index + 1, pages.len())));
        }
    }

    #[test]
    fn text_is_escaped_and_unknown_costs_are_explained() {
        let mut data = card(1);
        data.subtitle =
            r#"</text><image href="file:///private/secret"/><script>alert(1)</script>"#.into();
        let pages = paginate(&data.rows);
        let svg = page_svg(&data, &pages[0], 1, 1);
        assert!(svg.contains("&lt;&amp;&gt;"));
        assert!(!svg.contains("材料 0 <&>"));
        assert!(svg.contains("已知成本不包括這些材料"));
        assert!(!svg.contains("<image"));
        assert!(!svg.contains("<script"));
    }

    #[test]
    fn unusually_long_imported_material_name_continues_without_losing_text() {
        let mut data = card(1);
        data.rows[0].title = "白".repeat(1000);
        let pages = paginate(&data.rows);
        assert!(pages.len() > 1);
        let reconstructed = pages
            .iter()
            .flatten()
            .flat_map(|row| row.lines.iter())
            .cloned()
            .collect::<String>();
        assert_eq!(reconstructed, data.rows[0].title);
        assert!(!pages[0][0].continued);
        assert!(pages[1][0].continued);
        for page in pages {
            assert!(page.iter().map(|row| row.height).sum::<f32>() <= ROWS_BOTTOM - ROWS_TOP);
        }
    }

    #[test]
    fn produces_real_1200_by_630_jpeg() {
        let data = card(4);
        let pages = paginate(&data.rows);
        let options = render_options();
        assert!(
            (options.image_href_resolver.resolve_string)("file:///private/secret", &options)
                .is_none()
        );
        assert!((options.image_href_resolver.resolve_data)(
            "image/svg+xml",
            std::sync::Arc::new(b"<svg/>".to_vec()),
            &options
        )
        .is_none());
        let raster = render_page(&data, &pages[0], 1, pages.len(), &options).unwrap();
        let mut encoded = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut encoded, 94)
            .encode_image(&image::DynamicImage::ImageRgba8(raster))
            .unwrap();
        let decoded = image::load_from_memory(&encoded).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (1200, 630));
        assert_eq!(&encoded[..2], &[0xff, 0xd8]);
    }

    #[test]
    fn unique_exports_do_not_replace_existing_files() {
        let directory =
            std::env::temp_dir().join(format!("gs-card-export-test-{}", std::process::id()));
        fs::create_dir_all(&directory).unwrap();
        let (first, mut file) = create_unique(&directory, "same").unwrap();
        file.write_all(b"keep").unwrap();
        drop(file);
        let (second, file) = create_unique(&directory, "same").unwrap();
        drop(file);
        assert_ne!(first, second);
        assert_eq!(fs::read(&first).unwrap(), b"keep");
        fs::remove_dir_all(directory).unwrap();
    }
}
