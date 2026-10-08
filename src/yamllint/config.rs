//! yamllint configuration (`.yamllint.yaml`), mirroring yamllint's
//! `config.py`: `extends`, per-rule `enable`/`disable`/settings, `level`,
//! option validation against each rule's schema, defaults, and the `ignore`
//! patterns at the top level and per rule.
//!
//! Two keys are handled differently from yamllint. `yaml-files` is accepted
//! and ignored: rsconstruct decides which files a processor scans. `locale`
//! is rejected: it changes how `key-ordering` collates, and this port
//! compares code points only.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Context, Result, bail};
use ignore::gitignore::{Gitignore, GitignoreBuilder};
use serde_yaml_ng::Value;

use super::Level;
use super::rules::{self, Alt, Default as OptDefault, OptKind, RuleSpec};

/// yamllint's `conf/default.yaml`, the base every config extends.
pub const DEFAULT_CONF: &str = "---

yaml-files:
  - '*.yaml'
  - '*.yml'
  - '.yamllint'

rules:
  anchors: enable
  braces: enable
  brackets: enable
  colons: enable
  commas: enable
  comments:
    level: warning
  comments-indentation:
    level: warning
  document-end: disable
  document-start:
    level: warning
  empty-lines: enable
  empty-values: disable
  float-values: disable
  hyphens: enable
  indentation: enable
  key-duplicates: enable
  key-ordering: disable
  line-length: enable
  new-line-at-end-of-file: enable
  new-lines: enable
  octal-values: disable
  quoted-strings: disable
  trailing-spaces: enable
  truthy:
    level: warning
";

/// yamllint's `conf/relaxed.yaml`.
pub const RELAXED_CONF: &str = "---

extends: default

rules:
  braces:
    level: warning
    max-spaces-inside: 1
  brackets:
    level: warning
    max-spaces-inside: 1
  colons:
    level: warning
  commas:
    level: warning
  comments: disable
  comments-indentation: disable
  document-start: disable
  empty-lines:
    level: warning
  hyphens:
    level: warning
  indentation:
    level: warning
    indent-sequences: consistent
  line-length:
    level: warning
    allow-non-breakable-inline-mappings: true
  truthy: disable
";

/// A rule option's value, as YAML gives it.
#[derive(Debug, Clone, PartialEq)]
pub enum OptValue {
    Null,
    Bool(bool),
    Int(i64),
    Str(String),
    List(Vec<Self>),
}

impl OptValue {
    fn from_yaml(value: &Value) -> Result<Self> {
        Ok(match value {
            Value::Null => Self::Null,
            Value::Bool(b) => Self::Bool(*b),
            Value::Number(n) => match n.as_i64() {
                Some(i) => Self::Int(i),
                None => bail!("invalid config: {n} is not an integer"),
            },
            Value::String(s) => Self::Str(s.clone()),
            Value::Sequence(items) => Self::List(
                items
                    .iter()
                    .map(Self::from_yaml)
                    .collect::<Result<Vec<_>>>()?,
            ),
            Value::Mapping(_) | Value::Tagged(_) => {
                bail!("invalid config: a rule option cannot be a mapping")
            }
        })
    }

    pub const fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(b) => Some(*b),
            _ => None,
        }
    }

    pub const fn as_int(&self) -> Option<i64> {
        match self {
            Self::Int(i) => Some(*i),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::Str(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_list(&self) -> Option<&[Self]> {
        match self {
            Self::List(items) => Some(items),
            _ => None,
        }
    }
}

impl std::fmt::Display for OptValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Null => write!(f, "null"),
            Self::Bool(b) => write!(f, "{b}"),
            Self::Int(i) => write!(f, "{i}"),
            Self::Str(s) => write!(f, "'{s}'"),
            Self::List(items) => {
                write!(f, "[")?;
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{item}")?;
                }
                write!(f, "]")
            }
        }
    }
}

impl std::fmt::Display for Alt {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AnyBool => write!(f, "bool"),
            Self::AnyInt => write!(f, "int"),
            Self::AnyStr => write!(f, "str"),
            Self::Bool(b) => write!(f, "{b}"),
            Self::Str(s) => write!(f, "'{s}'"),
        }
    }
}

impl Alt {
    /// Python's `value in alternatives or type(value) in alternatives`.
    fn matches(&self, value: &OptValue) -> bool {
        match (self, value) {
            (Self::AnyBool, OptValue::Bool(_))
            | (Self::AnyInt, OptValue::Int(_))
            | (Self::AnyStr, OptValue::Str(_)) => true,
            (Self::Bool(want), OptValue::Bool(got)) => want == got,
            (Self::Str(want), OptValue::Str(got)) => want == got,
            _ => false,
        }
    }
}

fn alts_to_string(alts: &[Alt]) -> String {
    let parts: Vec<String> = alts.iter().map(ToString::to_string).collect();
    format!("({})", parts.join(", "))
}

/// A rule's validated settings: its level, its options (every option of the
/// rule's schema is present, defaults filled in) and its own ignore patterns.
#[derive(Debug, Clone, Default)]
pub struct RuleConf {
    level: Option<Level>,
    ignore: Option<Gitignore>,
    options: BTreeMap<String, OptValue>,
}

impl RuleConf {
    /// The level, `error` unless the config says `warning`.
    pub fn level(&self) -> Level {
        self.level.unwrap_or(Level::Error)
    }

    /// An option by name. Validation guarantees every option of the rule's
    /// schema is present, so a miss is a programming error.
    pub fn opt(&self, name: &str) -> &OptValue {
        self.options
            .get(name)
            .unwrap_or_else(|| panic!("yamllint rule option {name:?} missing after validation"))
    }

    pub fn bool_opt(&self, name: &str) -> bool {
        self.opt(name)
            .as_bool()
            .unwrap_or_else(|| panic!("yamllint rule option {name:?} is not a bool"))
    }

    pub fn int_opt(&self, name: &str) -> i64 {
        self.opt(name)
            .as_int()
            .unwrap_or_else(|| panic!("yamllint rule option {name:?} is not an int"))
    }

    pub fn str_opt(&self, name: &str) -> &str {
        self.opt(name)
            .as_str()
            .unwrap_or_else(|| panic!("yamllint rule option {name:?} is not a string"))
    }

    /// A list option whose items are strings.
    pub fn str_list_opt(&self, name: &str) -> Vec<&str> {
        self.opt(name)
            .as_list()
            .unwrap_or_else(|| panic!("yamllint rule option {name:?} is not a list"))
            .iter()
            .filter_map(OptValue::as_str)
            .collect()
    }

    /// Whether the rule's own `ignore` patterns exclude `path`.
    pub fn ignores(&self, path: &Path) -> bool {
        self.ignore.as_ref().is_some_and(|gi| is_ignored(gi, path))
    }

    /// Python's `dict.update`: every setting present in `other` replaces
    /// this one's.
    fn update(&mut self, other: &Self) {
        if other.level.is_some() {
            self.level = other.level;
        }
        if other.ignore.is_some() {
            self.ignore.clone_from(&other.ignore);
        }
        for (k, v) in &other.options {
            self.options.insert(k.clone(), v.clone());
        }
    }
}

/// One rule as the config names it.
#[derive(Debug, Clone)]
enum RawRule {
    Disabled,
    Settings(RuleConf),
}

/// A parsed config, before or after validation.
#[derive(Debug, Clone, Default)]
struct Parsed {
    rules: BTreeMap<String, RawRule>,
    ignore: Option<Gitignore>,
}

fn gitignore_from_lines<'a>(
    base_dir: &Path,
    lines: impl IntoIterator<Item = &'a str>,
) -> Result<Gitignore> {
    let mut builder = GitignoreBuilder::new(base_dir);
    for line in lines {
        builder
            .add_line(None, line)
            .with_context(|| format!("invalid config: bad ignore pattern {line:?}"))?;
    }
    builder
        .build()
        .context("invalid config: bad ignore patterns")
}

fn is_ignored(gitignore: &Gitignore, path: &Path) -> bool {
    gitignore
        .matched_path_or_any_parents(path, false)
        .is_ignore()
}

/// `ignore` as yamllint accepts it: a multi-line string or a list of strings.
fn gitignore_from_value(base_dir: &Path, value: &Value) -> Result<Gitignore> {
    match value {
        Value::String(s) => gitignore_from_lines(base_dir, s.lines()),
        Value::Sequence(items) if items.iter().all(Value::is_string) => {
            gitignore_from_lines(base_dir, items.iter().filter_map(Value::as_str))
        }
        _ => bail!("invalid config: ignore should contain file patterns"),
    }
}

/// `ignore-from-file`: one file name or a list of them, each holding
/// patterns one per line, resolved relative to `base_dir`.
fn gitignore_from_files(base_dir: &Path, value: &Value, what: &str) -> Result<Gitignore> {
    let names: Vec<&str> = match value {
        Value::String(s) => vec![s.as_str()],
        Value::Sequence(items) if items.iter().all(Value::is_string) => {
            items.iter().filter_map(Value::as_str).collect()
        }
        _ => bail!(
            "invalid config: ignore-from-file should contain {what}, either as a list or string"
        ),
    };
    let mut lines = Vec::new();
    for name in names {
        let path = base_dir.join(name);
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("Failed to read ignore-from-file {}", path.display()))?;
        lines.extend(text.lines().map(str::to_owned));
    }
    gitignore_from_lines(base_dir, lines.iter().map(String::as_str))
}

fn parse_level(value: &Value) -> Result<Level> {
    match value.as_str() {
        Some("error") => Ok(Level::Error),
        Some("warning") => Ok(Level::Warning),
        _ => bail!("invalid config: level should be \"error\" or \"warning\""),
    }
}

fn parse_rule_settings(base_dir: &Path, map: &serde_yaml_ng::Mapping) -> Result<RuleConf> {
    let mut conf = RuleConf::default();
    let mut ignore = None;
    let mut ignore_from_file = None;
    for (key, value) in map {
        let Some(key) = key.as_str() else {
            bail!("invalid config: rule option names must be strings")
        };
        match key {
            "level" => conf.level = Some(parse_level(value)?),
            "ignore" => ignore = Some(gitignore_from_value(base_dir, value)?),
            "ignore-from-file" => {
                ignore_from_file =
                    Some(gitignore_from_files(base_dir, value, "valid filename(s)")?);
            }
            _ => {
                conf.options
                    .insert(key.to_owned(), OptValue::from_yaml(value)?);
            }
        }
    }
    // Like yamllint, `ignore-from-file` wins when a rule carries both.
    conf.ignore = ignore_from_file.or(ignore);
    Ok(conf)
}

fn parse(text: &str, base_dir: &Path) -> Result<Parsed> {
    let conf: Value =
        serde_yaml_ng::from_str(text).map_err(|e| anyhow::anyhow!("invalid config: {e}"))?;
    let Value::Mapping(conf) = &conf else {
        bail!("invalid config: not a mapping")
    };

    let mut parsed = Parsed::default();
    match conf.get("rules") {
        None => {}
        Some(Value::Mapping(rules)) => {
            for (key, value) in rules {
                let Some(id) = key.as_str() else {
                    bail!("invalid config: rule names must be strings")
                };
                let raw = match value {
                    Value::String(s) if s == "enable" => RawRule::Settings(RuleConf::default()),
                    Value::String(s) if s == "disable" => RawRule::Disabled,
                    Value::Mapping(map) => RawRule::Settings(parse_rule_settings(base_dir, map)?),
                    _ => bail!(
                        "invalid config: rule \"{id}\": should be either \"enable\", \
                         \"disable\" or a mapping"
                    ),
                };
                parsed.rules.insert(id.to_owned(), raw);
            }
        }
        Some(_) => bail!("invalid config: rules should be a mapping"),
    }

    if let Some(extends) = conf.get("extends") {
        let Some(name) = extends.as_str() else {
            bail!("invalid config: extends should be a string")
        };
        let base = match name {
            "default" => load(DEFAULT_CONF, base_dir)?,
            "relaxed" => load(RELAXED_CONF, base_dir)?,
            path => {
                let path = base_dir.join(path);
                let text = std::fs::read_to_string(&path).with_context(|| {
                    format!("Failed to read extended yamllint config {}", path.display())
                })?;
                load(&text, base_dir)
                    .with_context(|| format!("in extended config {}", path.display()))?
            }
        };
        parsed.extend(base);
    }

    match (conf.get("ignore"), conf.get("ignore-from-file")) {
        (Some(_), Some(_)) => {
            bail!("invalid config: ignore and ignore-from-file keys cannot be used together")
        }
        (None, Some(value)) => {
            parsed.ignore = Some(gitignore_from_files(base_dir, value, "filename(s)")?);
        }
        (Some(value), None) => parsed.ignore = Some(gitignore_from_value(base_dir, value)?),
        (None, None) => {}
    }

    if let Some(value) = conf.get("yaml-files") {
        let is_list_of_str = value
            .as_sequence()
            .is_some_and(|items| items.iter().all(Value::is_string));
        if !is_list_of_str {
            bail!("invalid config: yaml-files should be a list of file patterns");
        }
    }

    if conf.get("locale").is_some() {
        bail!(
            "invalid config: locale is not supported by iyamllint \
             (key-ordering compares code points)"
        );
    }

    Ok(parsed)
}

impl Parsed {
    /// Mirrors `YamlLintConfig.extend`: settings merge into the base rule's,
    /// anything else replaces it.
    fn extend(&mut self, mut base: Self) {
        for (id, raw) in std::mem::take(&mut self.rules) {
            match (&raw, base.rules.get_mut(&id)) {
                (RawRule::Settings(own), Some(RawRule::Settings(base_rule))) => {
                    base_rule.update(own);
                }
                _ => {
                    base.rules.insert(id, raw);
                }
            }
        }
        self.rules = base.rules;
        if base.ignore.is_some() {
            self.ignore = base.ignore;
        }
    }

    /// Mirrors `YamlLintConfig.validate`.
    fn validate(&mut self) -> Result<()> {
        for (id, raw) in &mut self.rules {
            let Some(spec) = rules::get(id) else {
                bail!("invalid config: no such rule: \"{id}\"")
            };
            if let RawRule::Settings(conf) = raw {
                validate_rule_conf(spec, conf)?;
            }
        }
        Ok(())
    }
}

/// Mirrors `validate_rule_conf`: default level, every option known and of
/// the right shape, defaults filled in, then the rule's own check.
fn validate_rule_conf(spec: &RuleSpec, conf: &mut RuleConf) -> Result<()> {
    if conf.level.is_none() {
        conf.level = Some(Level::Error);
    }
    for (name, value) in &conf.options {
        let Some(opt) = spec.options.iter().find(|o| o.name == name) else {
            bail!(
                "invalid config: unknown option \"{name}\" for rule \"{}\"",
                spec.id
            )
        };
        match &opt.kind {
            OptKind::Bool => {
                if value.as_bool().is_none() {
                    bail!(
                        "invalid config: option \"{name}\" of \"{}\" should be bool",
                        spec.id
                    );
                }
            }
            OptKind::Int => {
                if value.as_int().is_none() {
                    bail!(
                        "invalid config: option \"{name}\" of \"{}\" should be int",
                        spec.id
                    );
                }
            }
            OptKind::OneOf(alts) => {
                if !alts.iter().any(|alt| alt.matches(value)) {
                    bail!(
                        "invalid config: option \"{name}\" of \"{}\" should be in {}",
                        spec.id,
                        alts_to_string(alts)
                    );
                }
            }
            OptKind::ListOf(alts) => {
                let ok = value.as_list().is_some_and(|items| {
                    items
                        .iter()
                        .all(|item| alts.iter().any(|alt| alt.matches(item)))
                });
                if !ok {
                    bail!(
                        "invalid config: option \"{name}\" of \"{}\" should only contain \
                         values in {}",
                        spec.id,
                        alts_to_string(alts)
                    );
                }
            }
        }
    }
    for opt in spec.options {
        if !conf.options.contains_key(opt.name) {
            let value = match &opt.default {
                OptDefault::Bool(b) => OptValue::Bool(*b),
                OptDefault::Int(i) => OptValue::Int(*i),
                OptDefault::Str(s) => OptValue::Str((*s).to_owned()),
                OptDefault::EmptyList => OptValue::List(Vec::new()),
                OptDefault::StrList(items) => OptValue::List(
                    items
                        .iter()
                        .map(|s| OptValue::Str((*s).to_owned()))
                        .collect(),
                ),
            };
            conf.options.insert(opt.name.to_owned(), value);
        }
    }
    if let Some(validate) = spec.validate
        && let Some(msg) = validate(conf)
    {
        bail!("invalid config: {}: {msg}", spec.id);
    }
    Ok(())
}

fn load(text: &str, base_dir: &Path) -> Result<Parsed> {
    let mut parsed = parse(text, base_dir)?;
    parsed.validate()?;
    Ok(parsed)
}

/// A validated yamllint configuration.
#[derive(Debug, Clone)]
pub struct Config {
    rules: BTreeMap<String, Option<RuleConf>>,
    ignore: Option<Gitignore>,
}

impl Config {
    /// Parse a config text. Relative paths in it (`extends`,
    /// `ignore-from-file`) resolve against `base_dir`.
    pub fn parse(text: &str, base_dir: &Path) -> Result<Self> {
        let parsed = load(text, base_dir)?;
        let rules = parsed
            .rules
            .into_iter()
            .map(|(id, raw)| {
                let conf = match raw {
                    RawRule::Disabled => None,
                    RawRule::Settings(conf) => Some(conf),
                };
                (id, conf)
            })
            .collect();
        Ok(Self {
            rules,
            ignore: parsed.ignore,
        })
    }

    /// Read and parse a config file; relative paths resolve against its
    /// directory.
    pub fn from_file(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read yamllint config {}", path.display()))?;
        let base_dir = path.parent().unwrap_or_else(|| Path::new(""));
        Self::parse(&text, base_dir)
            .with_context(|| format!("in yamllint config {}", path.display()))
    }

    /// yamllint's `default` configuration.
    pub fn default_conf() -> Self {
        Self::parse(DEFAULT_CONF, Path::new(""))
            .expect("yamllint's embedded default config is valid")
    }

    /// Mirrors `is_file_ignored`.
    pub fn is_file_ignored(&self, path: &Path) -> bool {
        self.ignore.as_ref().is_some_and(|gi| is_ignored(gi, path))
    }

    /// The rule's settings, `None` when the rule is disabled or unknown.
    #[cfg(test)]
    pub fn rule(&self, id: &str) -> Option<&RuleConf> {
        self.rules.get(id).and_then(Option::as_ref)
    }

    /// Mirrors `enabled_rules`: every enabled rule whose own `ignore` does
    /// not exclude `path`.
    pub fn enabled_rules(&self, path: Option<&Path>) -> Vec<(&'static RuleSpec, &RuleConf)> {
        self.rules
            .iter()
            .filter_map(|(id, conf)| {
                let conf = conf.as_ref()?;
                if let Some(path) = path
                    && conf.ignores(path)
                {
                    return None;
                }
                rules::get(id).map(|spec| (spec, conf))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> Result<Config> {
        Config::parse(text, Path::new(""))
    }

    #[test]
    fn default_conf_enables_the_documented_rules() {
        let conf = Config::default_conf();
        assert!(conf.rule("indentation").is_some());
        assert!(conf.rule("document-end").is_none());
        assert_eq!(conf.rule("truthy").unwrap().level(), Level::Warning);
        assert_eq!(conf.rule("line-length").unwrap().int_opt("max"), 80);
        assert_eq!(
            conf.rule("indentation").unwrap().str_opt("spaces"),
            "consistent"
        );
    }

    #[test]
    fn extends_merges_settings_into_the_base_rule() {
        let conf = parse(
            "extends: default\nrules:\n  truthy:\n    allowed-values: ['true', 'false']\n    check-keys: false\n  line-length:\n    max: 240\n  document-start: disable\n",
        )
        .unwrap();
        // The base level survives a partial override, as in yamllint.
        let truthy = conf.rule("truthy").unwrap();
        assert_eq!(truthy.level(), Level::Warning);
        assert!(!truthy.bool_opt("check-keys"));
        assert_eq!(conf.rule("line-length").unwrap().int_opt("max"), 240);
        assert_eq!(conf.rule("line-length").unwrap().level(), Level::Error);
        assert!(conf.rule("document-start").is_none());
    }

    #[test]
    fn a_rule_enabled_without_a_base_gets_error_level_and_defaults() {
        let conf = parse("rules:\n  colons: enable\n").unwrap();
        let colons = conf.rule("colons").unwrap();
        assert_eq!(colons.level(), Level::Error);
        assert_eq!(colons.int_opt("max-spaces-before"), 0);
        assert_eq!(colons.int_opt("max-spaces-after"), 1);
    }

    #[test]
    fn unknown_rule_and_option_are_errors() {
        let err = parse("rules:\n  nope: enable\n").unwrap_err().to_string();
        assert!(err.contains("no such rule: \"nope\""), "{err}");
        let err = parse("rules:\n  colons:\n    max-foo: 1\n")
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("unknown option \"max-foo\" for rule \"colons\""),
            "{err}"
        );
        let err = parse("rules:\n  colons:\n    max-spaces-after: yes\n")
            .unwrap_err()
            .to_string();
        assert!(err.contains("should be int"), "{err}");
        let err = parse("rules:\n  indentation:\n    indent-sequences: sometimes\n")
            .unwrap_err()
            .to_string();
        assert!(err.contains("should be in"), "{err}");
        let err = parse("rules:\n  truthy:\n    allowed-values: [maybe]\n")
            .unwrap_err()
            .to_string();
        assert!(err.contains("should only contain values in"), "{err}");
    }

    #[test]
    fn rule_value_must_be_enable_disable_or_mapping() {
        let err = parse("rules:\n  colons: 3\n").unwrap_err().to_string();
        assert!(
            err.contains("should be either \"enable\", \"disable\" or a mapping"),
            "{err}"
        );
    }

    #[test]
    fn locale_is_rejected_and_yaml_files_ignored() {
        assert!(parse("locale: en_US.UTF-8\n").is_err());
        assert!(parse("yaml-files: ['*.yaml']\nrules: {}\n").is_ok());
        assert!(parse("yaml-files: nope\n").is_err());
    }

    #[test]
    fn ignore_patterns_exclude_files() {
        let conf = parse("ignore: |\n  generated/\n  *.lock.yaml\nrules:\n  colons:\n    ignore: ['skip.yaml']\n").unwrap();
        assert!(conf.is_file_ignored(Path::new("generated/x.yaml")));
        assert!(conf.is_file_ignored(Path::new("a/b.lock.yaml")));
        assert!(!conf.is_file_ignored(Path::new("a/b.yaml")));
        assert!(conf.rule("colons").unwrap().ignores(Path::new("skip.yaml")));
        let colons_enabled = conf
            .enabled_rules(Some(Path::new("skip.yaml")))
            .iter()
            .any(|(spec, _)| spec.id == "colons");
        assert!(!colons_enabled);
        assert!(parse("ignore: [a]\nignore-from-file: b\n").is_err());
    }

    #[test]
    fn quoted_strings_validate_hook_runs() {
        let err = parse("rules:\n  quoted-strings:\n    required: true\n    extra-allowed: [x]\n")
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("cannot use both \"required: true\" and \"extra-allowed\""),
            "{err}"
        );
    }
}
