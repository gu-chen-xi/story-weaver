// 角色相关的纯函数（从 App.tsx 抽出，2026-10-17）。
//
// v3.5.1：情绪/动作词表**搬到后端**（`src-tauri/src/perform.rs`），前端通过
// `perform_lexicon` 取一次并缓存在模块里。为什么不在前端留一份：
// 回顾词典就出过"两份词表漂移"的事故（其中一份漏了「物品」），这里不重复踩。

import { invoke } from "@tauri-apps/api/core";

type EmoRow = { word: string; group: string; color: string; bg: string };
type ActRow = { word: string; group: string };

let emoMap: Map<string, EmoRow> | null = null;
let actList: ActRow[] | null = null;
let loading: Promise<void> | null = null;

/// 启动时（或首次用到时）拉一次词表；失败就保持内置回退，不打扰用户。
export function loadPerformLexicon(): Promise<void> {
  if (emoMap && actList) return Promise.resolve();
  if (loading) return loading;
  loading = invoke<{ emotions: EmoRow[]; actions: ActRow[] }>("perform_lexicon")
    .then((r) => {
      emoMap = new Map(r.emotions.map((e) => [e.word, e]));
      actList = r.actions;
    })
    .catch(() => { /* 静默：词表拉不到就用回退 */ })
    .finally(() => { loading = null; });
  return loading;
}

/// 解析 `current_status` 里的显式标签（与后端 `perform::parse_status` 同规则）
export function parseStatusTags(status: string): { emotion?: string; action?: string } {
  const pick = (key: string): string | undefined => {
    const i = status.indexOf(key);
    if (i < 0) return undefined;
    let rest = status.slice(i + key.length).replace(/^[：: ]+/, "");
    const end = rest.search(/[｜。；\n|]/);
    if (end >= 0) rest = rest.slice(0, end);
    const v = rest.trim();
    return v || undefined;
  };
  return { emotion: pick("情绪"), action: pick("动作") };
}

/// 从 `current_status` 文本里解析情绪，给出显示用的标签与配色。
/// 文本形如 "…｜情绪：害羞｜…"；认不出时返回灰调。
export function emotionInfo(status: string): { label: string; color: string; bg: string } {
  const { emotion } = parseStatusTags(status);
  const e = (emotion ?? "").trim();
  if (!e) return { label: "", color: "#6f7686", bg: "#f5f5f5" };
  const hit = emoMap?.get(e);
  if (hit) return { label: e, color: hit.color, bg: hit.bg };
  return { label: e, color: "#6f7686", bg: "#f5f5f5" };
}

/// 从 `current_status` 里解析动作（用于角色卡「动作：…」那一行）
export function actionOf(status: string): string {
  return parseStatusTags(status).action ?? "";
}

/// 从一段文本里认出动作词（用于"这句台词带了什么动作"这类轻量判断）
export function actionsInText(text: string): ActRow[] {
  if (!actList) return [];
  return actList.filter((a) => text.includes(a.word));
}

/// 角色 id → 名字；查不到时回退成 `#id`（不要显示空白，否则界面上看不出是哪个角色）。
export function charNameOf(chars: { id: number; name: string }[], id: number): string {
  return chars.find((c) => c.id === id)?.name ?? `#${id}`;
}
