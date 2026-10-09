//! hadolint's configuration: the `.hadolint.yaml` keys that bear on the
//! checks (`ignored`, `override`, `trustedRegistries`, `label-schema`,
//! `strict-labels`, `disable-ignore-pragma`, `failure-threshold`,
//! `no-fail`), read the way `Hadolint.Config.Configfile` reads them.

use serde::Deserialize;

/// hadolint's severities, ordered from most to least severe (its `Ord`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Severity {
    Error,
    Warning,
    Info,
    Style,
    Ignore,
}

impl Severity {
    pub fn parse(s: &str) -> Result<Self, String> {
        Ok(match s {
            "error" => Self::Error,
            "warning" => Self::Warning,
            "info" => Self::Info,
            "style" => Self::Style,
            "ignore" | "none" => Self::Ignore,
            other => return Err(format!("Invalid severity: {other}")),
        })
    }

    pub const fn text(self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Warning => "warning",
            Self::Info => "info",
            Self::Style => "style",
            Self::Ignore => "",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LabelType {
    Email,
    GitHash,
    RawText,
    Rfc3339,
    SemVer,
    Spdx,
    Url,
}

impl LabelType {
    pub fn parse(s: &str) -> Result<Self, String> {
        Ok(match s {
            "email" => Self::Email,
            "hash" => Self::GitHash,
            "text" | "" => Self::RawText,
            "rfc3339" => Self::Rfc3339,
            "semver" => Self::SemVer,
            "spdx" => Self::Spdx,
            "url" => Self::Url,
            other => return Err(format!("invalid label type: {other}")),
        })
    }
}

#[derive(Debug, Clone)]
pub struct Config {
    pub no_fail: bool,
    pub error_rules: Vec<String>,
    pub warning_rules: Vec<String>,
    pub info_rules: Vec<String>,
    pub style_rules: Vec<String>,
    pub ignore_rules: Vec<String>,
    pub trusted_registries: Vec<String>,
    /// Sorted by label name, as the original's `Map`.
    pub label_schema: Vec<(String, LabelType)>,
    pub strict_labels: bool,
    pub disable_ignore_pragma: bool,
    pub failure_threshold: Severity,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            no_fail: false,
            error_rules: Vec::new(),
            warning_rules: Vec::new(),
            info_rules: Vec::new(),
            style_rules: Vec::new(),
            ignore_rules: Vec::new(),
            trusted_registries: Vec::new(),
            label_schema: Vec::new(),
            strict_labels: false,
            disable_ignore_pragma: false,
            failure_threshold: Severity::Info,
        }
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct OverrideFile {
    error: Vec<String>,
    warning: Vec<String>,
    info: Vec<String>,
    style: Vec<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct ConfigFile {
    #[serde(rename = "no-fail")]
    no_fail: Option<bool>,
    #[serde(rename = "override")]
    override_: OverrideFile,
    ignored: Vec<String>,
    #[serde(rename = "trustedRegistries")]
    trusted_registries: Vec<String>,
    #[serde(rename = "label-schema")]
    label_schema: std::collections::BTreeMap<String, String>,
    #[serde(rename = "strict-labels")]
    strict_labels: Option<bool>,
    #[serde(rename = "disable-ignore-pragma")]
    disable_ignore_pragma: Option<bool>,
    #[serde(rename = "failure-threshold")]
    failure_threshold: Option<String>,
}

impl Config {
    /// Apply a `.hadolint.yaml` on top of this configuration (the file's
    /// lists extend, its scalars replace).
    pub fn apply_yaml(&mut self, text: &str) -> Result<(), String> {
        let file: ConfigFile = serde_yaml_ng::from_str(text).map_err(|e| format!("{e}"))?;
        if let Some(b) = file.no_fail {
            self.no_fail = b;
        }
        self.error_rules.extend(file.override_.error);
        self.warning_rules.extend(file.override_.warning);
        self.info_rules.extend(file.override_.info);
        self.style_rules.extend(file.override_.style);
        self.ignore_rules.extend(file.ignored);
        self.trusted_registries.extend(file.trusted_registries);
        for (name, ty) in file.label_schema {
            let ty = LabelType::parse(&ty)?;
            self.set_label(name, ty);
        }
        if let Some(b) = file.strict_labels {
            self.strict_labels = b;
        }
        if let Some(b) = file.disable_ignore_pragma {
            self.disable_ignore_pragma = b;
        }
        if let Some(t) = file.failure_threshold {
            self.failure_threshold = Severity::parse(&t)?;
        }
        Ok(())
    }

    pub fn set_label(&mut self, name: String, ty: LabelType) {
        match self.label_schema.binary_search_by(|(n, _)| n.cmp(&name)) {
            Ok(i) => self.label_schema[i].1 = ty,
            Err(i) => self.label_schema.insert(i, (name, ty)),
        }
    }
}
