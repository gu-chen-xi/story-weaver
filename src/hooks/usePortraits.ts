// 立绘素材的状态（v3.5.6，A 步第三块）。
//
// 归属：`views/PortraitPage`（素材库）、`views/ParamsPage`（生成参数页要选底图）、
// `views/CharsPage`（角色卡显示立绘）、右侧 `DirectorPanel`（当前角色立绘）。
// 四个地方共用同一份列表，所以做成一个 hook 由 App 调用一次。
//
// 被搬走的：`portraits` / `loadPortraits` / `pDetail`（详情弹窗）/ `pParamId` /
// `pDebugId` / `pParamForm`（生成参数页的调试与"未开放参数"表单）。

import { useCallback, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { PortraitItem } from "../types";

export type ParamForm = { prompt: string; negative: string; seed: string; steps: string; cfg: string; model: string };

export const EMPTY_PARAM_FORM: ParamForm = { prompt: "", negative: "", seed: "", steps: "", cfg: "", model: "" };

export type UsePortraitsDeps = {
  setMsg: (s: string) => void;
};

export function usePortraits({ setMsg }: UsePortraitsDeps) {
  const [portraits, setPortraits] = useState<PortraitItem[]>([]);
  /** 素材详情弹窗（null = 关闭） */
  const [pDetail, setPDetail] = useState<number | null>(null);
  /** 生成参数页：选中的底图 id */
  const [pParamId, setPParamId] = useState<number | null>(null);
  /** 生成参数页：调试面板选中的条目 */
  const [pDebugId, setPDebugId] = useState<number | null>(null);
  /** 「未开放的生成参数」折叠区表单 */
  const [pParamForm, setPParamForm] = useState<ParamForm>(EMPTY_PARAM_FORM);

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
    pParamId, setPParamId,
    pDebugId, setPDebugId,
    pParamForm, setPParamForm,
  };
}
