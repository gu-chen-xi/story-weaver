// 「切到某个页面时要拉什么」的集中处（2026-11-04 从 App.tsx 抽出）。
//
// 原来这些 useEffect 散在 App 里、和"对话区跟随"混在一起，读代码时很难看出
// 「哪个视图依赖哪些数据」。集中到一处后，这里就是唯一的答案。

import { useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";

export type UseViewEffectsArgs = {
  view: string;
  /** 只用到 id；不直接把 Story 类型绑进来（首页/工作区的 Story 形状不同） */
  currentStory: { id: number } | null;
  node: { id: number } | null;
  loadRequireApproval: () => unknown;
  loadSnapshots: () => unknown;
  loadAudit: () => unknown;
  loadEventLogs: () => unknown;
  loadDebug: () => unknown;
  /** 故事 id 是数字（不是库里的字符串 id） */
  loadChars: (storyId: number) => unknown;
  debugList: { id: number }[];
  selectedDebug: { id: number } | null;
  setSelectedDebug: (d: any) => void;
  setPDebugId: (id: number | null) => void;
  setCtxWindow: (n: number) => void;
};

export function useViewEffects(a: UseViewEffectsArgs) {
  const { view, currentStory, node } = a;

  // 提案页：拍板开关（与故事无关）+ 快照 / 审计 / 事件日志
  useEffect(() => {
    if (view !== "proposals") return;
    void a.loadRequireApproval();
    if (!currentStory) return;
    void a.loadSnapshots();
    void a.loadAudit();
    void a.loadEventLogs();
  }, [view, currentStory?.id]);

  // 参数页：请求记录 + 上下文窗口配置
  useEffect(() => {
    if (view !== "params") return;
    if (currentStory) a.loadDebug();
    invoke<string | null>("get_config", { key: "model.context_window" })
      .then((v) => { const n = Number(v); if (n > 0) a.setCtxWindow(n); })
      .catch(() => {});
  }, [view, currentStory?.id]);

  // 参数页：选中的请求记录失效时回落到第一条
  useEffect(() => {
    if (view !== "params") return;
    if (a.debugList.length && !a.debugList.some((d) => d.id === a.selectedDebug?.id)) a.setSelectedDebug(a.debugList[0]);
  }, [view, a.debugList, a.selectedDebug]);

  useEffect(() => {
    if (view === "params") a.setPDebugId(null);
  }, [view, node?.id]);

  // 剧情页：节点变了要重拉请求记录（上下文窗口配置只在缺省时补）
  useEffect(() => {
    if (view !== "story" || !currentStory) return;
    a.loadDebug();
    invoke<string | null>("get_config", { key: "model.context_window" })
      .then((v) => { const n = Number(v); if (n > 0) a.setCtxWindow(n); })
      .catch(() => {});
  }, [view, currentStory?.id, node?.id]);

  // 角色页
  useEffect(() => {
    if (view === "chars" && currentStory) a.loadChars(currentStory.id);
  }, [view, currentStory?.id]);
}
