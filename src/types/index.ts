export type Protocol =
  | "anthropic-messages"
  | "openai-completions"
  | "openai-responses";

export type AgentMode = "third_party" | "official";

export type Provider = {
  readonly id: string;
  readonly name: string;
  readonly baseUrl: string;
  readonly protocol: Protocol;
  readonly headers: Readonly<Record<string, string>>;
  readonly compat: Readonly<Record<string, unknown>>;
  readonly enabled: boolean;
  readonly notes: string;
  readonly secretRef: string;
  readonly createdAt: string;
  readonly updatedAt: string;
};

export type ModelCapabilities = {
  readonly reasoning: boolean;
  readonly vision: boolean;
};

export type Model = {
  readonly id: string;
  readonly providerId: string;
  readonly modelId: string;
  readonly displayName: string;
  readonly enabled: boolean;
  readonly capabilities: ModelCapabilities;
  readonly createdAt: string;
  readonly updatedAt: string;
};

export type AgentBindings = {
  readonly claude: {
    readonly mode: AgentMode;
    readonly providerId: string | null;
    readonly modelId: string | null;
    readonly haikuModelId: string | null;
    readonly sonnetModelId: string | null;
    readonly opusModelId: string | null;
  };
  readonly codex: {
    readonly mode: AgentMode;
    readonly providerId: string | null;
    readonly modelId: string | null;
    readonly providerKey: string;
  };
  readonly opencode: {
    readonly providerId: string | null;
    readonly modelId: string | null;
    readonly smallModelId: string | null;
  };
  readonly pi: {
    readonly providerId: string | null;
    readonly modelId: string | null;
  };
};

export type TestPrompt = {
  readonly id: string;
  readonly name: string;
  readonly content: string;
  readonly isDefault: boolean;
  readonly createdAt: string;
  readonly updatedAt: string;
};

/** Last connectivity test for a model row (persisted in store.json). */
export type ModelTestResult = {
  readonly ok: boolean;
  readonly testedAt: string;
  readonly latencyMs?: number | null;
};

export type TestPromptInput = {
  readonly id?: string | null;
  readonly name: string;
  readonly content: string;
};

export type TestConnectionResult = {
  readonly ok: boolean;
  readonly latencyMs: number;
  readonly httpStatus: number | null;
  readonly protocol: Protocol;
  readonly requestUrl: string;
  readonly responseText: string | null;
  readonly error: string | null;
  readonly logs: readonly string[];
  readonly requestMethod: string;
  readonly requestHeaders: readonly string[];
  readonly requestBody: string | null;
  readonly responseHeaders: readonly string[];
  readonly responseBody: string | null;
};

export type Store = {
  readonly version: number;
  readonly providers: readonly Provider[];
  readonly models: readonly Model[];
  readonly agentBindings: AgentBindings;
  readonly testPrompts: readonly TestPrompt[];
  /** modelId -> last test result */
  readonly modelTestResults: Readonly<Record<string, ModelTestResult>>;
};

export type AppConfig = {
  readonly version: number;
  readonly language: string;
  readonly backupKeepCount: number;
  readonly paths: {
    readonly claudeSettings: string | null;
    readonly codexConfig: string | null;
    readonly opencodeConfig: string | null;
    readonly opencodeAuth: string | null;
    readonly piModels: string | null;
    readonly piSettings: string | null;
    readonly piAuth: string | null;
  };
};

export type DetectedPaths = {
  readonly modelhubDir: string;
  readonly claudeSettings: string;
  readonly claudeExists: boolean;
  readonly codexConfig: string;
  readonly codexExists: boolean;
  readonly opencodeConfig: string;
  readonly opencodeExists: boolean;
  readonly piModels: string;
  readonly piExists: boolean;
};

export type FullState = {
  readonly config: AppConfig;
  readonly store: Store;
  readonly secretMasks: Readonly<Record<string, string>>;
  readonly paths: DetectedPaths;
};

export type ProviderInput = {
  readonly name: string;
  readonly baseUrl: string;
  readonly protocol: Protocol;
  readonly apiKey: string;
  readonly headers?: Readonly<Record<string, string>>;
  readonly compat?: Readonly<Record<string, unknown>>;
  readonly enabled?: boolean;
  readonly notes?: string;
};

export type ModelInput = {
  readonly providerId: string;
  readonly modelId: string;
  readonly displayName: string;
  readonly enabled?: boolean;
  readonly capabilities?: ModelCapabilities;
};

export type ApplyAgentResult = {
  readonly agent: string;
  readonly ok: boolean;
  readonly message: string;
  readonly files: readonly string[];
  readonly restartRequired: boolean;
};

export type ApplyResult = {
  readonly results: readonly ApplyAgentResult[];
};

export type ImportPreviewItem = {
  readonly id: string;
  readonly source: string;
  readonly name: string;
  readonly baseUrl: string;
  readonly protocol: Protocol;
  readonly modelCount: number;
  readonly modelIds?: readonly string[];
  readonly extraModelCount?: number;
  readonly newModelIds?: readonly string[];
  readonly existingModelIds?: readonly string[];
  readonly alreadyExists: boolean;
  readonly existingProviderId: string | null;
  readonly existingName: string | null;
  readonly nameConflict: boolean;
  readonly hasApiKey: boolean;
};

export type ImportPreview = {
  readonly items: readonly ImportPreviewItem[];
  readonly scanNotes?: readonly string[];
};

export type ImportAction = "import" | "override" | "skip";

export type ImportItemDecision = {
  readonly id: string;
  readonly name: string;
  readonly action: ImportAction;
};

export type ImportResult = {
  readonly importedProviders: number;
  readonly importedModels: number;
  readonly skipped: number;
  readonly overridden: number;
};

export type RemoteModel = {
  readonly id: string;
  readonly name: string;
};

export type BackupEntry = {
  readonly agent: string;
  readonly stamp: string;
  readonly fileName: string;
  readonly path: string;
};

export type PageId =
  | "providers"
  | "agents"
  | "apply"
  | "import"
  | "backups"
  | "settings";

export const PROTOCOLS = [
  "openai-completions",
  "openai-responses",
  "anthropic-messages",
] as const satisfies readonly Protocol[];

export function emptyBindings(): AgentBindings {
  return {
    claude: {
      mode: "official",
      providerId: null,
      modelId: null,
      haikuModelId: null,
      sonnetModelId: null,
      opusModelId: null,
    },
    codex: {
      mode: "official",
      providerId: null,
      modelId: null,
      providerKey: "modelhub",
    },
    opencode: {
      providerId: null,
      modelId: null,
      smallModelId: null,
    },
    pi: {
      providerId: null,
      modelId: null,
    },
  };
}

/** Drop dangling provider/model refs from a session draft after store mutations. */
export function scrubBindings(
  b: AgentBindings,
  providers: readonly { readonly id: string }[],
  models: readonly { readonly id: string }[],
): AgentBindings {
  const pids = new Set(providers.map((p) => p.id));
  const mids = new Set(models.map((m) => m.id));
  const keepP = (id: string | null): string | null =>
    id && pids.has(id) ? id : null;
  const keepM = (id: string | null): string | null =>
    id && mids.has(id) ? id : null;

  const claudeProviderId = keepP(b.claude.providerId);
  const codexProviderId = keepP(b.codex.providerId);
  const opencodeProviderId = keepP(b.opencode.providerId);
  const piProviderId = keepP(b.pi.providerId);

  return {
    claude: {
      ...b.claude,
      providerId: claudeProviderId,
      modelId: claudeProviderId ? keepM(b.claude.modelId) : null,
      haikuModelId: keepM(b.claude.haikuModelId),
      sonnetModelId: keepM(b.claude.sonnetModelId),
      opusModelId: keepM(b.claude.opusModelId),
    },
    codex: {
      ...b.codex,
      providerId: codexProviderId,
      modelId: codexProviderId ? keepM(b.codex.modelId) : null,
    },
    opencode: {
      providerId: opencodeProviderId,
      modelId: opencodeProviderId ? keepM(b.opencode.modelId) : null,
      smallModelId: opencodeProviderId ? keepM(b.opencode.smallModelId) : null,
    },
    pi: {
      providerId: piProviderId,
      modelId: piProviderId ? keepM(b.pi.modelId) : null,
    },
  };
}
