//! 可选的「把 AI 标识烧录进图片」（ROADMAP 第四批 ⑥ 附加项 · 主动合规）
//!
//! ## 这个模块做的是**加**标识，与"去水印"完全相反
//!
//! 《人工智能生成合成内容标识办法》要求 AI 生成内容带**显式标识**与**隐式标识**。本模块：
//!
//! - **显式**：在图右下角画半透明「AI」角标（像素级，转发出去也带着）
//! - **隐式**：在 PNG 的 `tEXt` 块写入 `AIGenerated=true` 等元数据（文件层面可识别）
//!
//! 法规禁止的是**删除、篡改、隐匿**标识；本模块只做加法，软件里**不存在**任何"去标识"功能。
//!
//! ## 实现选择（记一次真实的工程判断）
//!
//! 最初手写了 PNG 编解码以避免依赖，但真机验证时**素材库里 3 张真实 PNG 全部解不动** ——
//! 它们用 dynamic Huffman 压缩，而手写实现只覆盖到 stored 块。同时发现项目**早已直接依赖
//! `image = "0.25"`（含 png feature）**（立绘尺寸探测就在用它）。于是改用成熟库：
//! `image` 负责像素解码/编码，`png` 负责读回与写回**全部文本块**（保住服务商写入的隐式标识）。
//!
//! ## 失败处理
//!
//! 任何失败都返回可读原因，调用方**降级为按原图入库**（绝不破坏原图、绝不让生成整体失败）。

use std::io::Cursor;

/// 角标文案
const LABEL: &str = "AI";

/// 是否为 PNG（按魔数判断）
pub fn is_png(bytes: &[u8]) -> bool {
    bytes.len() > 8 && bytes[..8] == [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]
}

/// 点阵字形的宽高（8 宽 11 高，刚好能画出清晰的「AI」）
const GLYPH_W: u32 = 8;
const GLYPH_H: u32 = 11;

/// 8×11 点阵字形：每行一个字节，**最高位是最左边的像素**（MSB-first）。
///
/// 说明：第一版用了 4×7 且位序写反，真机核对时角标渲染成了**镜像**（「A」左右反转、「I」横躺）——
/// 合规标识若不可辨认就失去意义，因此改为 8×11 并把位序在注释里写死，避免再犯。
fn glyph(ch: char) -> [u8; GLYPH_H as usize] {
    match ch {
        //  A  ............   每行 bit7..bit0 对应从左到右
        'A' => [
            0b00011000,
            0b00111100,
            0b01100110,
            0b01100110,
            0b11000011,
            0b11000011,
            0b11111111,
            0b11000011,
            0b11000011,
            0b11000011,
            0b00000000,
        ],
        //  I
        'I' => [
            0b11111111,
            0b00011000,
            0b00011000,
            0b00011000,
            0b00011000,
            0b00011000,
            0b00011000,
            0b00011000,
            0b00011000,
            0b11111111,
            0b00000000,
        ],
        _ => [0; GLYPH_H as usize],
    }
}

/// 在 RGBA 缓冲的右下角画角标；返回 (x, y, 宽, 高)
fn draw_label(rgba: &mut [u8], w: u32, h: u32) -> (u32, u32, u32, u32) {
    let n = LABEL.chars().count() as u32;
    // 自适应：先按图片宽度取一档，再逐步收缩到"含边距后仍塞得进图片"为止
    // （小图必须也能画 —— 否则角标越界 = 白画）
    let mut scale: u32 = if w >= 512 { 4 } else if w >= 256 { 3 } else { 2 };
    let fits = |s: u32| -> bool {
        let cw = GLYPH_W * s;
        let ch = GLYPH_H * s;
        let pad = 2 * s;
        let text_w = cw * n + s * (n - 1);
        (text_w + pad * 2 + 5 * s) <= w && (ch + pad * 2 + 5 * s) <= h
    };
    while scale > 1 && !fits(scale) {
        scale -= 1;
    }
    if !fits(scale) {
        // 图太小（连最小角标都塞不下）：不画，返回空框让调用方跳过烧录。
        // 宁可这张图没有显式角标，也不要画出越界的一半角标。
        return (0, 0, 0, 0);
    }
    let cw = GLYPH_W * scale;
    let ch = GLYPH_H * scale;
    let gap = scale;
    let pad = 2 * scale;
    let text_w = cw * n + gap * (n - 1);
    let box_w = text_w + pad * 2;
    let box_h = ch + pad * 2;
    let margin = 5 * scale;
    let x0 = w.saturating_sub(box_w + margin);
    let y0 = h.saturating_sub(box_h + margin);

    let mut put = |x: u32, y: u32, rgb: [u8; 3], a: f32| {
        if x >= w || y >= h {
            return;
        }
        let off = ((y * w + x) * 4) as usize;
        for k in 0..3 {
            let old = rgba[off + k] as f32;
            rgba[off + k] = (old * (1.0 - a) + rgb[k] as f32 * a).round().clamp(0.0, 255.0) as u8;
        }
        rgba[off + 3] = 255; // 角标区域不透明，避免与图自身透明度叠加后看不见
    };

    for y in y0..(y0 + box_h).min(h) {
        for x in x0..(x0 + box_w).min(w) {
            put(x, y, [10, 10, 12], 0.55);
        }
    }
    let mut cx = x0 + pad;
    for c in LABEL.chars() {
        let g = glyph(c);
        for (ry, row) in g.iter().enumerate() {
            for rx in 0..GLYPH_W {
                // MSB 是最左像素
                if row & (1 << (GLYPH_W - 1 - rx)) != 0 {
                    for sy in 0..scale {
                        for sx in 0..scale {
                            put(cx + rx * scale + sx, y0 + pad + ry as u32 * scale + sy, [255, 210, 125], 0.95);
                        }
                    }
                }
            }
        }
        cx += cw + gap;
    }
    (x0, y0, box_w, box_h)
}

/// 读出 PNG 里已有的文本块（**保住服务商写入的隐式标识** —— 加标识时不能反而丢掉别人的标识）
fn read_text_chunks(bytes: &[u8]) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let dec = png::Decoder::new(Cursor::new(bytes));
    if let Ok(rd) = dec.read_info() {
        // png 0.18：文本块是结构体（0.17 才是元组），字段为 keyword / text
        for c in rd.info().uncompressed_latin1_text.iter() {
            out.push((c.keyword.clone(), c.text.clone()));
        }
        for c in rd.info().utf8_text.iter() {
            out.push((c.keyword.clone(), c.get_text().unwrap_or_default().to_string()));
        }
    }
    out
}

/// 给一张 PNG 加「AI 生成」显式角标 + 隐式元数据标识，返回新字节
pub fn stamp_ai_label(png_bytes: &[u8]) -> Result<Vec<u8>, String> {
    if !is_png(png_bytes) {
        return Err("不是 PNG 文件".into());
    }
    let existing = read_text_chunks(png_bytes);
    let existing_keys: Vec<String> = existing.iter().map(|(k, _)| k.clone()).collect();

    let img = image::load_from_memory_with_format(png_bytes, image::ImageFormat::Png)
        .map_err(|e| format!("解码失败：{e}"))?;
    let (w, h) = (img.width(), img.height());
    if w == 0 || h == 0 {
        return Err("图片尺寸异常".into());
    }
    let mut rgba = img.to_rgba8().into_raw();

    let (lx, ly, lw, lh) = draw_label(&mut rgba, w, h);

    let mut out: Vec<u8> = Vec::new();
    {
        let mut enc = png::Encoder::new(Cursor::new(&mut out), w, h);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        for (k, v) in &existing {
            enc.add_text_chunk(k.clone(), v.clone()).map_err(|e| e.to_string())?;
        }
        let ours = [
            ("AIGenerated", "true"),
            ("AIGeneratedSoftware", "Story-Weaver"),
            (
                "AIGeneratedNote",
                "AI-generated content. Per the Measures for Labeling AI-Generated Synthetic Content (CN, effective 2025-09-01), this label must not be removed or altered.",
            ),
        ];
        for (k, v) in ours {
            if existing_keys.iter().any(|x| x == k) {
                continue; // 已存在就不重复堆叠
            }
            enc.add_text_chunk(k.to_string(), v.to_string()).map_err(|e| e.to_string())?;
        }
        let mut writer = enc.write_header().map_err(|e| e.to_string())?;
        writer.write_image_data(&rgba).map_err(|e| e.to_string())?;
    }

    // 自检：编出来的必须能回读、尺寸一致、角标确实落盘
    let back = image::load_from_memory_with_format(&out, image::ImageFormat::Png)
        .map_err(|e| format!("编码自检失败（无法回读）：{e}"))?;
    if back.width() != w || back.height() != h {
        return Err("编码自检失败（尺寸不一致）".into());
    }
    let back_rgba = back.to_rgba8();
    let px = back_rgba.get_pixel((lx + lw / 2).min(w - 1), (ly + lh / 2).min(h - 1));
    if px[3] != 255 {
        return Err("编码自检失败（角标未落盘）".into());
    }
    Ok(out)
}

/// 写盘助手（供命令层使用）
pub fn write_stamped(dir: &std::path::Path, bytes: &[u8], idx: usize) -> Result<String, String> {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|x| x.as_millis())
        .unwrap_or(0);
    let p = dir.join(format!("stamped-{stamp}-{idx}.png"));
    std::fs::write(&p, bytes).map_err(|e| format!("写文件失败：{e}"))?;
    Ok(p.to_string_lossy().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 造一张测试 PNG（可带一个文本块，模拟服务商写入的隐式标识）
    fn make_png(w: u32, h: u32, with_vendor_text: bool) -> Vec<u8> {
        let mut rgba = vec![0u8; (w * h * 4) as usize];
        for (i, px) in rgba.chunks_mut(4).enumerate() {
            px[0] = (i % 200) as u8;
            px[1] = 80;
            px[2] = 160;
            px[3] = 255;
        }
        let mut out: Vec<u8> = Vec::new();
        {
            let mut enc = png::Encoder::new(Cursor::new(&mut out), w, h);
            enc.set_color(png::ColorType::Rgba);
            enc.set_depth(png::BitDepth::Eight);
            if with_vendor_text {
                enc.add_text_chunk("VendorMark".to_string(), "made-by-vendor".to_string()).unwrap();
            }
            let mut wr = enc.write_header().unwrap();
            wr.write_image_data(&rgba).unwrap();
        }
        out
    }

    #[test]
    fn png_magic_is_recognized() {
        assert!(is_png(&make_png(4, 4, false)));
        assert!(!is_png(b"not a png"));
    }

    #[test]
    fn stamp_adds_visible_label_and_metadata() {
        let png = make_png(120, 90, false);
        let before = image::load_from_memory(&png).unwrap().to_rgba8();
        let stamped = stamp_ai_label(&png).expect("应能烧录");
        let after = image::load_from_memory(&stamped).unwrap().to_rgba8();
        assert_eq!(after.width(), before.width());
        assert_eq!(after.height(), before.height());
        assert_ne!(after.as_raw(), before.as_raw(), "角标应改变像素");

        let text = String::from_utf8_lossy(&stamped);
        assert!(text.contains("AIGenerated"), "应写入隐式标识关键词");
        assert!(text.contains("Story-Weaver"), "应写明来源软件");
        assert!(text.contains("2025-09-01"), "应写明标识义务依据");
    }

    #[test]
    fn vendor_metadata_is_preserved_not_stripped() {
        // 合规关键：我们**加**标识，绝不能顺手把服务商的标识删掉
        let png = make_png(60, 60, true);
        assert!(String::from_utf8_lossy(&png).contains("VendorMark"));
        let stamped = stamp_ai_label(&png).expect("应能烧录");
        let text = String::from_utf8_lossy(&stamped);
        assert!(text.contains("VendorMark"), "服务商原有的隐式标识必须保留");
        assert!(text.contains("made-by-vendor"));
        assert!(text.contains("AIGenerated"), "同时加上我们的标识");
    }

    #[test]
    fn label_lands_in_bottom_right_and_inset() {
        let (w, h) = (400u32, 300u32);
        let mut rgba = vec![255u8; (w * h * 4) as usize];
        let (x0, y0, bw, bh) = draw_label(&mut rgba, w, h);
        assert!(x0 + bw <= w, "角标不应越界（右）");
        assert!(y0 + bh <= h, "角标不应越界（下）");
        assert!(x0 > w / 2 && y0 > h / 2, "应位于右下区域");
        // 图太小：应返回空框（调用方据此跳过烧录），而不是画一半越界
        let (sw, sh) = (12u32, 9u32);
        let mut small = vec![0u8; (sw * sh * 4) as usize];
        let (sx, sy, sbw, sbh) = draw_label(&mut small, sw, sh);
        assert_eq!((sbw, sbh), (0, 0), "太小就应返回空框");
        assert!(sx + sbw <= sw && sy + sbh <= sh);

        // 刚好够的最小尺寸：仍应画出来且不越界
        let (mw, mh) = (64u32, 40u32);
        let mut mid = vec![0u8; (mw * mh * 4) as usize];
        let (mx, my, mbw, mbh) = draw_label(&mut mid, mw, mh);
        assert!(mbw > 0 && mbh > 0, "64x40 应该能画下最小角标");
        assert!(mx + mbw <= mw && my + mbh <= mh, "且不越界");
    }

    #[test]
    fn repeated_stamping_does_not_stack_duplicates() {
        let png = make_png(40, 40, false);
        let once = stamp_ai_label(&png).unwrap();
        let twice = stamp_ai_label(&once).unwrap();
        // 只在**文本块**里数关键词（整字节搜索会把像素数据里的偶然匹配也算进去）
        let keys: Vec<String> = read_text_chunks(&twice).into_iter().map(|(k, _)| k).collect();
        let n = keys.iter().filter(|k| k.starts_with("AIGenerated")).count();
        assert_eq!(n, 3, "我们的三个标识键各写一次，不应重复堆叠（实际 {n} 个：{keys:?}）");
        // 再烧一次也不增加
        let thrice = stamp_ai_label(&twice).unwrap();
        let keys3: Vec<String> = read_text_chunks(&thrice).into_iter().map(|(k, _)| k).collect();
        assert_eq!(keys3.len(), keys.len(), "重复烧录不应增加文本块");
    }

    #[test]
    fn non_png_is_rejected_with_reason() {
        assert!(stamp_ai_label(b"plain text").unwrap_err().contains("不是 PNG"));
    }

    /// 真机验证（默认跳过）：拿素材库里的真实图片试烧录。
    /// 跑法：`cargo test --lib live_stamp -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn live_stamp_real_library_images() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..").join("library").join("default").join("shared").join("portraits");
        let Ok(rd) = std::fs::read_dir(&dir) else {
            println!("素材库目录不存在，跳过：{}", dir.display());
            return;
        };
        let (mut ok, mut fail) = (0usize, 0usize);
        for e in rd.flatten().take(12) {
            let path = e.path();
            if path.extension().map(|x| x != "png").unwrap_or(true) {
                continue;
            }
            let bytes = std::fs::read(&path).unwrap_or_default();
            let name = path.file_name().unwrap_or_default().to_string_lossy().to_string();
            match stamp_ai_label(&bytes) {
                Ok(stamped) => {
                    ok += 1;
                    let img = image::load_from_memory(&stamped).unwrap();
                    // 落一份到临时目录便于人工核对角标外观
                    if let Ok(p) = write_stamped(
                        &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target").join("gen-tmp"),
                        &stamped,
                        ok,
                    ) {
                        println!("      已写出：{p}");
                    }
                    println!("  OK {name}  {}x{}  {} KB -> 烧录后 {} KB（角标+隐式标识）",
                        img.width(), img.height(), bytes.len() / 1024, stamped.len() / 1024);
                }
                Err(e) => { fail += 1; println!("  FAIL {name}  {e}"); }
            }
        }
        println!("\n  结果：成功 {ok} 张 / 失败 {fail} 张");
    }
}
