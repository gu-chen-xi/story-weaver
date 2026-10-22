// 从 `timeline.rs` 拆出的子模块（第二波「按内聚拆业务」）。
//
// `use super::*;` 让本文件看得见 `timeline.rs` 里的其余条目（子模块可访问父模块私有项）。
// 搬过来的条目统一提到 `pub(super)`，父模块用 `pub use node::*;` 取回，
// 外部 `crate::timeline::xxx` 的调用路径**全部不变**。

use super::*;

#[derive(serde::Serialize, Clone)]
pub struct TimelineNode {
    pub id: i64,
    pub date_label: String,
    pub sort_order: i64,
    pub node_type: String,
    pub is_flashback: i64,
    pub tags: String,
    /// D 子项：日摘要（"这一天发生了什么"）。空 = 还没生成；生成一次即固化、不回改。
    #[serde(default)]
    pub day_summary: String,
    /// 第五批 ①：父节点 id。NULL = 树根（通常是「第 1 天」）。
    ///
    /// 现在所有故事都是线性链（每个节点至多一个后继）；等做分支系统时，
    /// **"多个节点共享同一个 parent_id"就是分叉**，数据结构无需再改。
    #[serde(default)]
    pub parent_id: Option<i64>,
    /// 第五批 ③：所属分支（`None` = 主线）。
    ///
    /// ⚠ **必须真的从 SQL 里查出来**：早先只给前端类型加了 `branch_id` 却忘了加进 SELECT，
    /// 结果前端拿到的永远是 `undefined` → 树状图把每条分支的节点都显示成"主线"
    /// （用户实测反馈"id=68 理应属于第六轮，却显示主线"）。
    #[serde(default)]
    pub branch_id: Option<String>,
}

/// 树节点（供纯函数使用；用已有的 TimelineNode 即可，这里给纯函数一个最小输入类型，
/// 这样测试不必构造 day_summary / tags 那些与树无关的字段）
#[derive(Debug, Clone, PartialEq)]
pub struct TreeNode {
    pub id: i64,
    pub parent_id: Option<i64>,
    pub sort_order: i64,
}

/// 由扁平列表构建"父子索引"：parent_id → 其子节点（按 sort_order, id 排序）。
///
/// 说明：**不做连通性校验**（不判断是否有环、是否同一棵树）—— 那是"检查"功能的职责；
/// 这里只负责把父子关系整理出来，遇到环也只会返回一个有限的子表，不会 panic。
// 保留未用：树状视图辅助（调试分支用）
#[allow(dead_code)]
pub fn children_index(nodes: &[TreeNode]) -> std::collections::HashMap<Option<i64>, Vec<i64>> {
    let mut map: std::collections::HashMap<Option<i64>, Vec<i64>> = std::collections::HashMap::new();
    for n in nodes {
        map.entry(n.parent_id).or_default().push(n.id);
    }
    // 同层按 (sort_order, id) 排序 —— 与线性视图的顺序一致
    let order: std::collections::HashMap<i64, (i64, i64)> =
        nodes.iter().map(|n| (n.id, (n.sort_order, n.id))).collect();
    for ids in map.values_mut() {
        ids.sort_by_key(|id| order.get(id).copied().unwrap_or((0, *id)));
    }
    map
}

/// 从某节点向上走到根，返回**从根到该节点**的路径（含两端）。
///
/// 用途：跨天装配要取"当前节点所在的那条线"的历史 —— 有了分支之后，
/// "按 sort_order 取前 N 个"是错的（会把别的分支的内容也读进来），必须按父链取。
/// 遇到环（数据异常）时会在访问过的集合上停下，不会死循环。
pub fn path_to_root(nodes: &[TreeNode], target: i64) -> Vec<i64> {
    let by_id: std::collections::HashMap<i64, &TreeNode> =
        nodes.iter().map(|n| (n.id, n)).collect();
    let mut out: Vec<i64> = Vec::new();
    let mut seen: std::collections::HashSet<i64> = std::collections::HashSet::new();
    let mut cur = Some(target);
    while let Some(id) = cur {
        if !seen.insert(id) {
            break; // 环：停下
        }
        let Some(node) = by_id.get(&id) else { break };
        out.push(id);
        cur = node.parent_id;
    }
    out.reverse(); // 从根到目标
    out
}

/// 展开成"带层级的行"（供树状视图渲染：缩进列表 A 形态）。
///
/// - `root`：从哪个节点开始展开（None = 全部根节点，通常只有一个）
/// - `expanded`：已展开的节点集合；未在集合中的节点其子节点不出现
/// - 返回 `(节点 id, 深度)`，深度从 0 起
///
/// 这是**纯函数**：折叠状态由调用方（界面）持有，函数本身无副作用 —— 便于单测。
// 保留未用：树状视图辅助（调试分支用）
#[allow(dead_code)]
pub fn flatten_tree(
    nodes: &[TreeNode],
    root: Option<i64>,
    expanded: &std::collections::HashSet<i64>,
) -> Vec<(i64, usize)> {
    let idx = children_index(nodes);
    let mut out: Vec<(i64, usize)> = Vec::new();
    let mut stack: Vec<(i64, usize)> = Vec::new();

    // 起点：指定 root 则只展开它；否则展开所有根（parent_id 为 None）
    let starts: Vec<i64> = match root {
        Some(r) => vec![r],
        None => idx.get(&None).cloned().unwrap_or_default(),
    };
    for s in starts.into_iter().rev() {
        stack.push((s, 0));
    }
    let mut seen: std::collections::HashSet<i64> = std::collections::HashSet::new();
    while let Some((id, depth)) = stack.pop() {
        if !seen.insert(id) {
            continue; // 防环
        }
        out.push((id, depth));
        if expanded.contains(&id) {
            if let Some(kids) = idx.get(&Some(id)) {
                for k in kids.iter().rev() {
                    stack.push((*k, depth + 1));
                }
            }
        }
    }
    out
}

/// 某节点的**祖先链**（含自身），从根到自身的顺序 —— 与 `path_to_root` 同义，
/// 单独再导出一个名字是给"跨天装配"用的：那里读起来是"我要这条线上的祖先"。
pub fn ancestor_ids(nodes: &[TreeNode], target: i64) -> Vec<i64> {
    path_to_root(nodes, target)
}

/// 读出一棵树的全部节点（供跨天装配按父链取历史）。
///
/// 与 `list_timeline_nodes` 的区别：本函数只取与"树"有关的三个字段，
/// 且**不做排序假设** —— 树结构由 `parent_id` 决定，顺序由纯函数内部处理。
pub async fn load_tree_nodes(story_id: &str) -> Result<Vec<TreeNode>, sqlx::Error> {
    let pool = crate::story_db::open_story_pool(story_id).await?;
    let rows = sqlx::query("SELECT id, parent_id, sort_order FROM timeline_nodes")
        .fetch_all(&pool)
        .await?;
    Ok(rows
        .iter()
        .map(|r| TreeNode {
            id: r.try_get(0).unwrap_or(0),
            parent_id: r.try_get(1).unwrap_or(None),
            sort_order: r.try_get(2).unwrap_or(0),
        })
        .collect())
}

/// 某节点的子节点 id（用于界面上的"▸ 展开"按钮：无子节点则不显示按钮）
// 保留未用：排障用：某故事有哪些分支
#[allow(dead_code)]
pub fn branches_of(nodes: &[TreeNode], id: i64) -> Vec<i64> {
    children_index(nodes).get(&Some(id)).cloned().unwrap_or_default()
}

/// 是否为分叉点（子节点多于一个）—— 树状视图用它高亮"这里分了叉"
// 保留未用：排障用：某节点是否是分叉点
#[allow(dead_code)]
pub fn is_fork(nodes: &[TreeNode], id: i64) -> bool {
    branches_of(nodes, id).len() > 1
}

/// 获取或创建"第 1 天"节点（M2 先用最简单的单节点）。
pub async fn ensure_first_node(story_id: &str) -> Result<TimelineNode, sqlx::Error> {
    let pool = crate::story_db::open_story_pool(story_id).await?;
    // 查第一个节点
    let row = sqlx::query(
        "SELECT id, date_label, sort_order, node_type, is_flashback, tags, parent_id, branch_id
         FROM timeline_nodes ORDER BY sort_order LIMIT 1"
    )
    .fetch_optional(&pool)
    .await?;

    if let Some(r) = row {
        return Ok(TimelineNode {
            id: r.try_get(0)?,
            date_label: r.try_get(1)?,
            sort_order: r.try_get(2)?,
            node_type: r.try_get(3)?,
            is_flashback: r.try_get(4)?,
            tags: r.try_get(5)?,
            day_summary: r.try_get(6).unwrap_or_default(),
            parent_id: r.try_get::<Option<i64>, _>(7).unwrap_or(None),
            branch_id: r.try_get::<Option<String>, _>(8).unwrap_or(None),
        });
    }

    // 没有就建一个（归属当前分支）
    let branch = current_branch_for(story_id).await;
    let row = sqlx::query(
        "INSERT INTO timeline_nodes(date_label, sort_order, node_type, is_flashback, branch_id)
         VALUES('第 1 天', 1, 'day', 0, ?1) RETURNING id, date_label, sort_order, node_type, is_flashback, tags, parent_id, branch_id"
    )
    .bind(&branch)
    .fetch_one(&pool)
    .await?;
    Ok(TimelineNode {
        id: row.try_get(0)?,
        date_label: row.try_get(1)?,
        sort_order: row.try_get(2)?,
        node_type: row.try_get(3)?,
        is_flashback: row.try_get(4)?,
        tags: row.try_get(5)?,
        day_summary: String::new(),
        parent_id: row.try_get::<Option<i64>, _>(7).unwrap_or(None),
        branch_id: branch.clone(),
    })
}

/// 列出某故事的全部时间线节点（按 sort_order 升序）。
pub async fn list_timeline_nodes(story_id: &str) -> Result<Vec<TimelineNode>, sqlx::Error> {
    let pool = crate::story_db::open_story_pool(story_id).await?;
    list_timeline_nodes_in(&pool).await
}

/// `list_timeline_nodes` 的核心（接 pool，供测试直连）。
///
/// **返回全树**（含各分支的节点）：父子关系判断与 🌲 树状图需要看到所有分支；
/// "只显示当前分支这一条线"是**界面侧**的过滤。
pub async fn list_timeline_nodes_in(pool: &SqlitePool) -> Result<Vec<TimelineNode>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT id, date_label, sort_order, node_type, is_flashback, tags, day_summary, parent_id, branch_id
         FROM timeline_nodes ORDER BY sort_order ASC, id ASC"
    ).fetch_all(pool).await?;
    let mut out = Vec::new();
    for r in &rows {
        out.push(TimelineNode {
            id: r.try_get(0)?,
            date_label: r.try_get(1)?,
            sort_order: r.try_get(2)?,
            node_type: r.try_get(3)?,
            is_flashback: r.try_get(4)?,
            tags: r.try_get(5)?,
            day_summary: r.try_get(6).unwrap_or_default(),
            parent_id: r.try_get::<Option<i64>, _>(7).unwrap_or(None),
            branch_id: r.try_get::<Option<String>, _>(8).unwrap_or(None),
        });
    }
    Ok(out)
}

/// 第五批 ③：**按节点推导它属于哪条分支**（自己开池）。
///
/// 用途：`+下一天` 时"新节点应该落在哪条线"由**界面上那个节点**决定 ——
/// 界面看到哪个节点，就落在哪条线上。这比信任界面记的 `currentBranchId` 可靠
/// （曾经因为界面状态过期，出现"人在分支、节点却建到主线"，用户实测踩到）。
///
/// 返回值：`None` = 该节点在主线（或节点不存在/库打不开 → 都按主线处理，不报错）。
pub async fn branch_of_node_in_story(story_id: &str, node_id: i64) -> Option<Option<String>> {
    let pool = crate::story_db::open_story_pool(story_id).await.ok()?;
    // fetch_optional 直接给出 Option<Option<String>>：
    // 外层"行在不在"，内层"branch_id 是不是 NULL（主线）" —— 正是要的语义
    sqlx::query_scalar("SELECT branch_id FROM timeline_nodes WHERE id = ?1")
        .bind(node_id)
        .fetch_optional(&pool)
        .await
        .ok()?
}

/// 创建下一天节点：
/// - `sort_order` = 当前最大值 + 1（保证排在最后）
/// - `date_label` 的"第 N 天" = **已有天数标签里的最大值 + 1**（插曲/倒叙不占天数；标签无法解析时退回"节点数 + 1"）
// 保留未用：旧入口（已被 create_next_day_node_with 取代），留着兼容测试
#[allow(dead_code)]
pub async fn create_next_day_node(story_id: &str) -> Result<TimelineNode, sqlx::Error> {
    create_next_day_node_with(story_id, None).await
}

/// 第五批 ③：`branch_override` = **界面显式传来的分支**（`None` 时退回"故事记录的当前分支"）。
///
/// 为什么要显式传：`stories.current_branch_id` 是**隐式状态** ——
/// 一旦界面与后端不同步（切换失败、多窗口、崩溃后重启），
/// 就会出现"我在主线，节点却建到了分支里"这类**看不见的错**（用户实测踩到）。
/// 界面把当前分支一起传下来，就不存在不同步的可能。
pub async fn create_next_day_node_with(
    story_id: &str,
    branch_override: Option<Option<String>>,
) -> Result<TimelineNode, sqlx::Error> {
    let pool = crate::story_db::open_story_pool(story_id).await?;
    let branch = match branch_override {
        Some(b) => b,
        None => current_branch_for(story_id).await,
    };
    create_next_day_node_in(&pool, &branch).await
}

/// `create_next_day_node` 的核心（接 pool + 分支，供测试直连）。
///
/// ## 天数与父节点的基准（用户 2026-10-16 定的规则）
///
/// > 分支**按天**算、不按消息；分叉点那一天算**共享历史**，它之后才算分支起点。
///
/// 所以基准 = **分支自己的节点 ∪ 父链上的共享历史**：
/// - **只数分支自己的节点** → 从"第 3 天"分叉的分支里点「下一天」会建成"第 1 天"（错）
/// - **数全库** → 会把别的分支的天数也算进来，跳号
/// - 正确做法：沿 `parent_id` 上溯到分叉点（含），与分支自己的节点合并
pub async fn create_next_day_node_in(
    pool: &SqlitePool,
    branch: &Option<String>,
) -> Result<TimelineNode, sqlx::Error> {
    let b_clause = crate::story_db::branch_clause("branch_id", branch, 1);

    // sort_order 仍取**全库最大值 + 1**：sort_order 是显示排序，全库唯一才不会打架
    let max_row = sqlx::query("SELECT COALESCE(MAX(sort_order), 0) FROM timeline_nodes")
        .fetch_one(pool).await?;
    let max_order: i64 = max_row.try_get(0)?;
    let next_sort = max_order + 1;

    // 分支自己的最后一个节点（父节点与"上溯起点"都用它；
    // 分支还没有自己的节点时为空 → 分叉点本身就是基准）
    let last_sql = format!(
        "SELECT id FROM timeline_nodes WHERE {} ORDER BY sort_order DESC, id DESC LIMIT 1",
        b_clause
    );
    let mut lq = sqlx::query_scalar::<_, i64>(&last_sql);
    if branch.is_some() {
        lq = lq.bind(branch.clone().unwrap_or_default());
    }
    let last_of_branch: Option<i64> = lq.fetch_optional(pool).await?;

    // 父节点 = 分支自己最后一个节点；没有则退到分叉点（其 parent_id 指向的节点）
    let parent_id: Option<i64> = match last_of_branch {
        Some(id) => Some(id),
        None => match branch {
            // 分支：分叉点 = 该分支所有节点的共同祖先。没有自己的节点时，
            // 用 branch 表里的 fork_node_id 作父。
            Some(bid) => sqlx::query_scalar::<_, i64>("SELECT fork_node_id FROM branches WHERE id = ?1")
                .bind(bid)
                .fetch_optional(pool)
                .await?,
            // 主线：最后一个主线节点
            None => sqlx::query_scalar::<_, i64>(
                "SELECT id FROM timeline_nodes WHERE branch_id IS NULL ORDER BY sort_order DESC, id DESC LIMIT 1")
                .fetch_optional(pool)
                .await?,
        },
    };

    // 天数序号：**分支自己的节点 + 父链上的共享历史**（与 create_node_at_day 同一口径，
    // 共用 branch_path_ids；那段逻辑曾在这里重复实现过一次，已合并）
    let path_ids = branch_path_ids(pool, branch).await;
    let labels: Vec<String> = {
        let rows = sqlx::query("SELECT id, date_label, node_type FROM timeline_nodes")
            .fetch_all(pool).await?;
        rows.iter().filter_map(|r| {
            let id: i64 = r.try_get(0).ok()?;
            let nt: String = r.try_get(2).unwrap_or_default();
            if (nt == "day" || nt == "main") && path_ids.contains(&id) {
                Some(r.try_get::<String, _>(1).unwrap_or_default())
            } else { None }
        }).collect()
    };
    let next_day = next_day_number(&labels);
    let label = format!("第 {} 天", next_day);

    let row = sqlx::query(
        "INSERT INTO timeline_nodes(date_label, sort_order, node_type, is_flashback, branch_id, parent_id) VALUES(?1, ?2, 'day', 0, ?3, ?4) RETURNING id, date_label, sort_order, node_type, is_flashback, tags, day_summary, parent_id, branch_id"
    )
    .bind(&label)
    .bind(next_sort)
    .bind(branch.clone())
    .bind(parent_id)
    .fetch_one(pool).await?;
    Ok(TimelineNode {
        id: row.try_get(0)?,
        date_label: row.try_get(1)?,
        sort_order: row.try_get(2)?,
        node_type: row.try_get(3)?,
        is_flashback: row.try_get(4)?,
        tags: row.try_get(5)?,
        day_summary: row.try_get(6).unwrap_or_default(),
        parent_id: row.try_get::<Option<i64>, _>(7).unwrap_or(None),
        branch_id: branch.clone(),
    })
}

/// 在**指定天数**插入一个 day 节点（C 子项"时间流逝"用：用户点日历上的区间条，选择在这段里插入事件）。
/// 天数即时间坐标（第 N 天 = 时间起点 + N-1 天），所以可以插在中间：
/// `sort_order` 取"下一个更大天数节点"的 sort_order，并把其后的节点整体后移一位，保证时间线顺序正确。
// 保留未用：旧入口（已被带 story 参数的新版取代）
#[allow(dead_code)]
pub async fn create_node_at_day(story_id: &str, day_no: i64) -> Result<TimelineNode, sqlx::Error> {
    create_node_at_day_with(story_id, day_no, None).await
}

/// 第五批 ③：`branch_override` 同 `create_next_day_node_with`。
pub async fn create_node_at_day_with(
    story_id: &str,
    day_no: i64,
    branch_override: Option<Option<String>>,
) -> Result<TimelineNode, sqlx::Error> {
    let pool = crate::story_db::open_story_pool(story_id).await?;
    if day_no <= 0 {
        return Err(sqlx::Error::Protocol("天数必须大于 0".to_string()));
    }
    let label = format!("第 {} 天", day_no);

    // 第五批 ③：天数比较用**本分支自己的节点 ∪ 父链上的共享历史**
    // （与 create_next_day_node_in 同一口径；只数自己的会在分支里跳号，数全库会被别的分支挤位）
    let branch = match branch_override {
        Some(b) => b,
        None => current_branch_for(story_id).await,
    };
    let b_clause = crate::story_db::branch_clause("branch_id", &branch, 1);
    let path_ids = branch_path_ids(&pool, &branch).await;
    let day_rows: Vec<(i64, String)> = {
        let rows = sqlx::query("SELECT id, sort_order, date_label, node_type, branch_id FROM timeline_nodes")
            .fetch_all(&pool).await?;
        rows.iter().filter_map(|r| {
            let id: i64 = r.try_get(0).ok()?;
            let nt: String = r.try_get(3).unwrap_or_default();
            let nb: Option<String> = r.try_get(4).unwrap_or(None);
            let mine = nb == branch;
            if (nt == "day" || nt == "main") && (mine || path_ids.contains(&id)) {
                Some((r.try_get(1).unwrap_or(0), r.try_get::<String, _>(2).unwrap_or_default()))
            } else { None }
        }).collect()
    };
    // **同一天不能重复创建**（2026-10-17）：
    // 原实现只校验"天数 > 0"，于是可以造出两个「第 5 天」——
    // 前端弹窗现在会拦，但后端也该守住，否则别的入口（脚本/未来功能）照样能写脏。
    // 范围用**本分支可见的天**（自己的 + 父链共享历史），与其他天数比较口径一致。
    if day_rows
        .iter()
        .any(|(_, lb)| parse_day_number(lb) == Some(day_no))
    {
        return Err(sqlx::Error::Protocol(format!(
            "第 {} 天已经存在，不能重复创建",
            day_no
        )));
    }

    let mut next_sort: Option<i64> = None;
    for (so, lb) in &day_rows {
        if let Some(n) = parse_day_number(lb) {
            if n > day_no && next_sort.map(|x| *so < x).unwrap_or(true) {
                next_sort = Some(*so);
            }
        }
    }
    let insert_at = match next_sort {
        Some(so) => {
            // 其后（含同位置）的节点整体后移，给新节点腾位
            sqlx::query("UPDATE timeline_nodes SET sort_order = sort_order + 1 WHERE sort_order >= ?1")
                .bind(so).execute(&pool).await?;
            so
        }
        None => {
            let max_row = sqlx::query("SELECT COALESCE(MAX(sort_order), 0) FROM timeline_nodes")
                .fetch_one(&pool).await?;
            let m: i64 = max_row.try_get(0)?;
            m + 1
        }
    };

    // 父节点 = 本分支里 sort_order 小于新位置的那个节点（没有就是本分支第一个 → NULL）
    let parent_sql = format!(
        "SELECT id FROM timeline_nodes WHERE {} AND sort_order < ?2 ORDER BY sort_order DESC, id DESC LIMIT 1",
        b_clause
    );
    let mut pq = sqlx::query_scalar::<_, i64>(&parent_sql);
    if branch.is_some() {
        pq = pq.bind(branch.clone().unwrap_or_default());
    }
    let parent_id: Option<i64> = pq.bind(insert_at).fetch_optional(&pool).await?;

    let row = sqlx::query(
        "INSERT INTO timeline_nodes(date_label, sort_order, node_type, is_flashback, branch_id, parent_id) VALUES(?1, ?2, 'day', 0, ?3, ?4)
         RETURNING id, date_label, sort_order, node_type, is_flashback, tags, day_summary, parent_id, branch_id"
    ).bind(&label).bind(insert_at).bind(&branch).bind(parent_id).fetch_one(&pool).await?;
    Ok(TimelineNode {
        id: row.try_get(0)?,
        date_label: row.try_get(1)?,
        sort_order: row.try_get(2)?,
        node_type: row.try_get(3)?,
        is_flashback: row.try_get(4)?,
        tags: row.try_get(5)?,
        day_summary: row.try_get(6).unwrap_or_default(),
        parent_id: row.try_get::<Option<i64>, _>(7).unwrap_or(None),
        branch_id: branch.clone(),
    })
}

/// 创建一个插曲/倒叙节点：date_label 自动叫"插曲 N"，node_type='interlude'，is_flashback=1。
// 保留未用：插曲节点（旧入口，UI 走带 branch 的新版）
#[allow(dead_code)]
pub async fn create_interlude_node(story_id: &str) -> Result<TimelineNode, sqlx::Error> {
    create_interlude_node_with(story_id, None).await
}

/// 第五批 ③：`branch_override` 同 `create_next_day_node_with`。
pub async fn create_interlude_node_with(
    story_id: &str,
    branch_override: Option<Option<String>>,
) -> Result<TimelineNode, sqlx::Error> {
    let pool = crate::story_db::open_story_pool(story_id).await?;
    // 统计已有插曲数量
    let cnt_row = sqlx::query("SELECT COUNT(*) FROM timeline_nodes WHERE node_type = 'interlude'")
        .fetch_one(&pool).await?;
    let cnt: i64 = cnt_row.try_get(0)?;
    let next_interlude = cnt + 1;
    let max_row = sqlx::query("SELECT COALESCE(MAX(sort_order), 0) FROM timeline_nodes")
        .fetch_one(&pool).await?;
    let max_order: i64 = max_row.try_get(0)?;
    let label = format!("插曲 {}", next_interlude);
    let branch = match branch_override {
        Some(b) => b,
        None => current_branch_for(story_id).await,
    };
    let b_clause = crate::story_db::branch_clause("branch_id", &branch, 1);
    // 父节点 = 本分支最后一个节点（插曲接在当前分支末尾）
    let parent_sql = format!(
        "SELECT id FROM timeline_nodes WHERE {} ORDER BY sort_order DESC, id DESC LIMIT 1",
        b_clause
    );
    let mut pq = sqlx::query_scalar::<_, i64>(&parent_sql);
    if branch.is_some() {
        pq = pq.bind(branch.clone().unwrap_or_default());
    }
    let parent_id: Option<i64> = pq.fetch_optional(&pool).await?;
    let row = sqlx::query(
        "INSERT INTO timeline_nodes(date_label, sort_order, node_type, is_flashback, branch_id, parent_id)
         VALUES(?1, ?2, 'interlude', 1, ?3, ?4) RETURNING id, date_label, sort_order, node_type, is_flashback, tags, parent_id, branch_id"
    )
    .bind(&label)
    .bind(max_order + 1)
    .bind(&branch)
    .bind(parent_id)
    .fetch_one(&pool).await?;
    Ok(TimelineNode {
        id: row.try_get(0)?,
        date_label: row.try_get(1)?,
        sort_order: row.try_get(2)?,
        node_type: row.try_get(3)?,
        is_flashback: row.try_get(4)?,
        tags: row.try_get(5)?,
        day_summary: String::new(),
        parent_id: row.try_get::<Option<i64>, _>(7).unwrap_or(None),
        branch_id: branch.clone(),
    })
}

/// 删除一个时间线节点（连同其下所有消息）。至少保留一个节点。
pub async fn delete_timeline_node(story_id: &str, node_id: i64) -> Result<(), sqlx::Error> {
    let pool = crate::story_db::open_story_pool(story_id).await?;
    delete_timeline_node_in(&pool, node_id).await
}

/// `delete_timeline_node` 的核心（接 pool，供测试直连）。
pub async fn delete_timeline_node_in(pool: &SqlitePool, node_id: i64) -> Result<(), sqlx::Error> {
    // 第五批 ③：**禁止删除"被分支当作分叉点"的节点**（用户实测踩到）。
    // 删掉它之后分支就失去了共享历史的起点：天数会从 0 重新数（建出"第 1 天"），
    // 子节点的 parent_id 也会指向不存在的行 —— 数据变成"半坏"状态且不报错。
    let dependents: Vec<String> = sqlx::query_scalar("SELECT name FROM branches WHERE fork_node_id = ?1")
        .bind(node_id)
        .fetch_all(pool)
        .await
        .unwrap_or_default();
    if !dependents.is_empty() {
        return Err(sqlx::Error::Protocol(format!(
            "该节点是分支「{}」的分叉点，删掉它会让那条分支失去起点（天数错乱、父节点悬空）。请先删除该分支或改它的分叉点。",
            dependents.join("」「")
        )));
    }

    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM timeline_nodes").fetch_one(pool).await?;
    if count <= 1 {
        return Err(sqlx::Error::Protocol("至少保留一个时间线节点".to_string()));
    }
    sqlx::query("DELETE FROM messages WHERE node_id = ?1").bind(node_id).execute(pool).await?;
    sqlx::query("DELETE FROM timeline_nodes WHERE id = ?1").bind(node_id).execute(pool).await?;
    Ok(())
}
