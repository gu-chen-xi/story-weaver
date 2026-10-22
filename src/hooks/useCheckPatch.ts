// 「🔍 检查 → 一键替换」的整套状态与流程（从 App.tsx 抽出，v3.4）。
//
// 抽出来的理由：
//   ① 这是 App.tsx 里**最长的一段业务逻辑**（8 个 state + 5 个函数 + 3 个纯映射表），
//      而且它自成闭环：跑检查 → 拿建议 → 勾选 → 二次确认 → 落地 → 刷新；
//   ② 它是唯一"AI 输出会真的改数据"的入口，独立成文件后，
//      改这段代码时能一眼看到那三道闸门（本地规则先行 / 前端二次确认 / 后端白名单）；
//   ③ App.tsx 有行数守卫，拆出来的东西还能被单测。
//
// **逻辑与拆分前逐行一致**（没有任何顺手改动）。

import { useState, type Dispatch, type SetStateAction } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { CheckItemT, Character, PatchItemT, Relation, Story, WorldSetting } from "../types";

export type CheckBoard = "characters" | "world";

export type UseCheckPatchDeps = {
  currentStory: Story | null;
  chars: Character[];
  relations: Relation[];
  worlds: WorldSetting[];
  setMsg: Dispatch<SetStateAction<string>>;
  loadChars: (storyId: number) => Promise<void>;
  loadWorlds: (storyId: number) => Promise<void>;
  loadRelations: (storyId: number) => Promise<void>;
};

/// 由字段归属推断名词（角色 / 关系 / 条目），供确认框与卡片共用
export function targetNounOf(pp: PatchItemT): string {
  const RELF = ["relation_type", "strength", "state", "description"];
  const CHARF = ["name", "alias", "role", "gender", "age", "one_liner", "profile", "speech_style", "traits"];
  if (RELF.includes(pp.field) || pp.target_kind === "relation" || pp.target_name.includes("→")) return "关系";
  if (CHARF.includes(pp.field)) return "角色";
  return "条目";
}

/// 二次确认对话框里用的字段名（与弹窗内保持一致；以 patch 的 board 为准）
export function fieldLabelOf(pp: PatchItemT): string {
  const CHAR: Record<string, string> = {
    name: "角色名", alias: "别名", role: "定位", gender: "性别", age: "年龄",
    one_liner: "一句话身份", profile: "人物小传", speech_style: "说话方式", traits: "性格",
  };
  const REL: Record<string, string> = {
    relation_type: "关系类型", strength: "关系数值", state: "关系状态", description: "关系说明", direction: "关系方向",
  };
  const WORLD: Record<string, string> = {
    value: "内容", category: "分类", attitude: "对主角态度", scale: "规模", leader: "头目", control_range: "控制范围",
  };
  const owner = (pp.field in CHAR) ? "char" : (pp.field in REL) ? "rel" : (pp.field in WORLD) ? "world" : "unknown";
  if (owner === "char") return CHAR[pp.field];
  if (owner === "rel") return REL[pp.field];
  if (owner === "world") return WORLD[pp.field];
  return `未知字段「${pp.field}」`;
}

export function useCheckPatch(deps: UseCheckPatchDeps) {
  const { currentStory, chars, relations, worlds, setMsg, loadChars, loadWorlds, loadRelations } = deps;

  const [checkBoard, setCheckBoard] = useState<CheckBoard | null>(null);
  const [checkBusy, setCheckBusy] = useState(false);
  const [checkItems, setCheckItems] = useState<CheckItemT[]>([]);
  const [checkPatch, setCheckPatch] = useState<PatchItemT[]>([]);
  const [checkPick, setCheckPick] = useState<Set<number>>(new Set());   // 一键替换：逐条勾选（默认全选）
  const [checkPrev, setCheckPrev] = useState("");       // 上一轮 AI 建议（只带这一份，不带多轮上下文）
  const [checkInput, setCheckInput] = useState("");     // 「继续对话」的用户输入
  const [checkNote, setCheckNote] = useState("");       // 失败 / 降级说明

  // 组装板块内容（给 AI 读的原文；用 JSON 便于模型准确对应字段）
  const buildBoardContent = (board: CheckBoard): string => {
    if (board === "characters") {
      const cs = chars.map((c) => ({
        名称: c.name, 别名: c.alias || "", 定位: c.role || "", 性别: c.gender || "", 年龄: c.age || "",
        身份: c.one_liner || "", 人物小传: c.profile || "", 说话方式: c.speech_style || "", 性格: c.traits || "",
      }));
      const rs = relations.map((r) => {
        const f = chars.find((c) => c.id === r.from_character_id)?.name || `#${r.from_character_id}`;
        const tt = chars.find((c) => c.id === r.to_character_id)?.name || `#${r.to_character_id}`;
        return { 关系: `${f}→${tt}`, 关系类型: r.relation_type || "", 方向: r.direction || "", 数值: r.strength, 状态: r.state || "", 说明: r.description || "" };
      });
      return JSON.stringify({ 角色: cs, 关系: rs }, null, 1);
    }
    const ws = worlds.map((w) => ({
      名称: w.key, 分类: w.category || "", 内容: w.value || "",
      硬规则: w.is_hard_rule ? "是" : "否", 备注: w.meta_json || "",
    }));
    return JSON.stringify({ 世界设定: ws }, null, 1);
  };

  // 点「🔍 检查」：先跑本地规则（不花 token），再跑 AI（查语法/歧义/数值矛盾）
  const runCheck = async (board: CheckBoard) => {
    if (!currentStory) return;
    setCheckBoard(board); setCheckBusy(true); setCheckItems([]); setCheckPatch([]);
    setCheckPrev(""); setCheckInput(""); setCheckNote(""); setCheckPick(new Set());
    const all: CheckItemT[] = [];
    try {
      const local = await invoke<{ items: CheckItemT[] }>("check_board_struct", { storyId: String(currentStory.id), board });
      all.push(...(local.items || []));
      setCheckItems([...all]);
    } catch (e) {
      setCheckNote("本地规则检查失败：" + String(e));
    }
    try {
      const rep = await invoke<{ items: CheckItemT[]; patch: PatchItemT[]; note?: string }>("check_board_ai", {
        storyId: String(currentStory.id), board,
        content: buildBoardContent(board), previous: "", userInput: "",
      });
      all.push(...(rep.items || []));
      setCheckItems([...all]);
      setCheckPatch(rep.patch || []);
      setCheckPick(new Set((rep.patch || []).map((_, i) => i)));
      setCheckPrev(JSON.stringify({ problems: rep.items || [], patch: rep.patch || [] }, null, 1));
      if (rep.note) setCheckNote(rep.note);
    } catch (e) {
      // AI 检查失败不影响本地结论；提示降级
      setCheckNote("AI 检查失败（仅显示本地规则结果）：" + String(e));
    }
    setCheckBusy(false);
  };

  // 「继续对话」：带 板块内容 + 上一轮建议 + 用户输入，让 AI 输出更新后的完整建议集
  const continueCheck = async () => {
    if (!currentStory || !checkBoard) return;
    setCheckBusy(true); setCheckNote("");
    try {
      const rep = await invoke<{ items: CheckItemT[]; patch: PatchItemT[]; note?: string }>("check_board_ai", {
        storyId: String(currentStory.id), board: checkBoard,
        content: buildBoardContent(checkBoard), previous: checkPrev, userInput: checkInput,
      });
      // 本轮 AI 结果替换上一轮 AI 部分：重新跑一次本地规则，保证结构问题不丢
      const local = await invoke<{ items: CheckItemT[] }>("check_board_struct", { storyId: String(currentStory.id), board: checkBoard });
      setCheckItems([...(local.items || []), ...(rep.items || [])]);
      setCheckPatch(rep.patch || []);
      setCheckPick(new Set((rep.patch || []).map((_, i) => i)));
      setCheckPrev(JSON.stringify({ problems: rep.items || [], patch: rep.patch || [] }, null, 1));
      setCheckInput("");
      if (rep.note) setCheckNote(rep.note);
    } catch (e) {
      setCheckNote("继续对话失败：" + String(e));
    }
    setCheckBusy(false);
  };

  // 「一键替换」：二次确认 → 白名单校验（后端）→ 执行 → 刷新
  const applyCheckPatch = async () => {
    if (!currentStory || checkPatch.length === 0) return;
    const picked = checkPatch.filter((_, i) => checkPick.has(i));
    if (picked.length === 0) { setMsg("⚠ 没有勾选任何条目"); return; }
    const lines = picked.map((p, i) => {
      const fl = fieldLabelOf(p);
      // 模型常把字段名写错（软件会改判）—— 写错时在确认框里明说，避免用户以为改的是别的地方
      const note = fl.startsWith("未知字段") ? "（字段名由软件按内容自动定位）" : "";
      return `${i + 1}. ${targetNounOf(p)}「${p.target_name}」· ${fl}${note}`;
    }).join("\n");
    if (!confirm(`将要修改 ${picked.length} 条：\n\n${lines}\n\n（执行前会自动存一个设定版本，可回滚）\n确定执行？`)) return;
    setCheckBusy(true);
    try {
      const done = await invoke<string[]>("apply_check_patch", { storyId: String(currentStory.id), items: picked });
      setMsg(`✅ 已替换 ${done.length} 条：${done.join("；")}`);
      setCheckBoard(null);
      await loadChars(currentStory.id);
      await loadWorlds(currentStory.id);
      await loadRelations(currentStory.id);
    } catch (e) {
      setMsg("❌ 替换失败（未做任何修改或已中止）：" + String(e));
    }
    setCheckBusy(false);
  };

  return {
    checkBoard, setCheckBoard, checkBusy, checkItems, setCheckItems,
    checkPatch, setCheckPatch, checkPick, setCheckPick,
    checkPrev, checkInput, setCheckInput, checkNote,
    targetNounOf, fieldLabelOf, buildBoardContent,
    runCheck, continueCheck, applyCheckPatch,
  };
}
