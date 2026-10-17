// 前端共用的数据形状（与后端 Tauri 命令的返回结构一一对应）。
// 从 App.tsx 抽出（2026-10-17 拆分第一步），**只放类型，不放逻辑**。

export type Story = {
  id: number;
  title: string;
  time_origin: string;
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

export type PortraitItem = { id: number; name: string; role: string; ver: string; ratio: string; width: number; height: number; size: string; size_bytes: number; date: string; pinned: boolean; tags: string[]; g: string; path?: string | null; story_id?: number | null; character_id?: number | null; bound_story?: string | null; bound_character?: string | null; group: string; prompt: string; negative: string; seed: string; steps: string; cfg: string; model: string; versions: { version: number; desc: string; date: string }[]; source?: string; origin_tool?: string; derived_from?: number | null; };

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
  inputBudget: number;
  remain: number;
  pctUsed: number;
};
