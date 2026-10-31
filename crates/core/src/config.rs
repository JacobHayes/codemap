//! `.codemap/config.toml`: the project's own bar. Optional; a missing file means no rules, no
//! acknowledgements and no role overrides.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::Path;

use codemap_lang::Role;
use ignore::gitignore::{Gitignore, GitignoreBuilder};
use serde::Deserialize;

use crate::measure;
use crate::store::STATE_DIR;

const CONFIG_FILE: &str = "config.toml";

/// Whether `path` is the config of the project at `root`: a change to it changes the index even
/// though the file is not indexed.
#[must_use]
pub fn is_config(root: &Path, path: &Path) -> bool {
    path == root.join(STATE_DIR).join(CONFIG_FILE)
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("cannot read {path}: {source}")]
    Read { path: String, source: io::Error },
    #[error("invalid {path}: {source}")]
    Parse {
        path: String,
        source: Box<toml::de::Error>,
    },
    #[error("rule {rule:?} uses the unknown measure {measure:?}")]
    UnknownMeasure { rule: String, measure: String },
    #[error("rule {rule:?} needs at_least or at_most")]
    NoThreshold { rule: String },
    #[error("[roles] maps {glob:?} to the unknown role {role:?}")]
    UnknownRole { glob: String, role: String },
    #[error("[roles] glob {glob:?} is not valid: {source}")]
    BadGlob { glob: String, source: ignore::Error },
}

/// What a rule is measured per.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Granularity {
    File,
    Function,
    Directory,
}

/// A threshold on one catalog measure at one granularity.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rule {
    pub name: String,
    /// A name from the measure catalog.
    pub measure: String,
    pub per: Granularity,
    pub at_least: Option<f64>,
    pub at_most: Option<f64>,
}

/// An area whose complexity is essential: it stays visible but ranks after the rest.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Acknowledgement {
    /// A file or directory, relative to the root.
    pub path: String,
    /// Only this detector's findings; every detector's when absent.
    pub detector: Option<String>,
    pub reason: String,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default, rename = "rule")]
    pub rules: Vec<Rule>,
    #[serde(default, rename = "acknowledge")]
    pub acknowledgements: Vec<Acknowledgement>,
    /// Gitignore-style glob to role name.
    #[serde(default)]
    pub roles: BTreeMap<String, String>,
    /// The file's text, for change detection.
    #[serde(skip)]
    pub text: String,
}

impl Config {
    /// Reads `<root>/.codemap/config.toml`; an absent file is the empty config.
    ///
    /// # Errors
    /// Fails when the file cannot be read or parsed, or names an unknown measure or role.
    pub fn load(root: &Path) -> Result<Self, ConfigError> {
        let path = root.join(STATE_DIR).join(CONFIG_FILE);
        let display = path.display().to_string();
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(source) => {
                return Err(ConfigError::Read {
                    path: display,
                    source,
                });
            }
        };
        let mut config: Self = toml::from_str(&text).map_err(|source| ConfigError::Parse {
            path: display,
            source: Box::new(source),
        })?;
        config.text = text;
        config.validate()?;
        Ok(config)
    }

    fn validate(&self) -> Result<(), ConfigError> {
        for rule in &self.rules {
            if measure::named(&rule.measure).is_none() {
                return Err(ConfigError::UnknownMeasure {
                    rule: rule.name.clone(),
                    measure: rule.measure.clone(),
                });
            }
            if rule.at_least.is_none() && rule.at_most.is_none() {
                return Err(ConfigError::NoThreshold {
                    rule: rule.name.clone(),
                });
            }
        }
        RoleOverrides::new(&self.roles).map(drop)
    }
}

/// The `[roles]` globs, compiled. The first match in glob order wins.
pub struct RoleOverrides {
    matchers: Vec<(Gitignore, Role)>,
}

impl RoleOverrides {
    /// Compiles the globs.
    ///
    /// # Errors
    /// Fails on a glob that does not parse or a role that does not exist.
    pub fn new(roles: &BTreeMap<String, String>) -> Result<Self, ConfigError> {
        let mut matchers = Vec::new();
        for (glob, name) in roles {
            let role = Role::parse(name).ok_or_else(|| ConfigError::UnknownRole {
                glob: glob.clone(),
                role: name.clone(),
            })?;
            let bad = |source| ConfigError::BadGlob {
                glob: glob.clone(),
                source,
            };
            let mut builder = GitignoreBuilder::new("");
            builder.add_line(None, glob).map_err(bad)?;
            matchers.push((builder.build().map_err(bad)?, role));
        }
        Ok(Self { matchers })
    }

    /// The overriding role for a path relative to the root, if any glob matches.
    #[must_use]
    pub fn role_for(&self, path: &str) -> Option<Role> {
        self.matchers
            .iter()
            .find(|(matcher, _)| matcher.matched_path_or_any_parents(path, false).is_ignore())
            .map(|&(_, role)| role)
    }
}
