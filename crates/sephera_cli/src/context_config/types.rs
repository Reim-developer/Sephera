use std::{collections::BTreeMap, path::PathBuf};

use serde::Deserialize;

use crate::{args::ContextFormat, budget::parse_token_budget};

pub const CONFIG_FILE_NAME: &str = ".sephera.toml";
pub const DEFAULT_CONTEXT_BUDGET: u64 = 128_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadedSepheraConfig {
    pub source_path: PathBuf,
    pub context: LoadedContextSection,
    pub profiles: BTreeMap<String, LoadedContextSection>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LoadedContextSection {
    pub ignore: Vec<String>,
    pub focus: Vec<PathBuf>,
    pub diff: Option<String>,
    pub budget: Option<u64>,
    pub format: Option<ContextFormat>,
    pub output: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedContextOptions {
    pub base_path: PathBuf,
    pub ignore: Vec<String>,
    pub focus: Vec<PathBuf>,
    pub diff: Option<String>,
    pub budget: u64,
    pub format: ContextFormat,
    pub output: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AvailableContextProfiles {
    pub source_path: Option<PathBuf>,
    pub profiles: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolvedContextCommand {
    Execute(ResolvedContextOptions),
    ListProfiles(AvailableContextProfiles),
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SepheraToml {
    #[serde(default)]
    pub context: ContextToml,
    #[serde(default)]
    pub profiles: BTreeMap<String, ProfileToml>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContextToml {
    #[serde(default)]
    pub ignore: Vec<String>,
    #[serde(default)]
    pub focus: Vec<PathBuf>,
    pub diff: Option<String>,
    pub budget: Option<TokenBudgetValue>,
    pub format: Option<ContextFormat>,
    pub output: Option<PathBuf>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProfileToml {
    #[serde(default)]
    pub context: ContextToml,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum TokenBudgetValue {
    Integer(u64),
    String(String),
}

impl TokenBudgetValue {
    pub fn parse(self) -> anyhow::Result<u64> {
        match self {
            Self::Integer(value) if value > 0 => Ok(value),
            Self::Integer(_) => {
                anyhow::bail!("token budget must be greater than zero")
            }
            Self::String(value) => parse_token_budget(&value),
        }
    }
}
