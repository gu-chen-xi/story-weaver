// v3.4：**唯一**的视图名定义。
//
// 为什么收拢：原来 CharsPage / ParamsPage / PluginsPage / StoryPanel 各写了一份
// 一模一样的 union（共 4 份），加一个视图就得改 4 处 —— 典型的复制漂移。
// 现在这 4 个文件都 `export type ViewName = AppView;` 转出去，外部导入路径不变。
export type AppView =
  | "chars" | "world" | "relations" | "story" | "proposals" | "temp"
  | "timeline" | "stories" | "plugins" | "portrait" | "compliance" | "params"
  | "devtools" | "home";

// 前端共用的数据形状（与后端 Tauri 命令的返回结构一一对应）。
// 从 App.tsx 抽出（2026-10-17 拆分第一步），**只放类型，不放逻辑**。

export type Story = {
  id: number;
  title: string;
  time_origin: string;
  /** v3.3：前史/序章 —— 故事开始之前发生的事（空串 = 没写，提示词里整段不出现） */
  prologue: string;
  created_at: string;
  updated_at: string;
};

export type Character = {
  id: number;
  name: string;
  alias: string;
  role: string;
  faction: string;
  age: string;
  gender: string;
  first_appearance: string;
  one_liner: string;
  profile: string;
  speech_style: string;
  traits: string;
  current_status: string;
  stat_stamina: number;
  stat_sanity: number;
  stat_emotion: number;
  stat_screen: number;
  /** v3.4：过去（他的往事）—— 进 system 提示词的角色段 */
  background_past: string;
  /// 外貌（v3.4，作者设定）
  appearance: string;
  /** v3.4：动机（他为什么这么做） */
  motivation: string;
  /** v3.4：目的（他想达成什么） */
  goal: string;
  /** v3.4：对话示例（few-shot；只有焦点角色会进尾部块） */
  dialogue_examples: string;
  /** v3.4：行为准则（他会怎么做 / 绝不做什​么） */
  behavior_rules: string;
  version: number;
};

export type CharacterUsage = {
  id: number;
  dialogue_count: number;
  node_count: number;
};

export type CharacterForm = {
  name: string;
  alias: string;
  role: string;
  faction: string;
  age: string;
  gender: string;
  first_appearance: string;
  one_liner: string;
  profile: string;
  speech_style: string;
  traits: string;
  stat_stamina: number;
  stat_sanity: number;
  stat_emotion: number;
  stat_screen: number;
  // v3.4「角色卡 v2」
  background_past: string;
  /// 外貌（v3.4，作者设定）
  appearance: string;
  motivation: string;
  goal: string;
  dialogue_examples: string;
  behavior_rules: string;
};

export type WorldSetting = {
  id: number;
  key: string;
  value: string;
  is_hard_rule: number;
  version: number;
  category: string;
  meta_json: string;
};

export type WorldConflict = {
  title: string;
  level: string;
  left_id: number;
  right_id: number;
  left_label: string;
  left_text: string;
  left_source: string;
  right_label: string;
  right_text: string;
  right_source: string;
};

export type Relation = {
  id: number;
  from_character_id: number;
  to_character_id: number;
  relation_type: string;
  direction: string;
  strength: number;
  state: string;
  description: string;
  evidence: string;
  version: number;
  split_records: { text: string; role: string }[];
  split_matched: boolean;
};

// ===== 深色主题（参考 STORY DECK 原型 v1）=====

export type PortraitItem = { id: number; name: string; role: string; ver: string; ratio: string; width: number; height: number; size: string; size_bytes: number; date: string; pinned: boolean; tags: string[]; g: string; path?: string | null; story_id?: number | null; character_id?: number | null; bound_story?: string | null; bound_character?: string | null; group: string; prompt: string; negative: string; seed: string; steps: string; cfg: string; model: string; source?: string; origin_tool?: string; derived_from?: number | null; };

export type ScriptBlock = { type: "narr" | "dialog" | "action" | "speaker" | "inner"; name: string; text: string };

export type PromptSection = { name: string; text: string };

export type DebugMessage = { role: string; content: string };

export type BranchDto = {
  id: string; story_id: string; name: string; fork_node_id: number;
  source_branch_id: string | null; color: string; created_at: string; node_count?: number;
  /** 第五批 ④：分支备注（一句话说明这条线是干什么的） */
  note?: string;
};

// 第五批 ②：快照差异预览的类型（与后端 evolution::SnapshotDiff 对应）

export type SnapFieldChange = { field: string; before: string; after: string };

export type SnapItemChange = { kind: "added" | "removed" | "modified"; label: string; fields: SnapFieldChange[] };

export type SnapDiff = {
  characters: SnapItemChange[]; relations: SnapItemChange[]; world_settings: SnapItemChange[];
};

export type TimelineNodeDto = {
  id: number; date_label: string; sort_order: number; node_type: string;
  is_flashback: number; tags: string; day_summary?: string;
  // 第五批 ①：父节点（null = 树根）。现在所有故事都是线性链；
  // 等做分支系统时"多个节点共享同一个 parent_id"就是分叉。
  parent_id?: number | null;
  // 第五批 ③：所属分支（null = 主线）。列表查询返回全树，所以这个字段用来区分节点属于哪条线。
  branch_id?: string | null;
};
// 演进事件的中文名（后端 event_type → 中文；避免界面直接抛英文给用户）

export type DebugLike = {
  system_prompt: string; tail_block?: string | null; history_json: string; user_text: string; estimated_tokens: number;
  // 预算口径（老快照缺省为 0，computeBudget 会回退到配置值）
  context_window?: number; reserve_ratio?: number; max_tokens?: number; safety_margin?: number; input_budget?: number;
  // 跨天历史
  day_window?: number; day_n?: string; n_value?: number; avg_msg_tokens?: number;
};

export type BudgetComputed = {
  sysSections: PromptSection[];
  tailSection: PromptSection | null;   // 尾部「上下文更新」块（易变，放在历史之后）
  history: DebugMessage[];
  lastUser: DebugMessage | null;
  priorHistory: DebugMessage[];
  sysTokens: number;
  histTokens: number;
  userTokens: number;
  usedTokens: number;
  /// `usedTokens` 是否来自"老快照按当前口径重算"（该记录当时没有 estimated_tokens 字段）。
  /// 界面据此加标注，避免把重算值当成历史真实用量。
  usedRecalculated: boolean;
  inputBudget: number;
  remain: number;
  pctUsed: number;
};

export type DebugReq = {
id: number; node_id: number | null; model: string | null;
system_prompt: string;
tail_block: string | null;   // 尾部「上下文更新」块（当前模式 / 篇幅 / 数值与状态 / 聚焦 / 约束 / 时空 / 导演指令）；老快照为空
// 本次实际预算口径（A 子项）：老快照为 0，前端回退到配置值计算
context_window: number; reserve_ratio: number; max_tokens: number; safety_margin: number; input_budget: number;
// 跨天历史（B 子项）：保留几天的原文 / 每天各取几条上限（逗号分隔）/ 由预算倒推出来的 N
day_window: number; day_n: string; n_value: number; avg_msg_tokens: number;
history_json: string; user_text: string;
ai_reply: string | null; estimated_tokens: number; n_used: number;
/** API 实际返回的 usage（0 = 没拿到：老记录 / 上游没开 include_usage） */
prompt_tokens?: number; completion_tokens?: number;
status: string; error: string | null; created_at: string;
};

// ===== 导演代价账本（后端 director.rs::Ledger，2026-10-27）=====

/** 账本里的一行：类型 / 理由 / 分值 / 天；excused = 1 表示「已赦免」（留痕不计分） */
export type DirectorCost = {
  id: number;
  node_id: number | null;
  day_label: string;
  character_id: number | null;
  kind: string;
  reason: string;
  score: number;
  /** model = AI 写的收据；rule = 关键词兜底推断 */
  source: string;
  excused: number;
  /** 这笔账同时扣掉的关系强度（0 = 没动关系；正数 = 扣了多少） */
  rel_delta: number;
  /** 赦免请求次数 / 最近一次判定（空 = 没请求过） */
  pardon_count: number;
  pardon_verdict: string;
  created_at: string;
};

/** 类型表的只读副本（界面用来解释"为什么这条记这么多分"） */
/** 关系强度变化账本的一行（relation_ledger.rs） */
export type RelationChange = {
  id: number;
  relation_id: number;
  node_id: number | null;
  message_id: number | null;
  delta: number;
  kind: string;
  reason: string;
  /** director_cost / pardon / pardon_refused / evolution / user / undo */
  source: string;
  revoked: number;
  created_at: string;
};

/** 一次赦免请求（界面上挂在 AI 回复下面的盒子） */
export type PardonRequest = {
  id: number;
  cost_id: number;
  message_id: number;
  node_id: number;
  extra: string;
  reply: string;
  /** pending / agreed / unclear / refused */
  verdict: string;
  kind: string;
  reason: string;
  /** 当时那条导演指令的原文（盒子第二行） */
  director: string;
  my_name: string;
  ai_name: string;
  created_at: string;
};

export type DirectorCostKind = { name: string; hint: string; score: number };

/** 账本快照：最近 N 条 + 累计（只算没赦免的）+ 上限 */
export type DirectorLedger = {
  rows: DirectorCost[];
  total: number;
  limit: number;
  max_single: number;
  kinds: DirectorCostKind[];
};

export type Proposal = {
id: number; group_id: number; target_type: string; target_id: number;
field: string; old_value: string | null; new_value: string | null;
reason: string; evidence: string; confidence: number;
status: string; note: string; created_at: string;
/// ⑤-B：这条提案来自哪个时间线节点（拍板时显示"来源"）
node_id: number | null;
};

export type Snapshot = {
id: number; version: number; parent_version: number | null; created_at: string;
// 第五批 ②：临时版本 = 回滚前的自动备份；程序启动时会被清掉（退出即失效）
temporary?: number; note?: string;
};

export type AuditLog = { id: number; action: string; target: string | null; before: string | null; after: string | null; operator: string; created_at: string };

export type EventLogItem = { id: number; node_id: number | null; event_type: string; payload_json: string; confidence: number; source: string };

/// 「🔍 检查」报告里的一条问题（C2：从 App.tsx 搬到 types.ts，供弹窗组件复用）
export type CheckItemT = { where_: string; field: string; kind: string; why: string; suggestion: string };

/// 「🔍 检查」给出的可执行补丁（勾选后一键替换）
export type PatchItemT = { board: string; target_kind: string; target_name: string; field: string; before?: string; value: string };
