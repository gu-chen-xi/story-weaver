// 临时文件页的状态（v3.5.6，A 步：把 App.tsx 的 state 按页面归拢）。
//
// 归属：`views/TempPage`（数据管理页）+ 设置弹窗里的「打开数据目录」入口。
// 与剧情/角色无关，是最独立的一块，先搬它。

import { useCallback, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
// 类型定义在页面组件里（它同时是 TempPage 的 props 类型），这里复用同一份以免漂移
import type { TempFile } from "../views/TempPage";

export type UseTempFilesDeps = {
  setMsg: (s: string) => void;
};

export function useTempFiles({ setMsg }: UseTempFilesDeps) {
  const [tempFiles, setTempFiles] = useState<TempFile[]>([]);

  const loadTemp = useCallback(async () => {
    try {
      setTempFiles(await invoke<TempFile[]>("generator_list_temp"));
    } catch (e) {
      // 读不到文件列表不是致命错误（目录可能还不存在），不要打扰用户
      console.error(e);
    }
  }, []);

  /** 清理 temp/。后端会保护"5 分钟内修改过"的文件（正在生成中的不能删）。 */
  const cleanTemp = useCallback(async () => {
    try {
      const n = await invoke<number>("generator_cleanup_temp", { keep: [] });
      setMsg(`🗑 已清理 ${n} 个临时文件`);
      await loadTemp();
    } catch (e) {
      setMsg("❌ 清理失败：" + String(e));
    }
  }, [setMsg, loadTemp]);

  return { tempFiles, loadTemp, cleanTemp };
}
