// 「🧠 记忆」弹窗（2026-11-18）。
//
// ## 为什么是弹窗
//
// 记忆卡从 v3.16 起带**参与者归属**（`characters: a,b`），所以它天然属于"某个角色"。
// 原来时间线页底部那个总面板（`views/KeyMemories.tsx` 的旧版本）是全故事平铺，
// 看不出"这条是谁的"；现在入口收进**角色详情卡**，点开只看这个角色牵涉的卡 +
// 无归属的全局卡，其它关系组折叠着，想看再展开。
//
// ## 层级（用户特意提过）
//
// 角色详情弹窗是 `zIndex: 60`（`CharsPage.tsx` 的 `charFormOpen` 遮罩）。
// 这个弹窗必须压在它上面 → **70**；同时低于导出/设置弹窗（100）、立绘各种子弹窗
// （120–125）、toast（8500）、全局确认框（9000）。
//
// ⚠ 它**不能**写进角色弹窗那层 880px 面板里 —— 那层有 `overflow: hidden`，会被裁掉。
// 所以它是角色弹窗的**兄弟节点**（`CharsPage` 里单独渲染一个），不做 Portal。
//
// ## 只读感 + 小动作（用户定的口径）
//
// 默认就是浏览：没有大编辑区、没有表单。但「改 / 删 / 承诺徽标 / 导出」四个动作保留 ——
// 理由写在 `KeyMemories.tsx` 的注释里（错记的卡会进提示词；徽标是承诺的界面结算路）。

import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { groupMemories, splitGroups, type MemoryDto } from "../lib/memory";
import { MemoryCards, nextPromiseState, type MemoryActions } from "./KeyMemories";
import { useConfirm } from "../confirm";
import { T } from "../theme";
import { Icon } from "../components/icons";

const btn: React.CSSProperties = {
  fontSize: 11.5,
  padding: "4px 10px",
  borderRadius: 6,
  border: `1px solid ${T.line}`,
  background: T.panel2,
  color: T.text2,
  cursor: "pointer",
  fontFamily: "inherit",
};

export function MemoryModal(props: {
  storyId: string;
  storyTitle: string;
  /// 角色名（记忆卡按 front matter 里的 `characters:` 匹配 —— 存的是名字，不是 id）
  character: string;
  /// 全量缓存：`CharsPage` 进页面时拉过一次的那份 `memory_list`
  items: MemoryDto[];
  /// 写完之后回读（缓存归 CharsPage 管，弹窗不自己存一份）
  onReload: () => Promise<void>;
  onClose: () => void;
  setMsg: (s: string) => void;
}) {
  const confirm = useConfirm();
  const { storyId, storyTitle, character, items, onReload, onClose, setMsg } = props;
  const [busy, setBusy] = useState(false);
  const [othersOpen, setOthersOpen] = useState(false);

  const groups = groupMemories(items);
  const { mine, global, others } = splitGroups(groups, character);
  const mineCount = mine.reduce((n, g) => n + g.items.length, 0);
  const globalCount = global.reduce((n, g) => n + g.items.length, 0);
  const otherCount = others.reduce((n, g) => n + g.items.length, 0);

  const save: MemoryActions["save"] = async (id, body) => {
    try {
      await invoke("memory_update", { storyId, id, body });
      setMsg("✅ 已保存关键记忆");
      await onReload();
    } catch (e) {
      setMsg("❌ 保存失败：" + String(e));
      throw e; // 让卡片保留编辑态（用户刚打的字别吞掉）
    }
  };

  const remove: MemoryActions["remove"] = async (m) => {
    if (!(await confirm({
      title: "删除关键记忆",
      body: `删除这条「${m.label}」？（${m.day_label}）\n\n只删这条文档；剧情本身不受影响。`,
      danger: true,
    }))) return;
    setBusy(true);
    try {
      await invoke("memory_delete", { storyId, id: m.id });
      setMsg("🗑 已删除该条关键记忆");
      await onReload();
    } catch (e) {
      setMsg("❌ 删除失败：" + String(e));
    }
    setBusy(false);
  };

  const togglePromise: MemoryActions["togglePromise"] = async (m) => {
    const next = nextPromiseState(m.promise_state || "待兑现");
    try {
      await invoke("memory_promise_state", { storyId, id: m.id, state: next });
      setMsg(`✅ 承诺状态改成「${next}」`);
      await onReload();
    } catch (e) {
      setMsg("❌ 改承诺状态失败：" + String(e));
    }
  };

  const actions: MemoryActions = { save, remove, togglePromise };

  const exportMine = async () => {
    setBusy(true);
    try {
      // 只导「含她」的卡（全局卡不算她的；要全量去导出全部那条路）
      const [dir, n] = await invoke<[string, number]>("memory_export", {
        storyId,
        storyTitle: storyTitle || "未命名故事",
        character,
      });
      setMsg(n > 0
        ? `✅ 已导出 ${n} 条「${character}」的关键记忆：${dir}`
        : `📂 已建目录（「${character}」名下还没有记忆）：${dir}`);
    } catch (e) {
      setMsg("❌ 导出失败：" + String(e));
    }
    setBusy(false);
  };

  const groupBlock = (g: { label: string; items: MemoryDto[] }, showLabel: boolean) => (
    <div key={g.label || "global"} style={{ marginBottom: 10 }}>
      {showLabel && (
        <div style={{ display: "flex", alignItems: "center", gap: 8, margin: "2px 0 6px", fontSize: 11, color: "#e3c07a" }}>
          <span style={{ fontWeight: 650 }}>{g.label}</span>
          <span style={{ color: T.text3 }}>{g.items.length} 张</span>
        </div>
      )}
      <MemoryCards items={g.items} actions={actions} busy={busy} />
    </div>
  );

  return (
    <div
      onClick={onClose}
      style={{ position: "fixed", inset: 0, background: "rgba(0,0,0,.58)", zIndex: 70, display: "flex", alignItems: "center", justifyContent: "center", padding: 20 }}
    >
      <div
        onClick={(e) => e.stopPropagation()}
        style={{ width: 860, maxWidth: "100%", maxHeight: "84vh", display: "flex", flexDirection: "column", background: T.panel, border: `1px solid ${T.line}`, borderRadius: 14, boxShadow: "0 24px 70px rgba(0,0,0,.6)", overflow: "hidden" }}
      >
        <div style={{ display: "flex", alignItems: "center", gap: 10, padding: "12px 16px", borderBottom: `1px solid ${T.lineSoft}`, flexShrink: 0 }}>
          <b style={{ fontSize: 14, display: "inline-flex", alignItems: "center", gap: 7 }}><Icon name="spark" size={15} />关键记忆 · {character}</b>
          <span style={{ fontSize: 11, color: T.text3 }}>
            全故事 {items.length} 张 · 含「{character}」{mineCount} 张 · 全局 {globalCount} 张
            {otherCount > 0 ? ` · 其它关系组 ${otherCount} 张` : ""}
          </span>
          <div style={{ flex: 1 }} />
          <button style={btn} onClick={() => void exportMine()} disabled={busy || !character} title={`只导出含「${character}」的卡（文件夹名带上角色名）`}>
            ⬇ 导出她的记忆
          </button>
          <button onClick={onClose} style={{ background: "none", border: "none", color: T.text3, cursor: "pointer", fontSize: 16, fontFamily: "inherit" }}>✕</button>
        </div>

        <div style={{ padding: "12px 16px", overflowY: "auto", flex: 1, minHeight: 0 }}>
          <div style={{ fontSize: 11, color: T.text3, lineHeight: 1.7, marginBottom: 12 }}>
            这些是 AI 真正看得到的卡：生成时【关键记忆】只把每张压成一行（预算 1200 字），正文只有这里能看到。
            归属按 v3.16 的「参与者」算 —— a×b 与 b×a 是同一组；没有归属行的卡进「全局」，任何角色都看得见。
            <br />
            「改 / 删 / 承诺徽标」都留着：错记的卡会被注入提示词，必须能删；承诺的「待兑现」也要能在这里结算。
          </div>

          <div style={{ fontSize: 12.5, fontWeight: 650, marginBottom: 8 }}>
            含「{character}」的卡（{mineCount}）
          </div>
          {mine.length === 0 ? (
            <div style={{ fontSize: 11.5, color: T.text3, marginBottom: 14, lineHeight: 1.7 }}>
              还没有她名下的记忆。剧情里第一次出现她参与的 表白 / 承诺 / 牺牲 / 背叛 时，这里会自动多一条。
            </div>
          ) : (
            mine.map((g) => groupBlock(g, true))
          )}

          <div style={{ fontSize: 12.5, fontWeight: 650, marginBottom: 8, marginTop: 14 }}>
            全局（无归属）（{globalCount}）
          </div>
          {globalCount === 0 ? (
            <div style={{ fontSize: 11.5, color: T.text3, marginBottom: 14, lineHeight: 1.7 }}>
              没有无归属的卡。归属行是空的卡（老版本写的、自由模式下记的）会落在这里，任何角色都看得到。
            </div>
          ) : (
            global.map((g) => groupBlock(g, false))
          )}

          {otherCount > 0 && (
            <div style={{ marginTop: 14, borderTop: `1px dashed ${T.lineSoft}`, paddingTop: 10 }}>
              <button
                onClick={() => setOthersOpen(!othersOpen)}
                style={{ ...btn, height: 26, padding: "0 10px", color: T.text3 }}
              >
                {othersOpen ? "收起" : "展开"}其它关系组（{otherCount} 张，与她无关）
              </button>
              {othersOpen && (
                <div style={{ marginTop: 10 }}>
                  {others.map((g) => groupBlock(g, true))}
                </div>
              )}
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
