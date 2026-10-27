use sqlx::Row;
use sqlx::SqlitePool;



/// 第五批 ③：对外函数使用的"当前分支"（读 `stories.current_branch_id`，`None` = 主线）。
///
/// 规则：**对外函数按"当前分支"工作，`_in` 核心版只管"给定分支"**。
/// 这样界面切分支只需调一次 `set_current_branch`，其余命令自动沿用（不会漏传参数）。
async fn current_branch(story_id: &str) -> Option<String> {
    crate::branch::resolve_branch(story_id, &None).await.unwrap_or(None)
}


#[derive(serde::Serialize, Clone)]
pub struct RelationSplit {
    pub text: String,
    pub role: String,
}

#[derive(serde::Serialize, Clone)]
pub struct Relation {
    pub id: i64,
    pub from_character_id: i64,
    pub to_character_id: i64,
    pub relation_type: String,
    pub direction: String,
    pub strength: i64,
    pub state: String,
    pub description: String,
    pub evidence: String,
    pub version: i64,
    pub split_records: Vec<RelationSplit>,
    pub split_matched: bool,
}

/// 可识别的两字亲属词（姐妹 / 兄弟 按需求排除）：词 → 长辈单字 → 晚辈单字
const KIN_PAIRS: &[(&str, &str, &str)] = &[
    ("父女", "父", "女"),
    ("父子", "父", "子"),
    ("母女", "母", "女"),
    ("母子", "母", "子"),
    ("兄妹", "兄", "妹"),
    ("姐弟", "姐", "弟"),
];

fn role_gender(role: &str) -> Option<&'static str> {
    match role {
        "父" | "兄" | "弟" | "子" => Some("男"),
        "母" | "姐" | "妹" | "女" => Some("女"),
        _ => None,
    }
}

/// 关系类型是否命中可识别的两字亲属词（前端只给命中的双向条目开「展开拆分」开关）。
pub fn is_kinship(relation_type: &str) -> bool {
    KIN_PAIRS.iter().any(|(w, _, _)| relation_type.contains(w))
}

fn parse_age(age: &str) -> Option<i64> {
    age.trim().parse::<i64>().ok()
}

/// 双向关系拆成两条单向记录：
/// 1) 先按性别判断长辈（同性的父子/母女进入年龄判断）；
/// 2) 年龄大的是长辈；
/// 3) 都判断不了时，把 from(a) 兜底当长辈。
pub fn split_for_relation(
    direction: &str,
    from_name: &str,
    to_name: &str,
    from_gender: &str,
    to_gender: &str,
    from_age: &str,
    to_age: &str,
    relation_type: &str,
) -> Vec<RelationSplit> {
    if direction != "双向" {
        return Vec::new();
    }
    let rel = relation_type.trim();
    if rel.is_empty() || rel == "陌生人" {
        return vec![RelationSplit {
            text: format!("{} 和 {} 是陌生人", from_name, to_name),
            role: "陌生人".to_string(),
        }];
    }

    // 关键词识别（只认两字词）
    if let Some((_, elder_char, younger_char)) = KIN_PAIRS.iter().find(|(w, _, _)| rel.contains(w)) {
        let elder_gender = role_gender(elder_char).unwrap_or("");
        let from_matches = !from_gender.is_empty() && from_gender == elder_gender;
        let to_matches = !to_gender.is_empty() && to_gender == elder_gender;
        let from_is_elder = if from_matches && !to_matches {
            true
        } else if to_matches && !from_matches {
            false
        } else {
            match (parse_age(from_age), parse_age(to_age)) {
                (Some(a), Some(b)) if a != b => a > b,
                _ => true, // 兜底：from(a) 当长辈
            }
        };
        let (elder_name, younger_name) = if from_is_elder { (from_name, to_name) } else { (to_name, from_name) };
        return vec![
            RelationSplit { text: format!("{} 是 {} 的 {}", elder_name, younger_name, elder_char), role: elder_char.to_string() },
            RelationSplit { text: format!("{} 是 {} 的 {}", younger_name, elder_name, younger_char), role: younger_char.to_string() },
        ];
    }

    // 非亲属词的双向：同一关系类型拆成两条单向
    vec![
        RelationSplit { text: format!("{} 是 {} 的「{}」", from_name, to_name, rel), role: rel.to_string() },
        RelationSplit { text: format!("{} 是 {} 的「{}」", to_name, from_name, rel), role: rel.to_string() },
    ]
}

// ===== 关系强度区间（左闭右开，100 单独归顶档）：档位名 + 含义 + 表演要求 =====
/// (档位名, 区间左值, 区间右值含, 含义, 表演要求)；区间左闭右开，100 单独归"羁绊"
/// 关系强度五档：`(档名, 下界, 上界, 含义, 表演, 配合度)`。
///
/// **第 6 项 `coop`（配合度）是 2026-10-22 新增的**，因为配合度段原来在 prompt 里
/// 手写了一份档位名与边界，和这里**不一致**（写成「客气/熟识」、边界 0-19/20-39，
/// 而真相是「相识/亲近/交心」、0-20/20-40）→ 模型看到两套映射，表现成"疏离档却放行"。
/// 现在配合度段由 [`coop_legend`] 从这个常量生成，**单一真相**，物理上不可能再漂移。
pub const STRENGTH_BANDS: [(&str, i64, i64, &str, &str, &str); 5] = [
    ("疏离", 0, 20, "只有身份的牵连，没有情感投入。", "对话简短、公事化；不主动关心对方，不记得对方的细节；冲突时优先保护自己。",
     "默认拒绝、回避、拖延。主动配合必须有明确理由（对方先让步、有交换、或剧情硬需求）；亲密接触一律先拒。宁可写得冷淡生硬，也不要凭空给善意。"),
    ("相识", 20, 40, "有过交集，有基本好感或轻微戒备。", "会打招呼、会闲聊，但点到为止；对方遇事只给一句场面话，不会出手。",
     "配合但带保留，会先问清代价；亲密接触仍会避开或反问一句。"),
    ("亲近", 40, 60, "愿意分享一部分生活，有分寸的往来。", "主动搭话、开玩笑、告知无关紧要的私事；真出事时会犹豫，但仍以自身安危优先。",
     "一般会配合，可以讨价还价；重大让步需要理由。"),
    ("交心", 60, 80, "信任对方，会依赖，会为对方让步。", "重要的事会找对方商量；冲突时愿意先低头；对方的情绪能直接影响自己的行动。",
     "愿意让步、主动帮，可以不问代价。"),
    ("羁绊", 80, 100, "视对方为不可或缺的人，愿意牺牲或背负。", "对方受伤害会激烈反应；为对方破例、破规矩、担风险都在所不惜；台词和动作里带明显的占有或守护。",
     "愿意冒险、替对方扛，会主动越界去保护对方。"),
];

// ===== 导演指令（沉浸对话专用）：内联标记 + 注入说明 =====
/// 内联标记前缀（前端写入消息内容、后端剥离；改这里必须同步改 App.tsx 的 DIRECTOR_TAG_RE）
pub const DIRECTOR_TAG: &str = "〔导演指令〕";

/// 取出消息里的导演指令内容（指令本身可多行，直到空行结束）；没有则 None
pub fn parse_director(content: &str) -> Option<String> {
    let t = content.trim_start();
    if !t.starts_with(DIRECTOR_TAG) {
        return None;
    }
    let body = &t[DIRECTOR_TAG.len()..];
    let body = body.split("\n\n").next().unwrap_or(body).trim();
    if body.is_empty() {
        None
    } else {
        Some(body.to_string())
    }
}

/// 去掉导演指令行，只留用户正文（用于发给模型 / 调试快照 / 状态提取）
pub fn strip_director(content: &str) -> String {
    let t = content.trim_start();
    if !t.starts_with(DIRECTOR_TAG) {
        return content.to_string();
    }
    let body = &t[DIRECTOR_TAG.len()..];
    match body.split_once("\n\n") {
        Some((_, rest)) => rest.trim_start().to_string(),
        None => String::new(),
    }
}


/// 去掉**任意位置**的导演指令块（标记行 + 其后的非空行，直到空行结束）。
///
/// 和 [`strip_director`] 的区别：那个只处理"整条消息以标记开头"的存储格式；
/// 这个处理**拼好的一段文本**（例如关键记忆的"最近一次"是 `[用户] …` 前缀过的），
/// 标记可能在第 2 行。2026-10-27：卡片里抄进了一大段导演指令，就是因为只认开头。
pub fn strip_director_anywhere(text: &str) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut skipping = false;
    for line in text.lines() {
        // 注意：拼好的文本里标记前面还有个 "[用户] " 前缀，所以不能只看行首 —— 要看"整行里有没有"。
        if let Some(pos) = line.find(DIRECTOR_TAG) {
            // 标记行：保留标记**之前**的部分（"[用户] " 这种前缀），丢掉指令正文
            let head = line[..pos].trim_end();
            if !head.trim().is_empty() {
                out.push(head.to_string());
            }
            skipping = true;
            continue;
        }
        if skipping {
            if line.trim().is_empty() {
                skipping = false;
            }
            continue;
        }
        out.push(line.to_string());
    }
    out.join("\n").trim().to_string()
}
/// 导演指令的注入说明（只在沉浸对话模式注入；同样不能出现【】）
///
/// **v3.7.0 改口径**：从「必须执行 + 写出转变过程」改成「必须达成 + 必须付代价」。
/// 起因：疏离档（1-20）下只要求"写出过程"，模型就走捷径 ——「嘴上说不要，身体先软」
/// 或者一句「她其实早就喜欢他」把前面的抗拒抹掉，人设直接崩。
/// 现在把"代价"写成硬要求：条件 / 交换 / 时限 / 外部压力，至少一样，当场写明。
///
/// 为什么不写「可以拒绝导演指令」：那是软件层的事（强度 = 0 时前端直接不给发，
/// 见 `src/lib/directorGate.ts`）。给模型一个"可以拒"的口子，它会当成默认行为，
/// 每轮都拒一次显得有骨气 —— 这和配合度段当初把「倾向」当绿灯是同一个坑。
pub fn director_legend() -> String {
    let mut s = String::from("用户作为导演下达了本次剧情要求（不是用户角色的台词）：");
    s.push_str("\n- 必须达成：这一轮就要让要求的结果落地，不必先征得角色同意。强度 1 以上都算「还有一点希望」，不许以「她不愿意」为由整轮不推进。");
    s.push_str("\n- 必须付代价：强度 1-20（疏离）时，结果要经由一个当场写明的代价换取——条件、交换、时限、外部压力，至少一样；强度越高代价越轻，20 以上按该档配合度自然演出。");
    s.push_str("\n- 不得白给：不许「嘴上拒绝、身体先软」，台词与动作同向；不许用一句心理独白（「她其实早就……」）把前面的抗拒抹掉；不许改写角色的内心动机；不许写成突然换了个人。");
    s.push_str("\n- 写出转变过程：让相关角色经历内心挣扎、被触动、被迫妥协等合理过程，不要突兀硬拗。");
    s.push_str("\n- 强度 0 是彻底决裂，本就轮不到导演指令（软件层已拦）；若历史里仍带着，只演决裂本身——拒绝、回避、不回应，不推进任何关系。");
    s.push_str("\n- 用户扮演的角色仍由用户掌控（不得替其写台词或心声）；只作用于本次回复，不改变已设定的角色性格、关系与世界观。");
    s
}

/// 强度值 → 档位名。区间左闭右开（含下限不含上限），100 单独归顶档；越界值夹到 0~100
pub fn strength_band(strength: i64) -> &'static str {
    let v = strength.clamp(0, 100);
    for (name, lo, hi, _, _, _) in STRENGTH_BANDS.iter() {
        if v >= *lo && v < *hi {
            return name;
        }
    }
    "羁绊"
}

/// 关系状态档位（三选一，界面为固定下拉）
pub const RELATION_STATES: [(&str, &str); 3] = [
    ("稳定", "冲突止于拌嘴"),
    ("紧张", "容易升级为争执"),
    ("破裂", "出现回避、断绝、伤害性言行"),
];

/// 状态名 → 含义；非三档取值统一按「稳定」处理
pub fn relation_state_meaning(state: &str) -> &'static str {
    let s = state.trim();
    for (name, meaning) in RELATION_STATES.iter() {
        if s == *name {
            return meaning;
        }
    }
    RELATION_STATES[0].1
}

/// 关系方向读法（跟强度/状态图例一起注入；同样不能出现【】）
/// 单向＝数值是主动方的投入强度；双向＝数值是双方的相互义务；两侧各自记的数值各读各的
// 保留未用：提示词词典（保留：方向说明将来可能进 system）
#[allow(dead_code)]
pub fn direction_legend() -> String {
    let mut s = String::from("关系方向读法（同一条数值，方向不同读法不同）：");
    s.push_str("\n- 单向：数值代表主动方（箭头起点那一方）的投入强度，只描述主动方；对面那一方视为未明确，按该角色自身性格与其对人的既有态度来演，不主动、不假定同档反应。");
    s.push_str("\n- 双向：数值代表双方的相互义务与牺牲意愿，两人都按该档表现。");
    s.push_str("\n- 两侧各自记录数值时（A→B 与 B→A 各有一条），各读各的，不要合并成平均关系。");
    s
}

/// 关系状态图例（跟强度档位一起注入；同样不能出现【】）
pub fn state_legend() -> String {
    // 英文脚手架 + **保留中文枚举值**（数据里存的就是「稳定/紧张/破裂」，
    // 纯英文解释会让模型对不上数据）。详见 2026-10-22 的英文化试点记录。
    let mut s = String::from(
        "Relation footing (稳定/紧张/破裂 — the current footing BETWEEN THEM; it drives behaviour in conflict):",
    );
    for (name, meaning) in RELATION_STATES.iter() {
        s.push_str(&format!("\n- {} — {}", name, meaning));
    }
    s
}

/// 【关系态势】的行为规则（v3.4，2026-10-22 补）。
///
/// 为什么必须补：`稳定/紧张/破裂` 原先只是标签（含义有、规则没有），
/// 于是它和「关系强度」抢同一件事 —— 强度说"要不要配合"，态势说"冲突能闹多大"，
/// 但没写清谁管哪一段，冲突戏一多就会互相打架。
///
/// 分工（写进提示词，别改）：**强度管"愿不愿意"，态势管"冲突烈度上限"**。
/// 两者不互相覆盖：强度再高、态势破裂了，照样会冷、会拒（只是心里更痛）。
pub fn footing_rule() -> String {
    [
        "【关系态势】决定冲突的烈度上限（与关系强度分工：强度管「愿不愿意配合」，态势管「闹到什么程度」）：",
        "- 稳定：可以拌嘴、可以冷脸，但不动手、不翻旧账、不断联；冲突后靠一句话或一个动作就能回缓。",
        "- 紧张：会升级 —— 翻旧账、说重话、试探底线；事后**需要一件具体的事**才能缓和，一句话好不了。",
        "- 破裂：出现回避（不见、不回）、断绝（搬走、断联）、伤害性言行；配合度整体下调一档，",
        "  且**不接受无代价的和解**（想和好必须付出具体代价，不能靠撒娇或时间冲淡）。",
        "冲突时若态势与强度指向相反（强度高但态势紧张/破裂），**以态势定烈度**、以强度定语气温度 ——",
        "例：羁绊 + 破裂 = 越痛越狠，话说得越软越伤人。",
    ]
    .join("\n")
}

/// 【配合度】段（v3.4）：关系强度 → 配合意愿。
///
/// 为什么单独成段而不是塞进强度图例：强度图例讲"怎么相处"，这里讲"**要不要答应**"——
/// 后者是模型最容易放水的地方（"她嘴上说不要，其实心里…"）。
///
/// 三条关键约束（按 AI 审阅意见补，2026-10-22）：
///   ① 低强度**默认拒绝**，"倾向"不能当绿灯；
///   ② 硬触发清单：满足任一条件必须明确拒绝，不得口头敷衍就放行；
///   ③ 拒绝后就停住 —— 不加缓和句、不加「但是」、不加半推半就的动作。
pub fn coop_legend() -> String {
    let mut s = String::from(
        "【配合度】关系强度决定「愿不愿意配合」。这是**默认方向**，不是可选倾向：",
    );
    for (name, lo, hi, _desc, _act, coop) in STRENGTH_BANDS.iter() {
        s.push_str(&format!("\n- {}-{} {}：{}", lo, hi, name, coop));
    }
    s.push_str(
        "\n出现以下任一情况，必须**明确拒绝**，不得只做口头敷衍就放行：\
         \n- 上一轮出现过肢体伤害、且没有得到解释或道歉；\
         \n- 强度 < 20 且对方提出亲密接触；\
         \n- 台词与身体动作冲突时，以「拒绝」为准（嘴上说「不许」，身体不能先让开）。\
         \n拒绝之后就停住：不加缓和句、不加「但是」、不加半推半就的动作（让开半步、手松开、低声补软话）。\
         \n拒绝以站位和身体为准：门不开、人不动、手不放下。\
         \n低理智 ≠ 好说话：理智低表现为直白、冲动、不留缓冲，可能是直接顶回去，不是失去原则。\
         \n输出前自检：本回应里有没有实质同意信号（让步站位、放行、主动靠近）？\
         \n若有、而当前强度或当下状态不支持，删掉该动作重写。\
         \n硬规则与剧情约束优先级最高：导演指令要求配合时照做 —— 但强度低时必须付出**当场写明的代价**，\
         \n不得只靠「不情愿」四个字交差，也不许用心理独白把前面的抗拒抹掉。",
    );
    s
}

/// 关系强度档位图例（注入【数据字典】段，让 AI 按档位把握表演分寸）
/// 注意：图例正文不能出现【】（提示词预览按【】切段，会被误拆成独立段落）
pub fn strength_legend() -> String {
    let mut s = String::from(
        "Relationship strength (0-100, larger = closer; each band is [low, high), 100 belongs to the top band):",
    );
    for (name, lo, hi, desc, act, _) in STRENGTH_BANDS.iter() {
        s.push_str(&format!(
            "\n- {}-{} {} — Meaning: {} Performance: {}",
            lo, hi, name, desc, act
        ));
    }
    s.push_str(
        "\nUse the band of the CURRENT value to set the social distance and the way the character performs.\
         \nPre-existing facts carried by the relation type / note (kinship, forms of address, old grudges) stay valid.\
         \nIf they conflict: the band decides the manner, the note decides the fact.",
    );
    s
}

// ===== 关系条目的两种渲染：「方向和关系」（稳定，进 system）与「数值与状态」（易变，进尾部块）=====

/// 「方向和关系」一条：A 是 B 的「类型」（方向：…，说明：…）—— 不含强度数值
pub fn rel_direction_line(text: &str, dir: &str, desc: &str) -> String {
    let d = if desc.trim().is_empty() { String::new() } else { format!("，说明：{}", desc.trim()) };
    format!("- {}（方向：{}{}）", text, dir, d)
}

/// 「数值与状态」一条：A → B：强度 83（羁绊）｜状态：稳定
pub fn rel_value_line(from: &str, to: &str, strength: i64, state: &str) -> String {
    format!("- {} → {}：强度 {}（{}）｜关系态势：{}（{}）", from, to, strength, strength_band(strength), state, relation_state_meaning(state))
}

/// 断言档位划分（自测）：边界左闭右开、100 归顶档
// 保留未用：自检用：强度档位断言
#[allow(dead_code)]
pub fn assert_strength_bands() {
    assert_eq!(strength_band(0), "疏离");
    assert_eq!(strength_band(19), "疏离");
    assert_eq!(strength_band(20), "相识");
    assert_eq!(strength_band(39), "相识");
    assert_eq!(strength_band(40), "亲近");
    assert_eq!(strength_band(59), "亲近");
    assert_eq!(strength_band(60), "交心");
    assert_eq!(strength_band(79), "交心");
    assert_eq!(strength_band(80), "羁绊");
    assert_eq!(strength_band(99), "羁绊");
    assert_eq!(strength_band(100), "羁绊");
    assert_eq!(strength_band(-5), "疏离");
    assert_eq!(strength_band(120), "羁绊");
}

const SELECT_COLS: &str = "r.id, r.from_character_id, r.to_character_id, r.relation_type, r.direction, r.strength, r.state, r.description, r.evidence, r.version, COALESCE(f.name,''), COALESCE(f.gender,''), COALESCE(f.age,''), COALESCE(t.name,''), COALESCE(t.gender,''), COALESCE(t.age,'')";

pub async fn list_relations(story_id: &str) -> Result<Vec<Relation>, sqlx::Error> {
    let pool = crate::story_db::open_story_pool(story_id).await?;
    let b = current_branch(story_id).await;
    list_relations_in(&pool, &b).await
}

/// 第五批 ③：分支感知的关系列表（`None` = 主线）。
///
/// 同时给**两端的角色表**加分支条件：分支里的关系指向的是该分支自己的角色行，
/// 主线的角色行不该被别的分支的关系 JOIN 出来。
pub async fn list_relations_in(
    pool: &SqlitePool,
    branch: &Option<String>,
) -> Result<Vec<Relation>, sqlx::Error> {
    let rel_clause = crate::story_db::branch_clause("r.branch_id", branch, 1);
    let f_clause = crate::story_db::branch_clause("f.branch_id", branch, 2);
    let t_clause = crate::story_db::branch_clause("t.branch_id", branch, 3);
    let sql = format!(
        "SELECT {} FROM relations r
         LEFT JOIN characters f ON f.id = r.from_character_id AND {}
         LEFT JOIN characters t ON t.id = r.to_character_id AND {}
         WHERE {} ORDER BY r.id",
        SELECT_COLS, f_clause, t_clause, rel_clause
    );
    let mut q = sqlx::query(&sql);
    // 占位符顺序：1=关系分支，2=from 角色分支，3=to 角色分支
    for _ in 0..3 {
        if let Some(b) = branch {
            q = q.bind(b);
        }
    }
    let rows = q.fetch_all(pool).await?;
    let mut out = Vec::new();
    for r in &rows {
        let direction: String = r.try_get(4)?;
        let relation_type: String = r.try_get(3)?;
        let from_name_raw: String = r.try_get(10)?;
        let from_name = if from_name_raw.trim().is_empty() { format!("#{}", r.try_get::<i64, _>(1)?) } else { from_name_raw };
        let from_gender: String = r.try_get(11)?;
        let from_age: String = r.try_get(12)?;
        let to_name_raw: String = r.try_get(13)?;
        let to_name = if to_name_raw.trim().is_empty() { format!("#{}", r.try_get::<i64, _>(2)?) } else { to_name_raw };
        let to_gender: String = r.try_get(14)?;
        let to_age: String = r.try_get(15)?;
        let split_records = split_for_relation(&direction, &from_name, &to_name, &from_gender, &to_gender, &from_age, &to_age, &relation_type);
        let split_matched = is_kinship(&relation_type);
        out.push(Relation {
            id: r.try_get(0)?,
            from_character_id: r.try_get(1)?,
            to_character_id: r.try_get(2)?,
            relation_type,
            direction,
            strength: r.try_get(5)?,
            state: r.try_get(6)?,
            description: r.try_get(7)?,
            evidence: r.try_get(8)?,
            version: r.try_get(9)?,
            split_records,
            split_matched,
        });
    }
    Ok(out)
}

#[allow(clippy::too_many_arguments)]
pub async fn create_relation(
    story_id: &str,
    from_id: i64,
    to_id: i64,
    relation_type: &str,
    direction: &str,
    strength: i64,
    state: &str,
    description: &str,
    evidence: &str,
) -> Result<i64, sqlx::Error> {
    let pool = crate::story_db::open_story_pool(story_id).await?;
    let b = current_branch(story_id).await;
    create_relation_in(&pool, &b, from_id, to_id, relation_type, direction, strength, state, description, evidence).await
}

/// 第五批 ③：新建关系（写入带上分支）。
#[allow(clippy::too_many_arguments)]
pub async fn create_relation_in(
    pool: &SqlitePool,
    branch: &Option<String>,
    from_id: i64,
    to_id: i64,
    relation_type: &str,
    direction: &str,
    strength: i64,
    state: &str,
    description: &str,
    evidence: &str,
) -> Result<i64, sqlx::Error> {
    let row = sqlx::query(
        "INSERT INTO relations(from_character_id, to_character_id, relation_type, direction, strength, state, description, evidence, branch_id)
         VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9) RETURNING id",
    )
    .bind(from_id)
    .bind(to_id)
    .bind(relation_type)
    .bind(direction)
    .bind(strength)
    .bind(state)
    .bind(description)
    .bind(evidence)
    .bind(branch.clone())
    .fetch_one(pool)
    .await?;
    Ok(row.try_get(0)?)
}

#[allow(clippy::too_many_arguments)]
pub async fn update_relation(
    story_id: &str,
    id: i64,
    relation_type: &str,
    direction: &str,
    strength: i64,
    state: &str,
    description: &str,
    evidence: &str,
) -> Result<(), sqlx::Error> {
    let pool = crate::story_db::open_story_pool(story_id).await?;
    let b = current_branch(story_id).await;
    update_relation_in(&pool, &b, id, relation_type, direction, strength, state, description, evidence).await
}

/// 第五批 ③：更新关系（带分支条件，防止跨分支误改）。
#[allow(clippy::too_many_arguments)]
pub async fn update_relation_in(
    pool: &SqlitePool,
    branch: &Option<String>,
    id: i64,
    relation_type: &str,
    direction: &str,
    strength: i64,
    state: &str,
    description: &str,
    evidence: &str,
) -> Result<(), sqlx::Error> {
    let clause = crate::story_db::branch_clause("branch_id", branch, 8);
    let sql = format!(
        "UPDATE relations SET relation_type=?1, direction=?2, strength=?3, state=?4, description=?5, evidence=?6, version=version+1 WHERE id=?7 AND {}",
        clause
    );
    let mut q = sqlx::query(&sql)
        .bind(relation_type)
        .bind(direction)
        .bind(strength)
        .bind(state)
        .bind(description)
        .bind(evidence)
        .bind(id);
    if let Some(b) = branch {
        q = q.bind(b);
    }
    q.execute(pool).await?;
    Ok(())
}

pub async fn delete_relation(story_id: &str, id: i64) -> Result<(), sqlx::Error> {
    let pool = crate::story_db::open_story_pool(story_id).await?;
    let b = current_branch(story_id).await;
    delete_relation_in(&pool, &b, id).await
}

/// 第五批 ③：删除关系（带分支条件）。
pub async fn delete_relation_in(
    pool: &SqlitePool,
    branch: &Option<String>,
    id: i64,
) -> Result<(), sqlx::Error> {
    let clause = crate::story_db::branch_clause("branch_id", branch, 2);
    let sql = format!("DELETE FROM relations WHERE id = ?1 AND {}", clause);
    let mut q = sqlx::query(&sql).bind(id);
    if let Some(b) = branch {
        q = q.bind(b);
    }
    q.execute(pool).await?;
    Ok(())
}

#[cfg(test)]
mod tests;
