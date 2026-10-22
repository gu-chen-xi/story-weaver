// 从 `export.rs` 拆出的测试模块（第一波「只搬测试」，逻辑未改）。
//
// 模块路径仍是 `export::tests` —— 因为 `export.rs` + `export/` 目录是 Rust 标准布局，
// 所以本文件里的 `use super::*` 依然指向 `export`，一行都不用改写。

use super::*;

fn sample() -> StoryExport {
    StoryExport {
        title: "测试故事".into(),
        time_origin: "2026-01-01".into(),
        exported_at: "2026-10-18 15:00:00".into(),
        day_count: 1,
        nodes: vec![NodeExport {
            id: 1,
            date_label: "第 1 天".into(),
            node_type: "day".into(),
            is_flashback: false,
            msgs: vec![MsgExport {
                role: "assistant".into(),
                content: "[女主]\u{201c}你回来了。\u{201d}\n（她抬眼看他）".into(),
                mode: "chat".into(),
            }],
        }],
    }
}

/// 三种导出格式**都必须**带「AI 生成」声明。
///
/// 为什么值得单测：导出的文件会离开本软件（被转发、上传、投稿），标识必须跟着文件走 ——
/// 若将来重构渲染函数时漏掉结尾声明，这条测试会立刻变红。
#[test]
fn all_export_formats_carry_ai_notice() {
    let e = sample();
    let tx = render_txt(&e, "全部");
    let md = render_md(&e, "全部");
    let html = render_html(&e, "全部");

    for (name, body) in [("txt", &tx), ("md", &md), ("html", &html)] {
        assert!(body.contains("AI 生成"), "{name} 应含「AI 生成」字样");
        assert!(
            body.contains("人工智能生成合成内容"),
            "{name} 应点明属人工智能生成合成内容"
        );
        assert!(
            body.contains("请勿删除本声明") || body.contains("请保留本声明"),
            "{name} 应要求保留声明"
        );
        // 声明必须在**末尾**：取**最后一次**出现的位置来判断。
        // 注意 unwrap_or(0)：早期版本写成 unwrap_or(0) 会把"根本没找到"静默成位置 0，
        // 于是断言只是"没找到"而非"位置不对"，报错信息也误导。这里显式要求必须找到。
        let notice_at = body.rfind("AI 生成内容声明").unwrap_or_else(|| {
            panic!("{name} 里找不到「AI 生成内容声明」字样（rfind 未命中）")
        });
        // 更贴合意图的判断：声明之后**不应再有正文**（只允许空白与 HTML 收尾标签）
        let tail = &body[notice_at..];
        let tail_content: String = tail
            .lines()
            .filter(|l| {
                let l = l.trim();
                !l.is_empty() && !l.starts_with("</") && !l.starts_with("<footer")
            })
            .collect::<Vec<_>>()
            .join("\n");
        // 声明自身的文字（标题 + 两行说明）之外，尾部不该再有别的行
        assert!(
            tail_content.lines().count() <= 5,
            "{name} 的声明之后还有正文内容（{} 行）：{tail_content}",
            tail_content.lines().count()
        );
    }
    // 剧情内容本身仍在
    assert!(tx.contains("你回来了"), "正文不应被声明覆盖");
}

/// 空内容也要带声明（"暂无剧情内容"的情形不能被漏掉）
/// 导出时间戳必须是标准口径 `YYYY-MM-DD HH:MM:SS`
/// （旧实现用 SystemTime + 8h 手工拼，非 +8 时区会错；现在交给 SQLite localtime）
#[test]
fn exported_at_uses_standard_stamp_format() {
    let e = sample();
    assert!(
        regex_like_stamp(&e.exported_at),
        "导出时间不是标准格式：{}", e.exported_at
    );
    // 三种格式都能带上它
    for body in [render_txt(&e, "全部"), render_md(&e, "全部"), render_html(&e, "全部")] {
        assert!(body.contains("2026-10-18 15:00:00"), "导出内容应带上导出时间");
    }
}

/// `YYYY-MM-DD HH:MM:SS` 的手工校验（不引 regex 依赖）
fn regex_like_stamp(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 19
        && b[4] == b'-' && b[7] == b'-' && b[10] == b' '
        && b[13] == b':' && b[16] == b':'
        && b.iter().enumerate().all(|(i, c)| {
            matches!(i, 4 | 7 | 10 | 13 | 16) || c.is_ascii_digit()
        })
}

#[test]
fn empty_export_still_carries_notice() {
    let e = StoryExport {
        title: "空故事".into(),
        time_origin: "2026-01-01".into(),
        exported_at: "2026-10-18 15:00:00".into(),
        day_count: 0,
        nodes: vec![],
    };
    for body in [render_txt(&e, "全部"), render_md(&e, "全部"), render_html(&e, "全部")] {
        assert!(body.contains("暂无剧情内容"), "应提示无内容");
        assert!(body.contains("AI 生成"), "空内容导出同样要带声明");
    }
}
