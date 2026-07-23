import { invoke } from "@tauri-apps/api/core";
import type {
  AgentBindings,
  AppConfig,
  ApplyResult,
  BackupEntry,
  FullState,
  ImportItemDecision,
  ImportPreview,
  ImportResult,
  Model,
  ModelInput,
  Provider,
  ProviderInput,
  RemoteModel,
} from "../types";

export async function getState(): Promise<FullState> {
  return invoke<FullState>("get_state");
}

export async function saveAppConfig(config: AppConfig): Promise<void> {
  return invoke("save_app_config", { config });
}

export async function addProvider(input: ProviderInput): Promise<Provider> {
  return invoke("add_provider", { input });
}

export async function updateProvider(
  id: string,
  input: ProviderInput,
): Promise<Provider> {
  return invoke("update_provider", { id, input });
}

export async function deleteProvider(id: string): Promise<void> {
  return invoke("delete_provider", { id });
}

export async function cloneProvider(
  id: string,
  newName: string,
  newApiKey: string,
): Promise<Provider> {
  return invoke("clone_provider", { id, newName, newApiKey });
}

export async function setProviderEnabled(
  id: string,
  enabled: boolean,
): Promise<void> {
  return invoke("set_provider_enabled", { id, enabled });
}

export async function addModel(input: ModelInput): Promise<Model> {
  return invoke("add_model", { input });
}

export async function updateModel(id: string, input: ModelInput): Promise<Model> {
  return invoke("update_model", { id, input });
}

export async function deleteModel(id: string): Promise<void> {
  return invoke("delete_model", { id });
}

export async function readLiveBindings(): Promise<AgentBindings> {
  return invoke("read_live_bindings");
}

export async function applyConfig(
  agents: readonly string[] = [],
  bindings?: AgentBindings | null,
): Promise<ApplyResult> {
  return invoke("apply_config", {
    request: {
      agents: [...agents],
      bindings: bindings ?? null,
    },
  });
}

export type DiffLine = {
  readonly kind: string;
  readonly text: string;
};

export type AgentDiff = {
  readonly agent: string;
  readonly file: string;
  readonly lines: readonly DiffLine[];
  readonly note: string;
};

export type ApplyPreview = {
  readonly agents: readonly AgentDiff[];
};

export async function previewApply(
  agents: readonly string[] = [],
  bindings?: AgentBindings | null,
): Promise<ApplyPreview> {
  return invoke("preview_apply", {
    request: {
      agents: [...agents],
      bindings: bindings ?? null,
    },
  });
}

export async function previewImport(): Promise<ImportPreview> {
  return invoke("preview_import");
}

export async function runImport(
  items: readonly ImportItemDecision[],
): Promise<ImportResult> {
  return invoke("run_import", { request: { items: [...items] } });
}

export async function listBackups(): Promise<readonly BackupEntry[]> {
  return invoke("list_backups");
}

export async function revealApiKey(secretRef: string): Promise<string> {
  return invoke("reveal_api_key", { secretRef });
}

export async function fetchProviderModels(providerId: string): Promise<readonly RemoteModel[]> {
  return invoke("fetch_provider_models", { providerId });
}

export async function deleteProviders(ids: readonly string[]): Promise<number> {
  return invoke("delete_providers", { ids: [...ids] });
}
