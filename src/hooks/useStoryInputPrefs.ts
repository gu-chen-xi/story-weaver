// 剧情输入台的偏好状态（v3.5.6，A 步第四块）。
//
// 归属：`views/StoryInput`（底部输入台）+ `views/DirectorPanel`（右侧导演监视器）。
// 都是"怎么写这一句"的界面偏好，跟故事数据无关 —— 其中两项持久化在 localStorage。
//
// 敏感词过滤不在这里：它是**内置常开**的常量（用户 2026-10-23 定，不给开关）。

import { useCallback, useState } from "react";

export type NavMode = "旁白" | "对话";
export type LenLevel = "简单" | "详细";

const CONS_KEY = "ai.constraints";
const DIR_KEY = "ai.directors";

function readList(key: string): string[] {
  try {
    const raw = JSON.parse(localStorage.getItem(key) ?? "[]");
    return Array.isArray(raw) ? raw.filter((x): x is string => typeof x === "string") : [];
  } catch {
    return [];
  }
}

export function useStoryInputPrefs() {
  /** 旁白 / 对话 */
  const [navMode, setNavMode] = useState<NavMode>("对话");
  /** 对话模式：我扮演的角色 */
  const [myRole, setMyRole] = useState("自由");
  /** 对话模式：AI 扮演的角色 */
  const [aiRole, setAiRole] = useState("自由");
  /** 剧情约束（持久化） */
  const [constraints, setConstraints] = useState<string[]>(() => readList(CONS_KEY));
  /** 篇幅：简单描写 / 详细展开 */
  const [lenLevel, setLenLevel] = useState<LenLevel>("简单");
  /** 记忆上下文：是否携带历史 */
  const [memoryCtx, setMemoryCtx] = useState(true);
  /** 「🔍 回顾」弹窗是否打开 */
  const [showRecallPicker, setShowRecallPicker] = useState(false);
  /** 导演指令（一次性：附加到下一次发送后自动清空；持久化） */
  const [directors, setDirectors] = useState<string[]>(() => readList(DIR_KEY));
  /** 导出粒度弹窗 */
  const [showExport, setShowExport] = useState(false);

  /** 改导演指令并落盘（持久化是"下次打开还在"的意思，不是"一直带着"） */
  const updateDirectors = useCallback((list: string[]) => {
    setDirectors(list);
    localStorage.setItem(DIR_KEY, JSON.stringify(list));
  }, []);

  return {
    navMode, setNavMode,
    myRole, setMyRole,
    aiRole, setAiRole,
    constraints, setConstraints,
    lenLevel, setLenLevel,
    memoryCtx, setMemoryCtx,
    showRecallPicker, setShowRecallPicker,
    directors, setDirectors, updateDirectors,
    showExport, setShowExport,
  };
}
