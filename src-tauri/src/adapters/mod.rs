mod claude;
mod codex;
mod fetch_models;
mod import;
mod live;
mod opencode;
mod pi;
mod preview;
mod util;

pub use fetch_models::fetch_remote_models;
pub use import::{import_from_agents, preview_import};
pub use live::read_live_bindings;
pub use preview::{preview_apply, ApplyPreview};

use anyhow::Result;

use crate::backup;
use crate::paths::ModelHubPaths;
use crate::store::{
    AgentMode, ApplyAgentResult, ApplyRequest, ApplyResult, StoreService,
};

pub fn apply_all(svc: &StoreService, paths: &ModelHubPaths, req: ApplyRequest) -> Result<ApplyResult> {
    let config = svc.load_config()?;
    let mut store = svc.load_store()?;
    if let Some(bindings) = req.bindings {
        store.agent_bindings = bindings;
    }
    let secrets = svc.load_secrets()?;
    let keep = config.backup_keep_count;

    let selected: Vec<&str> = if req.agents.is_empty() {
        vec!["claude", "codex", "opencode", "pi"]
    } else {
        req.agents.iter().map(|s| s.as_str()).collect()
    };

    let mut results = Vec::new();
    for agent in selected {
        let result = match agent {
            "claude" => claude::apply(svc, paths, &config, &store, &secrets, keep),
            "codex" => codex::apply(svc, paths, &config, &store, &secrets, keep),
            "opencode" => opencode::apply(svc, paths, &config, &store, &secrets, keep),
            "pi" => pi::apply(svc, paths, &config, &store, &secrets, keep),
            other => Ok(ApplyAgentResult {
                agent: other.to_string(),
                ok: false,
                message: format!("unknown agent: {other}"),
                files: vec![],
                restart_required: false,
            }),
        };
        match result {
            Ok(r) => results.push(r),
            Err(e) => results.push(ApplyAgentResult {
                agent: agent.to_string(),
                ok: false,
                message: e.to_string(),
                files: vec![],
                restart_required: false,
            }),
        }
    }
    Ok(ApplyResult { results })
}

pub fn official_mode_label(mode: &AgentMode) -> &'static str {
    match mode {
        AgentMode::Official => "official",
        AgentMode::ThirdParty => "third_party",
    }
}

pub(crate) fn backup_before_write(
    paths: &ModelHubPaths,
    agent: &str,
    file: &std::path::Path,
    keep: u32,
) -> Result<()> {
    let _ = backup::backup_file(paths, agent, file, keep)?;
    Ok(())
}
