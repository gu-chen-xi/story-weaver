// 角色相关的纯函数（从 App.tsx 抽出，2026-10-17）。
//
// 为什么单独放：这两个函数**同时被角色页和剧情页/设定页用**，
// 如果留在 App 里当 props 传，角色页会多两个纯逻辑 props —— 没必要。

/// 从 `current_status` 文本里解析情绪，给出显示用的标签与配色。
/// 文本形如 "…｜情绪：害羞｜…"；认不出时返回灰调。
export function emotionInfo(status: string): { label: string; color: string; bg: string } {
  const m = status.match(/情绪：([^｜]+)/);
  const e = m ? m[1].trim() : "";
  const map: Record<string, [string, string]> = {
    "生气": ["#EA6668", "#fdecec"],
    "开心": ["#52C41A", "#eaf7e6"],
    "难过": ["#4A7FD9", "#eaf1fc"],
    "害羞": ["#EAA7B2", "#fdf0f2"],
    "惊讶": ["#FAAD14", "#fef6e6"],
    "害怕": ["#C9A7E8", "#f5eefc"],
    "平静": ["#9E9E9E", "#f2f2f2"],
    "温柔": ["#94D8C3", "#eaf8f3"],
  };
  const hit = map[e];
  return hit ? { label: e, color: hit[0], bg: hit[1] } : { label: e || "", color: "#6f7686", bg: "#f5f5f5" };
}

/// 角色 id → 名字；查不到时回退成 `#id`（不要显示空白，否则界面上看不出是哪个角色）。
export function charNameOf(chars: { id: number; name: string }[], id: number): string {
  return chars.find((c) => c.id === id)?.name ?? `#${id}`;
}
