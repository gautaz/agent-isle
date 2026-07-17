use std::collections::HashMap;

use crate::capability_sources::CapabilitySource;
use crate::config::{AgentConfig, EnvValue, SandboxConfig};
use crate::sandbox::Mount;

/// Config-level mounts and env (from YAML config).
pub struct ConfigSource {
    config: SandboxConfig,
}

impl ConfigSource {
    pub fn new(config: SandboxConfig) -> Self {
        Self { config }
    }
}

impl CapabilitySource for ConfigSource {
    fn mounts(&self) -> Vec<Mount> {
        self.config.mounts.clone()
    }

    fn env(&self) -> HashMap<String, EnvValue> {
        self.config.env.clone()
    }
}

/// Agent-specific mounts and env (from selected agent config).
pub struct AgentSource {
    mounts: Vec<Mount>,
    env: HashMap<String, EnvValue>,
}

impl AgentSource {
    pub fn new(agent: &AgentConfig) -> Self {
        let mounts: Vec<Mount> = agent.mounts.iter().map(|m| m.to_mount()).collect();
        Self {
            mounts,
            env: agent.env.clone(),
        }
    }
}

impl CapabilitySource for AgentSource {
    fn mounts(&self) -> Vec<Mount> {
        self.mounts.clone()
    }

    fn env(&self) -> HashMap<String, EnvValue> {
        self.env.clone()
    }
}
