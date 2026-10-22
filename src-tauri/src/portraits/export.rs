// 从 `portraits.rs` 拆出的子模块（第二波「按内聚拆业务」）。
//
// `use super::*;` 让本文件看得见 `portraits.rs` 里的其余条目（子模块可访问父模块私有项）。
// 搬过来的条目统一提到 `pub(super)`，父模块用 `pub use export::*;` 取回，
// 外部 `crate::portraits::xxx` 的调用路径**全部不变**。

use super::*;

/// 批量导出图片到 exports/portrait/。返回摘要文本。
#[tauri::command]
pub async fn portrait_export(ids: Vec<i64>) -> Result<String, String> {
    let p = pool().await?;
    ensure_table(&p).await?;
    // 第五批「路径模型」修复：统一走 paths
    let dir = crate::paths::exports_root().join("portrait");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let dir = dir.canonicalize().unwrap_or(dir);
    let mut n = 0usize;
    let mut skip = 0usize;
    // 合规用导出清单：记录每个导出文件的来源与生成参数（AI 生成内容须可溯源）
    let mut manifest: Vec<serde_json::Value> = Vec::new();
    for id in &ids {
        let row = sqlx::query("SELECT name, path, COALESCE(source,''), COALESCE(origin_tool,''), COALESCE(prompt,''), COALESCE(negative,''), COALESCE(seed,'') FROM portraits WHERE id = ?1")
            .bind(id)
            .fetch_optional(&p)
            .await
            .map_err(|e| e.to_string())?;
        if let Some(r) = row {
            let name: String = r.try_get(0).map_err(|e: sqlx::Error| e.to_string())?;
            let fname: Option<String> = r.try_get(1).map_err(|e: sqlx::Error| e.to_string())?;
            match fname {
                Some(f) => {
                    let src = portraits_dir().join(&f);
                    if src.exists() {
                        // 来源与参数（在文件名与清单里带上，保证"AI 生成"标识不因导出而丢失）
                        let source: String = r.try_get(2).unwrap_or_default();
                        let origin_tool: String = r.try_get(3).unwrap_or_default();
                        let prompt: String = r.try_get(4).unwrap_or_default();
                        let negative: String = r.try_get(5).unwrap_or_default();
                        let seed: String = r.try_get(6).unwrap_or_default();
                        let is_ai = source.starts_with("ai-");
                        let ext = f.rsplit('.').next().unwrap_or("png");
                        let clean: String = name
                            .chars()
                            .map(|c| {
                                if "\\/:*?\"<>| ".contains(c) {
                                    '_'
                                } else {
                                    c
                                }
                            })
                            .collect();
                        // AI 生成的文件名带 _AI 后缀：纯字节拷贝（不动图片内容），只让标识跟着文件走
                        let suffix = if is_ai { "_AI" } else { "" };
                        let dst = dir.join(format!("{}_{}{}.{}", clean, id, suffix, ext));
                        std::fs::copy(&src, &dst).map_err(|e| e.to_string())?;
                        manifest.push(serde_json::json!({
                            "file": dst.file_name().map(|x| x.to_string_lossy().to_string()).unwrap_or_default(),
                            "name": name,
                            "source": if source.is_empty() { "unknown".to_string() } else { source.clone() },
                            "ai_generated": is_ai,
                            "origin_tool": origin_tool,
                            "prompt": prompt,
                            "negative": negative,
                            "seed": seed,
                        }));
                        n += 1;
                    } else {
                        skip += 1;
                    }
                }
                None => { skip += 1; }
            }
        } else {
            skip += 1;
        }
    }
    // 写出溯源清单（合规：AI 生成内容须带标识，清单让"哪些是 AI 生成的"跟着导出走）
    let ai_n = manifest.iter().filter(|m| m["ai_generated"] == serde_json::Value::Bool(true)).count();
    let manifest_path = dir.join("导出清单.json");
    let doc = serde_json::json!({
        "generated_at": chrono_like_now(),
        "note": "本清单记录导出素材的来源。ai_generated=true 的图片为 AI 生成内容，按《人工智能生成合成内容标识办法》对外发布时须标注\"AI 生成\"。",
        "total": manifest.len(),
        "ai_generated_count": ai_n,
        "items": manifest,
    });
    let _ = std::fs::write(&manifest_path, serde_json::to_string_pretty(&doc).unwrap_or_default());

    Ok(format!(
        "✅ 已导出 {}/{} 个素材 → {}（其中 AI 生成 {} 个，文件名带 _AI；已附「导出清单.json」）（{} 个无图/缺失跳过）",
        n,
        ids.len(),
        dir.display(),
        ai_n,
        skip
    ))
}
