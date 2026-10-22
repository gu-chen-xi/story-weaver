// 合规与责任：知情同意的状态与写入（从 App.tsx 抽出，v3.4）。
//
// 抽出来的理由：这块的逻辑是"版本号比对"，不是"布尔开关"——
// 曾经只存时间戳，条款改版后旧时间戳依然算数，用户"同意"的其实是旧条款。
// 单独成文件后，这段判断和它的理由放在一起，不容易被简化成 `if (acked)`。

import { useState, type Dispatch, type SetStateAction } from "react";
import { invoke } from "@tauri-apps/api/core";
import { NOTICE_VERSION } from "../compliance";
import { nowStamp } from "../lib/time";

export type UseComplianceDeps = {
  setMsg: Dispatch<SetStateAction<string>>;
};

export function useCompliance(deps: UseComplianceDeps) {
  const { setMsg } = deps;
  const [complianceAck, setComplianceAck] = useState(false);
  const [complianceNotice, setComplianceNotice] = useState(false);
  const [noticeAgreed, setNoticeAgreed] = useState(false);   // 首次启动弹窗必须勾选才能继续
  const [checklistDone, setChecklistDone] = useState<Set<string>>(new Set());

  const loadComplianceAck = async () => {
    try {
      // 知情同意的判定标准：**确认过的条款版本 == 当前条款版本**。
      // 只存时间戳是不够的：条款改版后旧时间戳依然算数，用户"同意"的其实是旧条款。
      const ver = await invoke<string | null>("get_config", { key: "compliance.acknowledged_version" });
      const ok = (ver ?? "").trim() === NOTICE_VERSION;
      setComplianceAck(ok);
      // 未确认当前版本（含"从没确认过"与"确认的是旧版本"两种情况）→ 弹窗
      if (!ok) setComplianceNotice(true);
    } catch { /* 读不到就不打扰 */ }
  };

  /** 首次启动合规提示：一次性，确认或关闭后写入 compliance.notice_seen */
  const dismissComplianceNotice = async () => {
    setComplianceNotice(false);
    try {
      await invoke("set_config", { key: "compliance.acknowledged_version", value: NOTICE_VERSION });
      await invoke("set_config", { key: "compliance.notice_seen", value: nowStamp() });
      setComplianceAck(true);
      setMsg("✅ 已记录：你已阅读并同意当前条款（版本 " + NOTICE_VERSION + "）");
    } catch { /* 忽略 */ }
  };

  const ackCompliance = async () => {
    try {
      await invoke("set_config", { key: "compliance.acknowledged_at", value: nowStamp() });
      await invoke("set_config", { key: "compliance.acknowledged_version", value: NOTICE_VERSION });
      setComplianceAck(true);
      setMsg("✅ 已记录：你已阅读合规与责任说明");
    } catch (e) { setMsg("❌ 保存失败: " + String(e)); }
  };

  return {
    complianceAck, setComplianceAck,
    complianceNotice, setComplianceNotice,
    noticeAgreed, setNoticeAgreed,
    checklistDone, setChecklistDone,
    loadComplianceAck, dismissComplianceNotice, ackCompliance,
  };
}
