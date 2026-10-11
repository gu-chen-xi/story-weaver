//! 生成结果入库桥接（ROADMAP 第四批 ⑥ · P1）
//!
//! ## 为什么单独一个模块（而不是直接改 portraits.rs）
//!
//! 立绘素材库必须**对"生成"这件事零感知** —— 否则 `portraits.rs` 里出现生成相关字段/函数，
//! 就又是反向依赖，删掉绘图功能时素材库会被牵连。所以：
//!
//! - `portraits.rs`：**不知道生成存在**，只提供 `portrait_import` 与 `portrait_update_params`
//! - 本模块：**唯一同时知道两边的地方**，负责"生成结果 → 走既有入库入口 + 落生成快照"
//! - `generator.rs`：只管产图到临时目录，不碰数据库
//!
//! 删掉绘图功能时要动的文件只有三个：`generator.rs`、本模块、以及注册表里的一条描述。
//! 素材库与角色管理**一行都不用改** —— 这就是设计文档里的验收标准。
//!
//! ## 严守 R1
//!
//! 入库**必须**调用 `portraits::portrait_import`（而不是自己写文件/插表），这样版本记录、
//! 宽高、大小、分组、标签、绑定全部走既有逻辑，行为与手工导入完全一致。

use crate::portraits::{self, PortraitItem};
use serde::{Deserialize, Serialize};

/// 生成来源（写入 portraits.source，便于素材库筛选与角标）
pub const SOURCE_AI_LOCAL: &str = "ai-local";
pub const SOURCE_AI_CLOUD: &str = "ai-cloud";

/// 一次生成结果入库所需的全部信息
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct IngestRequest {
    /// 临时文件绝对路径（generator 产出的）
    pub file: String,
    /// 素材名（通常是角色名）
    pub name: String,
    #[serde(default)]
    pub role: String,
    #[serde(default)]
    pub tags: String,
    #[serde(default)]
    pub group: String,
    #[serde(default)]
    pub story_id: Option<i64>,
    #[serde(default)]
    pub character_id: Option<i64>,
    /// 通道 id（generator::registry 里的 id）
    #[serde(default)]
    pub generator_id: String,
    /// "local" | "cloud"
    #[serde(default)]
    pub channel: String,
    /// 生成参数快照
    #[serde(default)]
    pub prompt: String,
    #[serde(default)]
    pub negative: String,
    #[serde(default)]
    pub seed: String,
    #[serde(default)]
    pub steps: String,
    #[serde(default)]
    pub cfg: String,
    #[serde(default)]
    pub model: String,
    /// 重出链：新素材指向被重出的那张（P4 用；P1 先透传）
    #[serde(default)]
    pub base_portrait_id: Option<i64>,
}

/// 把一张生成结果入库：**先走素材库既有的导入入口**，再补写来源与参数快照。
pub async fn ingest(req: &IngestRequest) -> Result<PortraitItem, String> {
    if req.file.trim().is_empty() {
        return Err("缺少生成结果文件".into());
    }
    // ① 走既有入口（版本记录、宽高、大小、分组、标签、绑定全部由素材库自己处理）
    let item = portraits::portrait_import(
        req.file.clone(),
        req.name.clone(),
        req.role.clone(),
        String::new(), // ver 由素材库按版本链自增
        req.tags.clone(),
        req.group.clone(),
        req.story_id,
        req.character_id,
    )
    .await?;

    // ② 补写"来源 + 生成参数快照"。
    //    用素材库的**通用列写入**入口 —— 它只负责把字符串写进去，不理解这些列的语义，
    //    所以素材库依然不知道"生成"这件事存在（R2）。
    let source = if req.channel == "cloud" { SOURCE_AI_CLOUD } else { SOURCE_AI_LOCAL };
    let cols: Vec<(&str, String)> = vec![
        ("source", source.to_string()),
        ("origin_tool", req.generator_id.clone()),
        ("derived_from", req.base_portrait_id.map(|v| v.to_string()).unwrap_or_default()),
        ("prompt", req.prompt.clone()),
        ("negative", req.negative.clone()),
        ("seed", req.seed.clone()),
        ("steps", req.steps.clone()),
        ("cfg", req.cfg.clone()),
        ("model", req.model.clone()),
    ];
    portraits::portrait_set_columns(item.id, &cols).await?;

    // ③ 回读完整记录返回给前端
    portraits::portrait_get(item.id).await
}

/// 来源 → 中文角标（素材库里显示用；未知来源不显示角标）
pub fn source_label(source: &str) -> &'static str {
    match source {
        SOURCE_AI_LOCAL => "AI·本地",
        SOURCE_AI_CLOUD => "AI·云端",
        "manual" => "手工",
        "placeholder" => "占位",
        _ => "",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_file_is_rejected_before_touching_library() {
        tauri::async_runtime::block_on(async {
            let r = ingest(&IngestRequest { file: String::new(), name: "x".into(), ..Default::default() }).await;
            assert!(r.is_err());
            assert!(r.unwrap_err().contains("生成结果文件"));
        });
    }

    #[test]
    fn nonexistent_file_fails_through_library_path() {
        tauri::async_runtime::block_on(async {
            // 素材库的导入入口自己会校验文件存在 —— 证明我们确实走了它，而不是绕过它
            let r = ingest(&IngestRequest {
                file: "不存在的文件.png".into(), name: "x".into(), ..Default::default()
            }).await;
            assert!(r.is_err());
            assert!(r.unwrap_err().contains("文件不存在"), "应报素材库的原始错误");
        });
    }

/// **架构边界测试**：把设计文档里的三条硬规则钉成可执行的断言。
    /// 以后谁不小心让素材库"知道"了生成、或让生成绕过入库入口，这里会红。
    #[test]
    fn architecture_boundaries_hold() {
        let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");

        // R2：素材库不得依赖生成（含"生成味"的标识符）
        let portraits = std::fs::read_to_string(src.join("portraits.rs")).expect("读 portraits.rs");
        for bad in ["generator", "GenRequest", "GenResult", "ai-local", "ai-cloud", "gen-tmp", "SD WebUI"] {
            assert!(
                !portraits.contains(bad),
                "R2 违反：portraits.rs 出现了生成相关标识符「{bad}」—— 素材库必须对生成零感知"
            );
        }

        // R1：生成模块不得自己写库（不许 INSERT/UPDATE 素材表）
        let gen = std::fs::read_to_string(src.join("generator.rs")).expect("读 generator.rs");
        // 只看非注释行（以 // 开头的说明文字里提到这些词是文档，不算违规）
        let code_lines: Vec<&str> = gen
            .lines()
            .map(|l| l.trim())
            // 过滤注释行：// 与 /* 与 * 开头的行，以及 //! 模块文档注释（那是说明文字，不是代码）
            .filter(|l| !l.starts_with("//") && !l.starts_with("/*") && !l.starts_with('*'))
            .collect();
        let code = code_lines.join("\n");
        for bad in ["INSERT INTO portraits", "UPDATE portraits", "portraits_dir", "portrait_import(", "sqlx::query"] {
            assert!(
                !code.contains(bad),
                "R1 违反：generator.rs 的代码里出现了「{bad}」—— 生成模块只产临时文件，不碰素材库"
            );
        }

        // 桥接模块必须是唯一同时知道两边的地方
        let bridge = std::fs::read_to_string(src.join("generator_ingest.rs")).expect("读 generator_ingest.rs");
        assert!(bridge.contains("portrait_import"), "桥接模块应调用素材库既有导入入口（R1）");
    }

    #[test]
    fn source_labels_are_chinese() {
        assert_eq!(source_label(SOURCE_AI_LOCAL), "AI·本地");
        assert_eq!(source_label(SOURCE_AI_CLOUD), "AI·云端");
        assert_eq!(source_label("manual"), "手工");
        assert_eq!(source_label("whatever"), "");
    }
}
