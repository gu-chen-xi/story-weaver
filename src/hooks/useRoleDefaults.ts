// 「我扮演 / AI 扮演」的默认值与自动纠正（2026-11-08 从 App.tsx 抽出，第五刀）。
//
// 拆出来的理由：这三段 effect 一共 50 行，且**只有本地 state 参与**（没有 invoke、没有 UI），
// 放在 App 里会和别的 effect 混在一起，读起来像一锅粥。
//
// 三条规则（都是用户实测后定的，别随手改）：
//  ① **默认值每个故事只应用一次**：我扮演=男主（没有就保持自由），AI 扮演=女主（没有就挑一个不是男主的）；
//  ② 换故事时把「已应用 / 已校验 / 已备份」三个标记清空，让①②重算；
//  ③ **每次进故事或角色表变化都校验一遍**：全局的扮演角色可能指向上一个故事的角色（或已被删掉的角色），
//     不在本故事角色表里就按默认规则纠正，并把纠正过程告诉用户。

import { useEffect, useRef, type RefObject } from "react";
import type { Character, Story } from "../types";

export type RoleDefaultsArgs = {
  currentStory: Story | null;
  chars: Character[];
  charsLoading: boolean;
  myRole: string;
  aiRole: string;
  setMyRole: (v: string) => void;
  setAiRole: (v: string) => void;
  setMsg: (v: string) => void;
  /** 「首次编辑设定前自动备份一次」的标记，换故事要重置（见 useStorySession） */
  backupTakenRef: RefObject<boolean>;
};

export function useRoleDefaults({
  currentStory, chars, charsLoading, myRole, aiRole, setMyRole, setAiRole, setMsg, backupTakenRef,
}: RoleDefaultsArgs) {
  // 「已经给哪个故事应用过默认值」
  const roleDefaultStory = useRef<number | null>(null);
  // 「上次校验过的组合」—— 避免同一组合重复提示
  const roleFixKey = useRef<string>("");

  // ① 默认值（每个故事只应用一次）
  useEffect(() => {
    if (!currentStory || chars.length === 0) return;
    if (roleDefaultStory.current === currentStory.id) return;
    roleDefaultStory.current = currentStory.id;
    const mine = chars.find((c) => c.role === "男主") ?? null;
    const hers = chars.find((c) => c.role === "女主") ?? null;
    if (mine && myRole === "自由") setMyRole(mine.name);
    const aiPick = hers ?? chars.find((c) => c.name !== (mine?.name ?? "")) ?? null;
    if (aiPick && aiRole === "自由") setAiRole(aiPick.name);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [chars, currentStory?.id]);

  // ② 换故事：三个「只跑一次」的标记全部清空
  useEffect(() => {
    backupTakenRef.current = false;
    roleDefaultStory.current = null;
    roleFixKey.current = "";
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [currentStory?.id]);

  // ③ 扮演角色校验（不在本故事角色表里 → 按默认规则纠正）
  const namesKey = chars.map((c) => c.name).join("\u0001");
  const roleCheckKey = `${currentStory?.id ?? 0}|${charsLoading ? 1 : 0}|${namesKey}|${myRole}|${aiRole}`;
  useEffect(() => {
    if (!currentStory || charsLoading) return;
    if (roleFixKey.current === roleCheckKey) return;
    roleFixKey.current = roleCheckKey;
    const names = new Set(chars.map((c) => c.name));
    const fixed: string[] = [];
    if (myRole !== "自由" && !names.has(myRole)) {
      const mine = chars.find((c) => c.role === "男主") ?? null;
      setMyRole(mine ? mine.name : "自由");
      fixed.push(`「我扮演」${myRole}（不在本故事角色表）→ ${mine ? mine.name : "自由"}`);
    }
    if (aiRole !== "自由" && !names.has(aiRole)) {
      const hers = chars.find((c) => c.role === "女主") ?? null;
      setAiRole(hers ? hers.name : "自由");
      fixed.push(`「AI 扮演」${aiRole}（不在本故事角色表）→ ${hers ? hers.name : "自由"}`);
    }
    if (fixed.length > 0) setMsg(`⚠ 扮演角色已纠正：${fixed.join("；")}`);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [roleCheckKey]);
}
