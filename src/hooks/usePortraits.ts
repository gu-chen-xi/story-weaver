// 立绘素材的状态（v3.5.6，A 步第三块）。
//
// 归属：`views/PortraitPage`（素材库）、`views/CharsPage`（角色卡显示立绘）、
// 右侧 `DirectorPanel`（当前角色立绘）。三处共用同一份列表，所以做成一个 hook 由 App 调用一次。
//
// 2026-10-26：生成参数页的「🎨 立绘参数」页签已删（参数编辑只留在素材详情），
// 所以 `pParamId` / `pParamForm` 这两个只为它存在的状态一起移除。

import { useCallback, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { PortraitItem } from "../types";

export type UsePortraitsDeps = {
  setMsg: (s: string) => void;
};

export function usePortraits({ setMsg }: UsePortraitsDeps) {
  const [portraits, setPortraits] = useState<PortraitItem[]>([]);
  /** 素材详情弹窗（null = 关闭） */
  const [pDetail, setPDetail] = useState<number | null>(null);
  /** 生成参数页：调试面板选中的条目 */
  const [pDebugId, setPDebugId] = useState<number | null>(null);

  const loadPortraits = useCallback(async () => {
    try {
      setPortraits(await invoke<PortraitItem[]>("portrait_list"));
    } catch (e) {
      console.error(e);
      setMsg("❌ 加载素材失败: " + String(e));
    }
  }, [setMsg]);

  return {
    portraits, loadPortraits,
    pDetail, setPDetail,
    pDebugId, setPDebugId,
  };
}
