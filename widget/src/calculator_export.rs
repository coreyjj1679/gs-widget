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
const ROW_LEFT: f32 = 414.0;
const TITLE_LEFT: f32 = 452.0;
const NAME_RIGHT: f32 = 788.0;
const INDENT: f32 = 18.0;
const MAX_DEPTH: usize = crate::db::MAX_NESTED_DEPTH;
const MAX_ROWS: usize = crate::db::MAX_TOTAL_MATERIALS;
const MAX_PAGES: usize = 128;

pub struct CardRow {
    pub title: String,
    pub depth: usize,
    /// Ancestor names; repeated as context when a subtree spans pages.
    pub context: String,
    /// This subtotal includes its displayed descendants and is counted once.
    pub nested: bool,
    pub detail: String,
    pub quantity: String,
    pub unit_price: String,
    pub subtotal: String,
    pub icon: Image,
    pub unpriced: bool,
}

pub struct CardData {
    pub language: String,
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
    details: Vec<String>,
    context: Vec<String>,
    height: f32,
    continued: bool,
}

impl RowLayout {
    fn context_height(&self) -> f32 {
        context_height(&self.context)
    }
}

fn context_height(lines: &[String]) -> f32 {
    if lines.is_empty() {
        0.0
    } else {
        lines.len() as f32 * 15.0 + 8.0
    }
}

fn row_text_width(row: &CardRow) -> f32 {
    NAME_RIGHT - TITLE_LEFT - row.depth as f32 * INDENT
}

fn body_height(title_lines: usize, detail_lines: usize) -> f32 {
    (title_lines as f32 * 21.0 + detail_lines as f32 * 17.0 + 17.0).max(46.0)
}

fn export_limit_error(lang: &str) -> String {
    crate::db::ui_text(lang,
        "這份明細太大，無法匯出完整且清晰的 JPG 卡片；請改用離線分享碼。",
        "This breakdown is too large for readable JPG cards. Share its offline code instead.")
}

/// Save every material on one or more 1200 × 630 JPEG cards, without overwriting
/// earlier exports. The caller should show the returned path and page count.
pub fn export_card(data: &CardData) -> Result<Vec<PathBuf>, String> {
    validate_card(data)?;
    let pages = paginate(&data.rows).map_err(|_| export_limit_error(&data.language))?;
    let directory = crate::db::calculator_export_dir();
    fs::create_dir_all(&directory).map_err(|e| format!("無法建立匯出資料夾：{e}"))?;
    let options = render_options();
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

// Only the repeated breadcrumb may abbreviate. Every original title/detail
// remains on its own row(s), so an abbreviated path never discards source data.
fn continuation_context(row: &CardRow, continued: bool) -> Vec<String> {
    let context = if !row.context.is_empty() {
        row.context.as_str()
    } else if continued {
        row.title.as_str()
    } else {
        return Vec::new();
    };
    let mut lines = wrap(context, (1156.0 - ROW_LEFT - 18.0) / 12.0);
    if lines.len() > 2 {
        lines = lines.split_off(lines.len() - 2);
        // Reserve room for the ellipsis without pushing text into a price column.
        lines[0] = format!("… {}", lines[0].trim_start());
        while display_width(&lines[0]) > (1156.0 - ROW_LEFT - 18.0) / 12.0 {
            let at = lines[0].char_indices().nth(2).map(|(at, _)| at);
            if let Some(at) = at {
                lines[0].remove(at);
            } else {
                break;
            }
        }
    }
    lines
}

fn paginate(rows: &[CardRow]) -> Result<Vec<Vec<RowLayout>>, ()> {
    let mut pages = Vec::new();
    let mut page = Vec::new();
    let mut height = 0.0;
    let capacity = ROWS_BOTTOM - ROWS_TOP;
    for (index, row) in rows.iter().enumerate() {
        if row.depth > MAX_DEPTH || rows.len() > MAX_ROWS {
            return Err(());
        }
        let width = row_text_width(row);
        let titles = wrap(&row.title, width / 16.0);
        let details = if row.detail.is_empty() {
            Vec::new()
        } else {
            wrap(&row.detail, width / 12.0)
        };
        let full_height = body_height(titles.len(), details.len());
        // Keep a parent's complete description and at least the beginning of its
        // first child together whenever that pair fits on an otherwise empty page.
        if row.nested
            && rows
                .get(index + 1)
                .is_some_and(|next| next.depth > row.depth)
            && !page.is_empty()
            && height + full_height + 46.0 > capacity
            && full_height + 46.0 + context_height(&continuation_context(row, false)) <= capacity
        {
            pages.push(std::mem::take(&mut page));
            height = 0.0;
        }
        let mut title_at = 0;
        let mut detail_at = 0;
        let mut continued = false;
        while title_at < titles.len() || detail_at < details.len() {
            if pages.len() >= MAX_PAGES {
                return Err(());
            }
            let context = if page.is_empty() && (row.depth > 0 || continued) {
                continuation_context(row, continued)
            } else {
                Vec::new()
            };
            let context_h = context_height(&context);
            let available = capacity - height - context_h;
            let remainder_height = body_height(titles.len() - title_at, details.len() - detail_at);
            let fresh_context_h = context_height(&continuation_context(row, continued));
            // Move an ordinary row intact; split only text too tall for a page.
            // A first child may use the space deliberately reserved by its parent.
            let after_parent = page.last().is_some_and(|previous: &RowLayout| {
                previous.index + 1 == index
                    && rows[previous.index].nested
                    && row.depth > rows[previous.index].depth
            });
            if !page.is_empty()
                && (available < 46.0
                    || (remainder_height > available
                        && remainder_height + fresh_context_h <= capacity
                        && !after_parent))
            {
                pages.push(std::mem::take(&mut page));
                height = 0.0;
                continue;
            }
            let mut next_title = title_at;
            let mut next_detail = detail_at;
            while next_title < titles.len()
                && body_height(next_title + 1 - title_at, 0) <= available
            {
                next_title += 1;
            }
            if next_title == titles.len() {
                while next_detail < details.len()
                    && body_height(next_title - title_at, next_detail + 1 - detail_at) <= available
                {
                    next_detail += 1;
                }
            }
            if next_title == title_at && next_detail == detail_at {
                return Err(());
            }
            let row_height =
                body_height(next_title - title_at, next_detail - detail_at) + context_h;
            page.push(RowLayout {
                index,
                lines: titles[title_at..next_title].to_vec(),
                details: details[detail_at..next_detail].to_vec(),
                context,
                height: row_height,
                continued,
            });
            height += row_height;
            title_at = next_title;
            detail_at = next_detail;
            continued = true;
            if title_at < titles.len() || detail_at < details.len() {
                pages.push(std::mem::take(&mut page));
                height = 0.0;
            }
        }
    }
    if !page.is_empty() || pages.is_empty() {
        pages.push(page);
    }
    if pages.len() > MAX_PAGES {
        return Err(());
    }
    Ok(pages)
}

fn validate_card(data: &CardData) -> Result<(), String> {
    if data.rows.len() > MAX_ROWS
        || data.rows.iter().enumerate().any(|(index, row)| {
            row.depth > MAX_DEPTH
                || row.title.is_empty()
                || row.title.len() > 4096
                || row.detail.len() > 4096
                || row.context.len() > 16384
                || (index == 0 && row.depth != 0)
                || (index > 0
                    && (row.depth > data.rows[index - 1].depth + 1
                        || (row.depth > data.rows[index - 1].depth
                            && !data.rows[index - 1].nested)))
                || (row.nested
                    && !data
                        .rows
                        .get(index + 1)
                        .is_some_and(|next| next.depth == row.depth + 1))
        })
    {
        return Err(export_limit_error(&data.language));
    }
    if wrap(&data.title, 45.0).len() > 3
        || display_width(&data.subtitle) > 64.0
        || wrap(&data.notice, 23.0).len() > 4
        || display_width(&data.metadata) > 65.0
    {
        return Err(crate::db::ui_text(&data.language,
            "這份計算的文字太長，無法完整放進清晰的 JPG 卡片；請改用離線分享碼。",
            "This calculation has text too long for a readable JPG card. Share its offline code instead."));
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
            (ROW_LEFT + data.rows[layout.index].depth as f32 * INDENT) as u32,
            (y + layout.context_height()) as u32 + 9,
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
    let label = |zh, en| crate::db::ui_text_ref(&data.language, zh, en);
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
        if data.rows.iter().any(|row| row.nested) {
            label("主配方費用", "Top-level fee")
        } else {
            label("製作費用", "Crafting fees")
        },
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
    if data.rows.iter().any(|row| row.nested) {
        text(
            &mut svg,
            ROW_LEFT,
            187.0,
            11.0,
            "#a8b2c1",
            400,
            "start",
            label(
                "縮排子項已計入上層小計，請勿重複相加。",
                "Indented costs are included in their parent subtotal.",
            ),
        );
    }
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
        for (index, context) in layout.context.iter().enumerate() {
            text(
                &mut svg,
                ROW_LEFT + 18.0,
                y + 12.0 + index as f32 * 15.0,
                12.0,
                "#929dad",
                400,
                "start",
                context,
            );
        }
        if !layout.context.is_empty() {
            text(
                &mut svg,
                ROW_LEFT,
                y + 12.0,
                12.0,
                "#d8b77b",
                500,
                "start",
                "↳",
            );
        }
        let body_y = y + layout.context_height();
        let title_x = TITLE_LEFT + row.depth as f32 * INDENT;
        if row.depth > 0 {
            let branch_x = ROW_LEFT + row.depth as f32 * INDENT - 8.0;
            let _ = write!(
                svg,
                r##"<path d="M{branch_x} {}v12h5" stroke="#48505a" fill="none"/>"##,
                body_y + 10.0
            );
        }
        for (index, name) in layout.lines.iter().enumerate() {
            text(
                &mut svg,
                title_x,
                body_y + 26.0 + index as f32 * 21.0,
                16.0,
                "#edf1f7",
                if row.nested { 600 } else { 400 },
                "start",
                name,
            );
        }
        for (index, detail) in layout.details.iter().enumerate() {
            text(
                &mut svg,
                title_x,
                body_y + 22.0 + layout.lines.len() as f32 * 21.0 + index as f32 * 17.0,
                12.0,
                "#a8b2c1",
                400,
                "start",
                detail,
            );
        }
        text(
            &mut svg,
            884.0,
            body_y + 26.0,
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
                body_y + 26.0,
                fitted_size(&row.unit_price, 120.0, 15.0),
                if row.unpriced { "#d8b77b" } else { "#a8b2c1" },
                400,
                "end",
                &row.unit_price,
            );
            text(
                &mut svg,
                1156.0,
                body_y + 26.0,
                fitted_size(&row.subtotal, 124.0, 15.0),
                if row.unpriced { "#d8b77b" } else { "#edf1f7" },
                if row.nested { 600 } else { 500 },
                "end",
                &row.subtotal,
            );
        }
        y += layout.height;
        line(&mut svg, ROW_LEFT, y - 2.0, 1156.0, "#272f3a");
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
            if data.rows.iter().any(|row| row.nested) {
                label(
                    "未知部分未計入已知成本；上層小計已含已填子項。",
                    "Unknown portions are excluded; parent subtotals include priced children.",
                )
            } else {
                label(
                    "— 尚未輸入單價；已知成本不包括這些材料。",
                    "— Price not supplied; known cost excludes these materials.",
                )
            },
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
            language: "zh".into(),
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
                    depth: 0,
                    context: String::new(),
                    nested: false,
                    detail: String::new(),
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
        let pages = paginate(&data.rows).unwrap();
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
        let pages = paginate(&data.rows).unwrap();
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
        let pages = paginate(&data.rows).unwrap();
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
        let pages = paginate(&data.rows).unwrap();
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

    fn nested_card(count: usize) -> CardData {
        let mut data = card(0);
        while data.rows.len() < count {
            let mut ancestors = Vec::new();
            for depth in 0..=MAX_DEPTH {
                if data.rows.len() == count {
                    break;
                }
                let index = data.rows.len();
                let title = format!("材料 {index} · 強化的白水晶 / Refined crystal");
                let nested = depth < MAX_DEPTH && index + 1 < count;
                data.rows.push(CardRow {
                    title: title.clone(),
                    depth,
                    context: ancestors.join(" › "),
                    nested,
                    detail: if nested {
                        "合成 2 批 · 產出 6 · 多出 2 · 已含製作費 10".into()
                    } else {
                        "原料價格由用家提供 / User-supplied price".into()
                    },
                    quantity: "4".into(),
                    unit_price: "9.50".into(),
                    subtotal: "38".into(),
                    icon: Image::default(),
                    unpriced: index % 7 == 0,
                });
                ancestors.push(title);
            }
        }
        data
    }

    fn assert_complete_layout(data: &CardData, pages: &[Vec<RowLayout>]) {
        let mut titles = vec![String::new(); data.rows.len()];
        let mut details = vec![String::new(); data.rows.len()];
        let mut first_fragments = Vec::new();
        for page in pages {
            assert!(!page.is_empty());
            assert!(page.iter().map(|row| row.height).sum::<f32>() <= ROWS_BOTTOM - ROWS_TOP);
            for row in page {
                if !row.continued {
                    first_fragments.push(row.index);
                }
                titles[row.index].push_str(&row.lines.concat());
                details[row.index].push_str(&row.details.concat());
                let width = row_text_width(&data.rows[row.index]);
                for line in &row.lines {
                    assert!(display_width(line) * 16.0 <= width + 0.1);
                }
                for line in &row.details {
                    assert!(display_width(line) * 12.0 <= width + 0.1);
                }
            }
            if data.rows[page[0].index].depth > 0 {
                assert!(
                    !page[0].context.is_empty(),
                    "nested page must retain ancestor context"
                );
            }
        }
        assert_eq!(first_fragments, (0..data.rows.len()).collect::<Vec<_>>());
        for (index, row) in data.rows.iter().enumerate() {
            assert_eq!(titles[index], row.title);
            assert_eq!(details[index], row.detail);
        }
    }

    #[test]
    fn nested_pagination_preserves_134_rows_through_depth_eight() {
        let data = nested_card(134);
        validate_card(&data).unwrap();
        let pages = paginate(&data.rows).unwrap();
        assert!(pages.len() > 10);
        assert_complete_layout(&data, &pages);
        for (index, page) in pages.iter().enumerate() {
            let svg = page_svg(&data, page, index + 1, pages.len());
            assert!(svg.contains("主配方費用"));
            assert!(svg.contains("子項已計入上層小計"));
            assert!(svg.contains("上層小計已含已填子項"));
            assert!(svg.contains(&data.total));
        }
    }

    #[test]
    fn parent_moves_with_first_child_at_a_page_boundary() {
        let mut data = card(7);
        data.rows[5].nested = true;
        data.rows[6].depth = 1;
        data.rows[6].context = data.rows[5].title.clone();
        validate_card(&data).unwrap();
        let pages = paginate(&data.rows).unwrap();
        assert_eq!(
            pages[0].iter().map(|row| row.index).collect::<Vec<_>>(),
            vec![0, 1, 2, 3, 4]
        );
        assert_eq!(
            pages[1].iter().map(|row| row.index).collect::<Vec<_>>(),
            vec![5, 6]
        );
        assert_complete_layout(&data, &pages);
    }

    #[test]
    fn long_nested_title_and_detail_continue_without_losing_or_executing_text() {
        let mut data = card(2);
        data.rows[0].nested = true;
        data.rows[1].depth = 1;
        data.rows[1].context = data.rows[0].title.clone();
        data.rows[1].title = "白".repeat(1000);
        data.rows[1].detail =
            "Detail <image href='file:///private/secret'/> & <script>bad()</script> ".repeat(30);
        validate_card(&data).unwrap();
        let pages = paginate(&data.rows).unwrap();
        assert!(pages.len() > 5);
        assert_complete_layout(&data, &pages);
        for (index, page) in pages.iter().enumerate() {
            let svg = page_svg(&data, page, index + 1, pages.len());
            assert!(!svg.contains("<image"));
            assert!(!svg.contains("<script"));
        }
    }

    #[test]
    fn invalid_trees_and_excessive_page_counts_are_rejected() {
        let mut data = card(2);
        data.rows[1].depth = 1; // Its parent was not declared a rollup.
        assert!(validate_card(&data).is_err());
        data.rows[0].nested = true;
        data.rows[1].depth = MAX_DEPTH + 1;
        assert!(validate_card(&data).is_err());
        data = card(MAX_ROWS + 1);
        assert!(validate_card(&data).is_err());
        data = card(MAX_ROWS);
        for row in &mut data.rows {
            row.title = "白".repeat(1000);
        }
        assert!(paginate(&data.rows).is_err());
    }

    fn encode_page(
        data: &CardData,
        page: &[RowLayout],
        number: usize,
        count: usize,
        options: &resvg::usvg::Options<'_>,
    ) -> Vec<u8> {
        let raster = render_page(data, page, number, count, options).unwrap();
        let mut encoded = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut encoded, 94)
            .encode_image(&image::DynamicImage::ImageRgba8(raster))
            .unwrap();
        let decoded = image::load_from_memory(&encoded).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (WIDTH, HEIGHT));
        assert_eq!(&encoded[..2], &[0xff, 0xd8]);
        encoded
    }

    #[test]
    fn every_nested_page_encodes_as_a_complete_jpeg() {
        let mut data = nested_card(25);
        data.language = "en".into();
        let pages = paginate(&data.rows).unwrap();
        assert_complete_layout(&data, &pages);
        let options = render_options();
        for (index, page) in pages.iter().enumerate() {
            let _ = encode_page(&data, page, index + 1, pages.len(), &options);
        }
    }

    /// Explicit artifact generation for visual QA, not part of the normal suite.
    #[test]
    #[ignore = "writes first/middle/last nested JPG previews for visual review"]
    fn writes_nested_card_previews() {
        let data = nested_card(134);
        let pages = paginate(&data.rows).unwrap();
        assert_complete_layout(&data, &pages);
        let directory = std::env::temp_dir().join("gs-nested-card-previews");
        fs::create_dir_all(&directory).unwrap();
        let options = render_options();
        for index in [0, pages.len() / 2, pages.len() - 1] {
            let bytes = encode_page(&data, &pages[index], index + 1, pages.len(), &options);
            let path = directory.join(format!("nested-cost-{:03}.jpg", index + 1));
            fs::write(&path, bytes).unwrap();
            println!("{}", path.display());
        }
    }
}
