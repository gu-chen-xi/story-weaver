import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { PLUGIN_REGISTRY } from "../constants";
import { reportError } from "../lib/errors";
import type { CloudForm, GenDescriptor } from "../plugins";

export function useGenerationChannels() {
  const [pluginState, setPluginState] = useState<Record<string, { enabled: boolean; panelOpen: boolean }>>(
    Object.fromEntries(PLUGIN_REGISTRY.map((d) => [d.id, { enabled: false, panelOpen: false }]))
  );
  const [genList, setGenList] = useState<GenDescriptor[]>([]);
  const [genSel, setGenSel] = useState("local_sdwebui");
  const [cloudForm, setCloudForm] = useState<CloudForm>({
    base_url: "", model: "", url: "", body_template: "", response_path: "", openai_response_path: "",
    auth_style: "bearer", extra_headers: "", api_key: "", has_api_key: false, download_hosts: "",
  });

  const loadCloudConfig = async () => {
    try {
      const c = await invoke<Record<string, unknown>>("generator_get_cloud_config");
      setCloudForm((prev) => ({
        ...prev,
        base_url: String(c.base_url ?? ""),
        model: String(c.model ?? ""),
        url: String(c.url ?? ""),
        body_template: String(c.body_template ?? ""),
        response_path: String(c.response_path ?? ""),
        openai_response_path: String(c.openai_response_path ?? ""),
        auth_style: String(c.auth_style || "bearer"),
        extra_headers: String(c.extra_headers ?? ""),
        download_hosts: String(c.download_hosts ?? ""),
        api_key: "",
        has_api_key: Boolean(c.has_api_key),
      }));
    } catch (e) {
      reportError("读取云端配置", e);
    }
  };

  const loadGenerators = async () => {
    try {
      const list = await invoke<GenDescriptor[]>("generator_list");
      setGenList(list);
      if (list.length > 0 && !list.some((g) => g.id === genSel)) setGenSel(list[0].id);
    } catch (e) {
      reportError("载入生成通道", e);
    }
  };

  return { pluginState, setPluginState, genList, setGenList, genSel, setGenSel, cloudForm, setCloudForm, loadCloudConfig, loadGenerators };
}
