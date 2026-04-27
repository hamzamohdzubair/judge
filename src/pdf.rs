use anyhow::Result;
use printpdf::{
    image_crate,
    path::{PaintMode, WindingOrder},
    Color, Image, ImageTransform, IndirectFontRef, Line, Mm, PdfDocument, PdfDocumentReference,
    PdfLayerReference, Point, Polygon, Rgb,
};

use crate::config::interviewer_name;
use crate::export::ExportData;
use crate::models::{Question, TopicData};

// Embedded Inter (OFL-licensed). License at fonts/INTER-LICENSE.txt.
const INTER_REGULAR: &[u8] = include_bytes!("../fonts/Inter-Regular.ttf");
const INTER_BOLD: &[u8] = include_bytes!("../fonts/Inter-Bold.ttf");
const EPAM_LOGO_PNG: &[u8] = include_bytes!("../assets/epam-logo.png");

// ── Layout ────────────────────────────────────────────────────────────────────
const PAGE_W: f32 = 210.0;
const PAGE_H: f32 = 297.0;
const MARGIN_L: f32 = 20.0;
const MARGIN_R: f32 = 20.0;
const MARGIN_B: f32 = 20.0;
const CONTENT_W: f32 = PAGE_W - MARGIN_L - MARGIN_R;
const TOP_Y: f32 = PAGE_H - 28.0;

// Table column fractions (question / response / points)
const COL_Q_FRAC: f32 = 0.60;
const COL_R_FRAC: f32 = 0.24;

fn cols() -> (f32, f32, f32) {
    let cq = CONTENT_W * COL_Q_FRAC;
    let cr = CONTENT_W * COL_R_FRAC;
    (cq, cr, CONTENT_W - cq - cr)
}

// ── Modern slate + indigo palette ────────────────────────────────────────────
fn c(r: u8, g: u8, b: u8) -> Color {
    Color::Rgb(Rgb::new(
        r as f32 / 255.0,
        g as f32 / 255.0,
        b as f32 / 255.0,
        None,
    ))
}
fn c_primary() -> Color { c(79, 70, 229) }    // indigo-600
fn c_primary_d() -> Color { c(67, 56, 202) }  // indigo-700
fn c_accent() -> Color { c(139, 92, 246) }    // violet-500
fn c_dark()    -> Color { c(15, 23, 42) }     // slate-900
fn c_text()    -> Color { c(30, 41, 59) }     // slate-800
fn c_muted()   -> Color { c(100, 116, 139) }  // slate-500
fn c_soft()    -> Color { c(148, 163, 184) }  // slate-400
fn c_panel()   -> Color { c(248, 250, 252) }  // slate-50
fn c_panel2()  -> Color { c(241, 245, 249) }  // slate-100
fn c_border()  -> Color { c(226, 232, 240) }  // slate-200
fn c_white()   -> Color { c(255, 255, 255) }
fn c_success() -> Color { c(5, 150, 105) }    // emerald-600
fn c_warning() -> Color { c(217, 119, 6) }    // amber-600
fn c_danger()  -> Color { c(225, 29, 72) }    // rose-600
fn c_gold()    -> Color { c(202, 138, 4) }    // yellow-700
fn c_silver()  -> Color { c(148, 163, 184) }  // slate-400
fn c_bronze()  -> Color { c(154, 52, 18) }    // orange-800

// ── Utilities ─────────────────────────────────────────────────────────────────
fn title_case(slug: &str) -> String {
    slug.split(|ch: char| ch == '-' || ch == '_' || ch.is_whitespace())
        .filter(|s| !s.is_empty())
        .map(|w| {
            // Short all-lowercase segments are almost always acronyms
            // (nlp, ml, ai, api, cv, llm, gpu, …) — uppercase them.
            if w.len() <= 3 && w.chars().all(|c| c.is_ascii_lowercase()) {
                return w.to_uppercase();
            }
            let mut it = w.chars();
            match it.next() {
                None => String::new(),
                Some(c0) => c0
                    .to_uppercase()
                    .chain(it.flat_map(|c| c.to_lowercase()))
                    .collect(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

// Inter widths are a bit narrower than Helvetica; these are conservative
// (overestimating) factors so wrapping and right-alignment never overflow.
fn text_width_mm(s: &str, size_pt: f32, bold: bool) -> f32 {
    let em = if bold { 0.56 } else { 0.52 };
    s.chars().count() as f32 * size_pt * em * 0.3528
}

fn wrap_text(text: &str, max_w_mm: f32, size_pt: f32, bold: bool) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    let mut current = String::new();
    for word in text.split_whitespace() {
        let candidate = if current.is_empty() {
            word.to_string()
        } else {
            format!("{} {}", current, word)
        };
        if text_width_mm(&candidate, size_pt, bold) <= max_w_mm {
            current = candidate;
        } else if current.is_empty() {
            lines.push(word.to_string());
        } else {
            lines.push(current);
            current = word.to_string();
        }
    }
    if !current.is_empty() {
        lines.push(current);
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

// ── Drawing primitives ────────────────────────────────────────────────────────
fn put_text(layer: &PdfLayerReference, s: &str, size_pt: f32, x: f32, y: f32, font: &IndirectFontRef) {
    layer.use_text(s, size_pt, Mm(x), Mm(y), font);
}

fn fill_rect(layer: &PdfLayerReference, x: f32, y: f32, w: f32, h: f32, fill: Color) {
    layer.set_fill_color(fill);
    layer.add_polygon(Polygon {
        rings: vec![vec![
            (Point::new(Mm(x), Mm(y)), false),
            (Point::new(Mm(x + w), Mm(y)), false),
            (Point::new(Mm(x + w), Mm(y + h)), false),
            (Point::new(Mm(x), Mm(y + h)), false),
        ]],
        mode: PaintMode::Fill,
        winding_order: WindingOrder::NonZero,
    });
}

fn hline(layer: &PdfLayerReference, x1: f32, x2: f32, y: f32, color: Color) {
    layer.set_outline_color(color);
    layer.set_outline_thickness(0.4);
    layer.add_line(Line {
        points: vec![
            (Point::new(Mm(x1), Mm(y)), false),
            (Point::new(Mm(x2), Mm(y)), false),
        ],
        is_closed: false,
    });
}

fn draw_circle(layer: &PdfLayerReference, cx: f32, cy: f32, r: f32, fill: Color) {
    let n = 48u32;
    let pts: Vec<(Point, bool)> = (0..n)
        .map(|i| {
            let a = 2.0 * std::f32::consts::PI * i as f32 / n as f32;
            (Point::new(Mm(cx + r * a.cos()), Mm(cy + r * a.sin())), false)
        })
        .collect();
    layer.set_fill_color(fill);
    layer.add_polygon(Polygon {
        rings: vec![pts],
        mode: PaintMode::Fill,
        winding_order: WindingOrder::NonZero,
    });
}

// Per-page background: corner circle accents + bottom bar mirroring the top chrome
fn draw_page_bg(layer: &PdfLayerReference) {
    // Bottom-right quarter-circle (pale indigo)
    draw_circle(layer, PAGE_W + 8.0, -8.0, 70.0, c(235, 232, 255));
    // Top-left quarter-circle (pale violet), sits behind the top chrome bar
    draw_circle(layer, -8.0, PAGE_H + 8.0, 50.0, c(243, 231, 255));
    // Bottom bar mirroring the top chrome
    fill_rect(layer, 0.0, 0.0, PAGE_W, 2.5, c_primary());
    fill_rect(layer, 0.0, 2.5, PAGE_W, 0.7, c_accent());
}

// Small decorative square + primary-colored separator line
fn section_divider(layer: &PdfLayerReference, y: f32) {
    fill_rect(layer, MARGIN_L, y - 0.8, 4.0, 1.6, c_primary());
    hline(layer, MARGIN_L + 6.0, MARGIN_L + CONTENT_W, y, c_border());
}

// ── EPAM logo ────────────────────────────────────────────────────────────────
// PNG is 1847×650; target width ~30mm → effective DPI ≈ 1565
const EPAM_LOGO_W_MM: f32 = 30.0;
const EPAM_LOGO_H_MM: f32 = EPAM_LOGO_W_MM * 650.0 / 1847.0;
const EPAM_LOGO_DPI: f32 = 1847.0 / (EPAM_LOGO_W_MM / 25.4);

fn draw_epam_logo(layer: &PdfLayerReference, x_right: f32, y_top: f32) {
    let x = x_right - EPAM_LOGO_W_MM;
    let y = y_top - EPAM_LOGO_H_MM;
    let img = image_crate::load_from_memory(EPAM_LOGO_PNG)
        .expect("embedded EPAM logo is valid PNG");
    let pdf_img = Image::from_dynamic_image(&img);
    pdf_img.add_to_layer(
        layer.clone(),
        ImageTransform {
            translate_x: Some(Mm(x)),
            translate_y: Some(Mm(y)),
            dpi: Some(EPAM_LOGO_DPI),
            ..Default::default()
        },
    );
}

// ── Page chrome ───────────────────────────────────────────────────────────────
fn draw_chrome(layer: &PdfLayerReference, _font_bold: &IndirectFontRef) {
    draw_page_bg(layer);
    // Top bar: darker indigo strip + thinner violet accent below it
    fill_rect(layer, 0.0, PAGE_H - 2.5, PAGE_W, 2.5, c_primary());
    fill_rect(layer, 0.0, PAGE_H - 3.2, PAGE_W, 0.7, c_accent());
    draw_epam_logo(layer, PAGE_W - MARGIN_R, PAGE_H - 8.5);
}

// ── Canvas (auto page-break) ──────────────────────────────────────────────────
struct Canvas {
    layer: PdfLayerReference,
    y: f32,
    in_table: bool,
}

fn make_page(doc: &PdfDocumentReference) -> PdfLayerReference {
    let (p, l) = doc.add_page(Mm(PAGE_W), Mm(PAGE_H), "Layer");
    doc.get_page(p).get_layer(l)
}

fn ensure_space(
    canvas: &mut Canvas,
    doc: &PdfDocumentReference,
    font_bold: &IndirectFontRef,
    needed: f32,
) {
    if canvas.y - needed < MARGIN_B {
        canvas.layer = make_page(doc);
        canvas.y = TOP_Y;
        draw_chrome(&canvas.layer, font_bold);
        if canvas.in_table {
            draw_table_header_row(canvas, font_bold);
        }
    }
}

// ── Public entry ──────────────────────────────────────────────────────────────
pub fn to_pdf(data: &ExportData) -> Result<Vec<u8>> {
    let title = format!("Interview Report — {}", data.candidate.name);
    let (doc, first_page, first_layer) =
        PdfDocument::new(&title, Mm(PAGE_W), Mm(PAGE_H), "Layer 1");

    let font = doc
        .add_external_font_with_subsetting(INTER_REGULAR, true)
        .map_err(|e| anyhow::anyhow!("failed to load Inter Regular: {}", e))?;
    let font_bold = doc
        .add_external_font_with_subsetting(INTER_BOLD, true)
        .map_err(|e| anyhow::anyhow!("failed to load Inter Bold: {}", e))?;

    // Page 1: summary
    let layer = doc.get_page(first_page).get_layer(first_layer);
    draw_chrome(&layer, &font_bold);
    draw_summary(&layer, data, &font, &font_bold);

    // Page 2+: single flowing questions table
    let mut canvas = Canvas {
        layer: make_page(&doc),
        y: TOP_Y,
        in_table: false,
    };
    draw_chrome(&canvas.layer, &font_bold);
    draw_combined_questions(&mut canvas, &doc, data, &font, &font_bold);

    let bytes = doc
        .save_to_bytes()
        .map_err(|e| anyhow::anyhow!("failed to serialize PDF: {}", e))?;
    Ok(bytes)
}

// ── Summary page ──────────────────────────────────────────────────────────────
fn draw_summary(
    layer: &PdfLayerReference,
    data: &ExportData,
    font: &IndirectFontRef,
    font_bold: &IndirectFontRef,
) {
    let mut y = PAGE_H - 32.0;

    // Eyebrow
    layer.set_fill_color(c_soft());
    put_text(layer, "INTERVIEW REPORT", 9.0, MARGIN_L, y, font_bold);
    y -= 13.0;

    // Hero: candidate name (big, dark, tight)
    layer.set_fill_color(c_dark());
    put_text(layer, &data.candidate.name.to_uppercase(), 32.0, MARGIN_L, y, font_bold);
    y -= 10.0;

    // Role with accent dot
    fill_rect(layer, MARGIN_L, y - 0.5, 2.5, 2.5, c_accent());
    layer.set_fill_color(c_primary());
    put_text(layer, &title_case(&data.candidate.role), 16.0, MARGIN_L + 5.5, y, font_bold);
    y -= 7.5;

    // Interviewer + date
    layer.set_fill_color(c_muted());
    let date = data.candidate.created_at.format("%B %d, %Y").to_string();
    let meta = format!(
        "Interviewed by {}  \u{00B7}  {}",
        interviewer_name(),
        date
    );
    put_text(layer, &meta, 10.0, MARGIN_L, y, font);
    y -= 7.0;

    section_divider(layer, y);
    y -= 11.0;

    // Stat cards
    y = draw_stat_cards(layer, data, y, font_bold);
    y -= 13.0;

    // Top 3 topics
    y = draw_top_performers(layer, data, y, font_bold);

    // All topics progress
    draw_topic_performance(layer, data, y, font, font_bold);

    // Footer
    layer.set_fill_color(c_soft());
    put_text(
        layer,
        &format!("Candidate ID: {}", data.candidate.id),
        7.5, MARGIN_L, MARGIN_B - 5.0, font,
    );
    let g = format!("Generated {}", chrono::Utc::now().format("%Y-%m-%d %H:%M UTC"));
    let gw = text_width_mm(&g, 7.5, false);
    put_text(layer, &g, 7.5, PAGE_W - MARGIN_R - gw, MARGIN_B - 5.0, font);
}

fn draw_stat_cards(
    layer: &PdfLayerReference,
    data: &ExportData,
    top_y: f32,
    font_bold: &IndirectFontRef,
) -> f32 {
    let total_score: u32 = data.topics.iter().map(|t| t.score(data.responses)).sum();
    let total_max: u32 = data.topics.iter().map(|t| t.max_score()).sum();
    let total_answered: usize = data.topics.iter().map(|t| t.answered(data.responses)).sum();
    let total_questions: usize = data.topics.iter().map(|t| t.questions.len()).sum();
    let pct = if total_max > 0 { total_score * 100 / total_max } else { 0 };

    let card_w = (CONTENT_W - 12.0) / 3.0;
    let card_h: f32 = 34.0;
    let y = top_y - card_h;

    let cards: [(&str, String); 3] = [
        ("TOTAL SCORE", format!("{}/{}", total_score, total_max)),
        ("PERCENTAGE", format!("{}%", pct)),
        ("ANSWERED", format!("{}/{}", total_answered, total_questions)),
    ];
    for (i, (lbl, val)) in cards.iter().enumerate() {
        let x = MARGIN_L + i as f32 * (card_w + 6.0);
        // Card background
        fill_rect(layer, x, y, card_w, card_h, c_panel());
        // Two-tone accent stripe at top (gradient feel)
        fill_rect(layer, x, y + card_h - 2.0, card_w, 2.0, c_primary());
        fill_rect(layer, x, y + card_h - 2.6, card_w, 0.6, c_accent());
        // Label (small caps-style)
        layer.set_fill_color(c_muted());
        put_text(layer, lbl, 8.5, x + 7.0, y + card_h - 10.0, font_bold);
        // Big value
        layer.set_fill_color(c_dark());
        put_text(layer, val, 24.0, x + 7.0, y + 7.5, font_bold);
    }
    y
}

fn draw_top_performers(
    layer: &PdfLayerReference,
    data: &ExportData,
    mut top_y: f32,
    font_bold: &IndirectFontRef,
) -> f32 {
    let mut ranked: Vec<(&TopicData, u32, usize)> = data
        .topics
        .iter()
        .map(|t| {
            let mx = t.max_score();
            let sc = t.score(data.responses);
            let ans = t.answered(data.responses);
            let pct = if mx > 0 { sc * 100 / mx } else { 0 };
            (t, pct, ans)
        })
        .filter(|(_, _, ans)| *ans > 0)
        .collect();
    ranked.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.name.cmp(&b.0.name)));
    let top: Vec<_> = ranked.into_iter().take(3).collect();

    if top.is_empty() {
        return top_y;
    }

    // Section heading
    layer.set_fill_color(c_dark());
    put_text(layer, "TOP 3 TOPICS", 10.5, MARGIN_L, top_y, font_bold);
    top_y -= 6.5;

    let row_h: f32 = 11.5;
    let pad: f32 = 5.5;
    let card_h = top.len() as f32 * row_h + 2.0 * pad;
    let card_y = top_y - card_h;

    fill_rect(layer, MARGIN_L, card_y, CONTENT_W, card_h, c_panel());
    fill_rect(layer, MARGIN_L, card_y, 3.5, card_h, c_primary());

    let medal_colors = [c_gold(), c_silver(), c_bronze()];
    let mut ry = top_y - pad;
    for (i, (t, pct, _)) in top.iter().enumerate() {
        let mw: f32 = 8.0;
        let mh: f32 = 8.0;
        let mx0 = MARGIN_L + 9.0;
        let my = ry - row_h + 2.0;

        // Medal with rank
        fill_rect(layer, mx0, my, mw, mh, medal_colors[i].clone());
        layer.set_fill_color(c_white());
        let rank = format!("{}", i + 1);
        let rw = text_width_mm(&rank, 11.0, true);
        put_text(layer, &rank, 11.0, mx0 + (mw - rw) / 2.0, my + 1.8, font_bold);

        // Topic name
        layer.set_fill_color(c_dark());
        put_text(layer, &title_case(&t.name), 13.0, mx0 + mw + 6.0, my + 2.2, font_bold);

        // Percentage right-aligned
        let p_txt = format!("{}%", pct);
        let pw = text_width_mm(&p_txt, 16.0, true);
        layer.set_fill_color(c_primary());
        put_text(
            layer,
            &p_txt,
            16.0,
            MARGIN_L + CONTENT_W - pw - 8.0,
            my + 1.4,
            font_bold,
        );

        ry -= row_h;
    }
    card_y - 12.0
}

fn draw_topic_performance(
    layer: &PdfLayerReference,
    data: &ExportData,
    mut y: f32,
    font: &IndirectFontRef,
    font_bold: &IndirectFontRef,
) {
    layer.set_fill_color(c_dark());
    put_text(layer, "ALL TOPICS", 10.5, MARGIN_L, y, font_bold);
    y -= 3.0;
    hline(layer, MARGIN_L, MARGIN_L + CONTENT_W, y, c_border());
    y -= 7.5;

    for t in data.topics {
        let sc = t.score(data.responses);
        let mx = t.max_score();
        let ans = t.answered(data.responses);
        let tot = t.questions.len();
        let tp = if mx > 0 { sc * 100 / mx } else { 0 };

        layer.set_fill_color(c_dark());
        put_text(layer, &title_case(&t.name), 10.5, MARGIN_L, y, font_bold);

        let stats = format!(
            "{}/{} q  \u{00B7}  {}/{} pts  \u{00B7}  {}%",
            ans, tot, sc, mx, tp
        );
        let sw = text_width_mm(&stats, 9.5, false);
        layer.set_fill_color(c_muted());
        put_text(layer, &stats, 9.5, MARGIN_L + CONTENT_W - sw, y, font);
        y -= 3.5;

        let bar_h: f32 = 2.5;
        let bar_y = y - bar_h;
        fill_rect(layer, MARGIN_L, bar_y, CONTENT_W, bar_h, c_panel2());
        if ans > 0 && tp > 0 {
            let fw = CONTENT_W * (tp as f32 / 100.0);
            fill_rect(layer, MARGIN_L, bar_y, fw, bar_h, perf_color(tp));
        }
        y -= 8.5;
    }
}

fn perf_color(pct: u32) -> Color {
    if pct >= 75 { c_success() }
    else if pct >= 50 { c_primary() }
    else if pct >= 25 { c_warning() }
    else { c_danger() }
}

// ── Combined questions breakdown ─────────────────────────────────────────────
fn draw_combined_questions(
    canvas: &mut Canvas,
    doc: &PdfDocumentReference,
    data: &ExportData,
    font: &IndirectFontRef,
    font_bold: &IndirectFontRef,
) {
    // Eyebrow label
    canvas.layer.set_fill_color(c_soft());
    put_text(&canvas.layer, "DETAILED BREAKDOWN", 9.0, MARGIN_L, canvas.y, font_bold);
    canvas.y -= 11.0;

    // Title
    canvas.layer.set_fill_color(c_dark());
    put_text(&canvas.layer, "Questions by Topic", 24.0, MARGIN_L, canvas.y, font_bold);
    canvas.y -= 8.5;

    // Candidate / role reference line
    canvas.layer.set_fill_color(c_muted());
    let ref_line = format!(
        "{}  \u{00B7}  {}",
        data.candidate.name,
        title_case(&data.candidate.role)
    );
    put_text(&canvas.layer, &ref_line, 10.0, MARGIN_L, canvas.y, font);
    canvas.y -= 5.0;

    section_divider(&canvas.layer, canvas.y);
    canvas.y -= 10.0;

    // Enter table mode; header will auto-repeat on page breaks
    canvas.in_table = true;
    draw_table_header_row(canvas, font_bold);

    for topic in data.topics {
        draw_topic_divider(canvas, doc, topic, data, font, font_bold);

        let answered: Vec<&Question> = topic
            .questions
            .iter()
            .filter(|q| data.responses.contains_key(&q.id))
            .collect();

        if answered.is_empty() {
            draw_not_discussed_row(canvas, doc, font_bold);
            continue;
        }

        for level in 1..=4u8 {
            let level_qs: Vec<&Question> = answered
                .iter()
                .copied()
                .filter(|q| q.level == level)
                .collect();
            if level_qs.is_empty() {
                continue;
            }
            draw_level_divider(canvas, doc, level, level_qs.len(), font, font_bold);
            for (i, q) in level_qs.iter().enumerate() {
                draw_question_row(canvas, doc, q, data, i % 2 == 1, font, font_bold);
            }
        }
    }
    canvas.in_table = false;
}

fn draw_table_header_row(canvas: &mut Canvas, font_bold: &IndirectFontRef) {
    let (cq, cr, _) = cols();
    let h: f32 = 6.0;
    let hy = canvas.y - h;
    fill_rect(&canvas.layer, MARGIN_L, hy, CONTENT_W, h, c_panel2());
    canvas.layer.set_fill_color(c_muted());
    put_text(&canvas.layer, "QUESTION", 7.5, MARGIN_L + 3.5, hy + 1.8, font_bold);
    put_text(&canvas.layer, "RESPONSE", 7.5, MARGIN_L + cq + 3.5, hy + 1.8, font_bold);
    put_text(&canvas.layer, "POINTS", 7.5, MARGIN_L + cq + cr + 3.5, hy + 1.8, font_bold);
    canvas.y -= h;
}

fn draw_topic_divider(
    canvas: &mut Canvas,
    doc: &PdfDocumentReference,
    topic: &TopicData,
    data: &ExportData,
    font: &IndirectFontRef,
    font_bold: &IndirectFontRef,
) {
    let h: f32 = 10.5;
    ensure_space(canvas, doc, font_bold, h + 12.0);
    let ry = canvas.y - h;

    // Subtle two-tone card: wide indigo bar + darker right segment for emphasis
    fill_rect(&canvas.layer, MARGIN_L, ry, CONTENT_W, h, c_primary_d());
    fill_rect(&canvas.layer, MARGIN_L, ry, 5.0, h, c_accent());

    // Topic name
    canvas.layer.set_fill_color(c_white());
    put_text(
        &canvas.layer,
        &title_case(&topic.name),
        13.0,
        MARGIN_L + 8.5,
        ry + 3.3,
        font_bold,
    );

    // Stats on right
    let sc = topic.score(data.responses);
    let mx = topic.max_score();
    let ans = topic.answered(data.responses);
    let tot = topic.questions.len();
    let pct = if mx > 0 { sc * 100 / mx } else { 0 };
    let stats = format!(
        "{}/{} q  \u{00B7}  {}/{} pts  \u{00B7}  {}%",
        ans, tot, sc, mx, pct
    );
    let sw = text_width_mm(&stats, 9.0, false);
    put_text(
        &canvas.layer,
        &stats,
        9.0,
        MARGIN_L + CONTENT_W - sw - 6.0,
        ry + 3.5,
        font,
    );

    canvas.y -= h;
}

fn draw_level_divider(
    canvas: &mut Canvas,
    doc: &PdfDocumentReference,
    level: u8,
    count: usize,
    font: &IndirectFontRef,
    font_bold: &IndirectFontRef,
) {
    let h: f32 = 7.0;
    ensure_space(canvas, doc, font_bold, h + 12.0);
    let ry = canvas.y - h;

    fill_rect(&canvas.layer, MARGIN_L, ry, CONTENT_W, h, c_panel2());
    fill_rect(&canvas.layer, MARGIN_L, ry, 3.0, h, level_color(level));

    canvas.layer.set_fill_color(c_dark());
    let upper = level_name(level).to_uppercase();
    put_text(&canvas.layer, &upper, 10.5, MARGIN_L + 7.0, ry + 2.0, font_bold);

    let tag = format!(
        "LEVEL {}  \u{00B7}  {} question{}",
        level,
        count,
        if count == 1 { "" } else { "s" }
    );
    let tag_x = MARGIN_L + 7.0 + text_width_mm(&upper, 10.5, true) + 5.0;
    canvas.layer.set_fill_color(c_muted());
    put_text(&canvas.layer, &tag, 8.0, tag_x, ry + 2.0, font);

    canvas.y -= h;
}

fn draw_not_discussed_row(
    canvas: &mut Canvas,
    doc: &PdfDocumentReference,
    font_bold: &IndirectFontRef,
) {
    let h: f32 = 9.0;
    ensure_space(canvas, doc, font_bold, h + 2.0);
    let ry = canvas.y - h;
    fill_rect(&canvas.layer, MARGIN_L, ry, CONTENT_W, h, c_panel());
    hline(&canvas.layer, MARGIN_L, MARGIN_L + CONTENT_W, ry, c_border());

    // Dimmed left marker to tell it apart from a level row
    fill_rect(&canvas.layer, MARGIN_L, ry, 3.0, h, c_soft());
    canvas.layer.set_fill_color(c_muted());
    put_text(
        &canvas.layer,
        "Topic not discussed",
        10.0,
        MARGIN_L + 7.0,
        ry + 2.8,
        font_bold,
    );
    canvas.y -= h;
}

fn draw_question_row(
    canvas: &mut Canvas,
    doc: &PdfDocumentReference,
    q: &Question,
    data: &ExportData,
    zebra: bool,
    font: &IndirectFontRef,
    font_bold: &IndirectFontRef,
) {
    let (cq, cr, _) = cols();
    let q_size: f32 = 9.5;
    let line_h: f32 = 4.4;
    let row_pad: f32 = 3.2;
    let max_points = 4 * q.level as u32;
    let score = data.responses.get(&q.id).copied().unwrap_or(0);
    let q_lines = wrap_text(&q.text, cq - 7.0, q_size, false);
    let row_h = q_lines.len() as f32 * line_h + 2.0 * row_pad;

    ensure_space(canvas, doc, font_bold, row_h);
    let ry = canvas.y - row_h;

    if zebra {
        fill_rect(&canvas.layer, MARGIN_L, ry, CONTENT_W, row_h, c_panel());
    }
    hline(&canvas.layer, MARGIN_L, MARGIN_L + CONTENT_W, ry, c_border());

    // Question text
    canvas.layer.set_fill_color(c_text());
    let mut ty = canvas.y - row_pad - 3.2;
    for line in &q_lines {
        put_text(&canvas.layer, line, q_size, MARGIN_L + 3.5, ty, font);
        ty -= line_h;
    }

    // Response pill — subtle rounded look via wider horizontal padding
    let (label, rcolor) = response_label(score);
    let lbl_w = text_width_mm(label, 9.0, true) + 5.0;
    let lbl_h: f32 = 5.4;
    let bx = MARGIN_L + cq + 3.5;
    let by = canvas.y - row_pad - 4.8;
    fill_rect(&canvas.layer, bx, by, lbl_w, lbl_h, rcolor);
    canvas.layer.set_fill_color(c_white());
    put_text(&canvas.layer, label, 9.0, bx + 2.5, by + 1.3, font_bold);

    // Points
    let earned = score as u32 * q.level as u32;
    let p_txt = format!("{}/{}", earned, max_points);
    canvas.layer.set_fill_color(c_dark());
    put_text(
        &canvas.layer,
        &p_txt,
        10.5,
        MARGIN_L + cq + cr + 3.5,
        canvas.y - row_pad - 3.2,
        font_bold,
    );

    canvas.y -= row_h;
}

fn level_name(level: u8) -> &'static str {
    match level {
        1 => "Basic",
        2 => "Intermediate",
        3 => "Advanced",
        4 => "Expert",
        _ => "—",
    }
}

fn level_color(level: u8) -> Color {
    match level {
        1 => c_warning(),
        2 => c_primary(),
        3 => c_success(),
        4 => c(22, 101, 52),
        _ => c_muted(),
    }
}

fn response_label(score: u8) -> (&'static str, Color) {
    match score {
        0 => ("Could not answer", c_danger()),
        1 => ("Basic", c_warning()),
        2 => ("Intermediate", c_primary()),
        3 => ("Advanced", c_success()),
        4 => ("Expert", c(22, 101, 52)),
        _ => ("—", c_muted()),
    }
}
