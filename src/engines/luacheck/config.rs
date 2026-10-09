//! luacheck's `config`, `globbing` and `fs` modules: loading `.luacheckrc`
//! (a Lua chunk, run in an environment that falls back to the globals, with
//! luacheck's autovivifying `files` and std-falling-back `stds`), stacking
//! it with the command-line options, and per-path option overrides.

use std::cmp::Ordering;
use std::collections::{BTreeMap, HashSet};
use std::path::Path;
use std::rc::Rc;

use anyhow::{Context, Result, bail};
use mlua::{Lua, Table, Value};

use super::options::{self, OptionStack, Stds};
use super::pattern;
use super::standards;
use super::value::{LKey, LTable, LVal};

// ---------------------------------------------------------------------------
// fs

/// `fs.split_base`: the root part of a path and the rest.
pub fn split_base(path: &str) -> (&str, &str) {
    if let Some(rest) = path.strip_prefix("//") {
        ("//", rest)
    } else if let Some(rest) = path.strip_prefix('/') {
        ("/", rest)
    } else {
        ("", path)
    }
}

fn is_absolute(path: &str) -> bool {
    !split_base(path).0.is_empty()
}

/// `fs.normalize`.
pub fn normalize(path: &str) -> String {
    let (base, rest) = split_base(path);
    let mut parts: Vec<&str> = Vec::new();
    for part in rest.split('/').filter(|p| !p.is_empty()) {
        if part == "." {
            continue;
        }
        if part == ".." && parts.last().is_some_and(|p| *p != "..") {
            parts.pop();
        } else {
            parts.push(part);
        }
    }
    if base.is_empty() && parts.is_empty() {
        ".".to_string()
    } else {
        format!("{base}{}", parts.join("/"))
    }
}

/// `fs.join` of two paths.
pub fn join(base: &str, path: &str) -> String {
    if base.is_empty() || is_absolute(path) {
        path.to_string()
    } else if base.ends_with('/') {
        format!("{base}{path}")
    } else {
        format!("{base}/{path}")
    }
}

fn is_subpath(path: &str, subpath: &str) -> bool {
    let (base1, rest1) = split_base(path);
    let (base2, rest2) = split_base(subpath);
    if base1 != base2 || !rest2.starts_with(rest1) {
        return false;
    }
    rest1 == rest2 || rest2.as_bytes().get(rest1.len()) == Some(&b'/')
}

/// The current directory with a trailing separator, as `fs.get_current_dir`.
pub fn current_dir() -> Result<String> {
    let dir = std::env::current_dir().context("Failed to get the current directory")?;
    let mut dir = dir.to_string_lossy().into_owned();
    if !dir.ends_with('/') {
        dir.push('/');
    }
    Ok(dir)
}

// ---------------------------------------------------------------------------
// globbing

fn get_parts(path: &str) -> Vec<&str> {
    path.split('/').filter(|p| !p.is_empty()).collect()
}

fn glob_part_to_pattern(glob_part: &[u8]) -> Vec<u8> {
    let mut buffer = vec![b'^'];
    let mut i = 0usize;
    while i < glob_part.len() {
        let bracketless_end = glob_part[i..]
            .iter()
            .position(|&c| c == b'[')
            .map_or(glob_part.len(), |p| i + p);
        for &c in &glob_part[i..bracketless_end] {
            if c.is_ascii_punctuation() {
                if c == b'*' || c == b'?' {
                    buffer.push(b'.');
                } else {
                    buffer.push(b'%');
                }
            }
            buffer.push(c);
        }
        i = bracketless_end;
        if glob_part.get(i) == Some(&b'[') {
            buffer.push(b'[');
            i += 1;
            match glob_part.get(i) {
                Some(b'!') => {
                    buffer.push(b'^');
                    i += 1;
                }
                Some(b']') => {
                    buffer.extend_from_slice(b"%]");
                    i += 1;
                }
                _ => {}
            }
            let end = glob_part[i.min(glob_part.len())..]
                .iter()
                .position(|&c| c == b']')
                .map_or(glob_part.len(), |p| i + p);
            let mut bracketless = &glob_part[i.min(glob_part.len())..end];
            i = end;
            if bracketless.first() == Some(&b'-') {
                buffer.extend_from_slice(b"%-");
                bracketless = &bracketless[1..];
            }
            let mut last_dash: &[u8] = b"";
            if bracketless.last() == Some(&b'-') {
                last_dash = b"-";
                bracketless = &bracketless[..bracketless.len() - 1];
            }
            for &c in bracketless {
                if c.is_ascii_punctuation() && c != b'-' {
                    buffer.push(b'%');
                }
                buffer.push(c);
            }
            buffer.extend_from_slice(last_dash);
            buffer.push(b']');
            i += 1;
        }
    }
    buffer.push(b'$');
    buffer
}

fn parts_match(
    glob_parts: &[&str],
    glob_i: usize,
    path_parts: &[&str],
    path_i: usize,
) -> Result<bool, pattern::PatternError> {
    let Some(glob_part) = glob_parts.get(glob_i) else {
        return Ok(true);
    };
    if *glob_part == "**" {
        for i in path_i..=path_parts.len() {
            if parts_match(glob_parts, glob_i + 1, path_parts, i)? {
                return Ok(true);
            }
        }
        return Ok(false);
    }
    let Some(path_part) = path_parts.get(path_i) else {
        return Ok(false);
    };
    Ok(pattern::pmatch(
        path_part.as_bytes(),
        &glob_part_to_pattern(glob_part.as_bytes()),
    )? && parts_match(glob_parts, glob_i + 1, path_parts, path_i + 1)?)
}

/// `globbing.match`: whether an absolute path matches an absolute glob (or
/// lies below it).
pub fn glob_match(glob: &str, path: &str) -> Result<bool, pattern::PatternError> {
    if !glob.contains(['*', '?', '[']) {
        return Ok(is_subpath(glob, path));
    }
    let (glob_base, glob) = split_base(glob);
    let (path_base, path) = split_base(path);
    if glob_base != path_base {
        return Ok(false);
    }
    parts_match(&get_parts(glob), 0, &get_parts(path), 0)
}

/// `globbing.compare`: whether `glob1` is less specific than `glob2`.
fn glob_less(glob1: &str, glob2: &str) -> bool {
    let (base1, glob1) = split_base(glob1);
    let (base2, glob2) = split_base(glob2);
    if base1 != base2 {
        return base1 < base2;
    }
    let parts1 = get_parts(glob1);
    let parts2 = get_parts(glob2);
    for i in 0..parts1.len().max(parts2.len()) {
        let (Some(p1), Some(p2)) = (parts1.get(i), parts2.get(i)) else {
            return parts1.get(i).is_none();
        };
        if (*p1 == "**" || *p2 == "**") && p1 != p2 {
            return *p1 == "**";
        }
        let specials = |p: &str| p.chars().filter(|c| matches!(c, '*' | '?' | '[')).count();
        let (s1, s2) = (specials(p1), specials(p2));
        if s1 != s2 {
            return s1 > s2;
        }
    }
    glob1 < glob2
}

// ---------------------------------------------------------------------------
// Lua values from mlua

fn convert(value: &Value, seen: &mut HashSet<usize>) -> Result<LVal> {
    Ok(match value {
        Value::Nil => LVal::Other("nil"),
        Value::Boolean(b) => LVal::Bool(*b),
        Value::Integer(i) => LVal::Num(*i as f64),
        Value::Number(n) => LVal::Num(*n),
        Value::String(s) => LVal::Str(s.as_bytes().to_vec()),
        Value::Table(t) => convert_table(t, seen)?,
        Value::Function(_) => LVal::Other("function"),
        Value::Thread(_) => LVal::Other("thread"),
        Value::UserData(_) | Value::LightUserData(_) => LVal::Other("userdata"),
        _ => LVal::Other("userdata"),
    })
}

fn convert_key(value: &Value) -> LKey {
    match value {
        Value::Boolean(b) => LKey::Bool(*b),
        Value::Integer(i) => LKey::Num(*i as f64),
        Value::Number(n) => LKey::Num(*n),
        Value::String(s) => LKey::Str(s.as_bytes().to_vec()),
        Value::Table(_) => LKey::Other("table"),
        Value::Function(_) => LKey::Other("function"),
        _ => LKey::Other("userdata"),
    }
}

fn convert_table(table: &Table, seen: &mut HashSet<usize>) -> Result<LVal> {
    let pointer = table.to_pointer() as usize;
    if !seen.insert(pointer) {
        // A table already being converted (a cycle): luacheck would walk it
        // as a plain table; nothing in an option table is cyclic in practice.
        return Ok(LVal::table(Vec::new(), Vec::new()));
    }
    let mut array = Vec::new();
    let mut index = 1i64;
    loop {
        let value: Value = table
            .raw_get(index)
            .context("Failed to read a configuration table")?;
        if value.is_nil() {
            break;
        }
        array.push(convert(&value, seen)?);
        index += 1;
    }
    let n = array.len() as i64;
    let mut hash = Vec::new();
    for pair in table.clone().pairs::<Value, Value>() {
        let (key, value) = pair.context("Failed to iterate a configuration table")?;
        let in_array = match &key {
            Value::Integer(i) => (1..=n).contains(i),
            Value::Number(f) => f.fract() == 0.0 && (1.0..=n as f64).contains(f),
            _ => false,
        };
        if in_array {
            continue;
        }
        hash.push((convert_key(&key), convert(&value, seen)?));
    }
    seen.remove(&pointer);
    Ok(LVal::table(array, hash))
}

// ---------------------------------------------------------------------------
// config

/// A loaded (or built) configuration.
pub struct Config {
    pub options: Rc<LTable>,
    /// The file the configuration was loaded from, for error messages.
    pub path: Option<String>,
    /// `None`: not anchored (command-line options); `Some("")`: the
    /// fallback config used when no file is found.
    pub anchor_dir: Option<String>,
}

fn with_default_std(files: &mut Vec<(LKey, LVal)>, glob: &str, std: &str) {
    let key = LKey::Str(glob.as_bytes().to_vec());
    let mut pattern_opts = vec![(LKey::Str(b"std".to_vec()), LVal::str(std))];
    if let Some((_, LVal::Table(existing))) = files.iter().find(|(k, _)| *k == key) {
        for (k, v) in existing.pairs() {
            match pattern_opts.iter_mut().find(|(pk, _)| *pk == k) {
                Some(entry) => entry.1 = v,
                None => pattern_opts.push((k, v)),
            }
        }
    }
    let value = LVal::table(Vec::new(), pattern_opts);
    match files.iter_mut().find(|(k, _)| *k == key) {
        Some(entry) => entry.1 = value,
        None => files.push((key, value)),
    }
}

/// `add_default_path_options`: default stds for spec files, rockspecs,
/// luacheck configs and `LDoc` configs.
fn add_default_path_options(options: &LTable) -> Rc<LTable> {
    let mut files: Vec<(LKey, LVal)> = match options.get("files") {
        Some(LVal::Table(t)) => t.pairs(),
        _ => Vec::new(),
    };
    for (glob, std) in [
        ("**/spec/**/*_spec.lua", "+busted"),
        ("**/test/**/*_spec.lua", "+busted"),
        ("**/tests/**/*_spec.lua", "+busted"),
        ("**/*.rockspec", "+rockspec"),
        ("**/*.luacheckrc", "+luacheckrc"),
        ("**/config.ld", "+ldoc"),
    ] {
        with_default_std(&mut files, glob, std);
    }
    let mut hash: Vec<(LKey, LVal)> = options
        .hash
        .iter()
        .filter(|(k, _)| *k != LKey::Str(b"files".to_vec()))
        .cloned()
        .collect();
    hash.push((LKey::Str(b"files".to_vec()), LVal::table(Vec::new(), files)));
    Rc::new(LTable::new(options.array.clone(), hash))
}

fn fallback_config() -> Config {
    Config {
        options: add_default_path_options(&LTable::default()),
        path: None,
        anchor_dir: Some(String::new()),
    }
}

/// `fs.find_file`: the directory holding `file`, searching upwards from
/// `start`, and the relative path to it.
fn find_file(start: &str, file: &str) -> Option<(String, String)> {
    let path = normalize(start);
    let (base, mut rest) = split_base(&path);
    let base = base.to_string();
    let mut rest_owned = rest.to_string();
    let mut rel_path = String::new();
    loop {
        let dir = format!("{base}{rest_owned}");
        if Path::new(&join(&dir, file)).is_file() {
            return Some((dir, rel_path));
        }
        if rest_owned.is_empty() {
            return None;
        }
        rest = &rest_owned;
        rest_owned = match rest.rfind('/') {
            Some(i) => rest[..i].to_string(),
            None => String::new(),
        };
        rel_path.push_str("../");
    }
}

fn strip_chunk_prefix(message: &str) -> String {
    let message = message
        .split("\nstack traceback:")
        .next()
        .unwrap_or(message);
    let message = message.trim_end();
    match message.strip_prefix("[string \"chunk\"]:") {
        Some(rest) => format!("line {rest}"),
        None => format!("line {message}"),
    }
}

/// Runs a configuration file the way `config.load_config` does and returns
/// the resulting option table.
fn run_config(path: &str, anchor_dir: Option<&str>) -> Result<LVal, String> {
    let src = std::fs::read(path).map_err(|e| {
        format!("Couldn't load configuration from {path}: I/O error (couldn't read: {e})")
    })?;
    let src = src.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(&src).to_vec();
    let lua = Lua::new();
    let fail = |kind: &str, msg: String| {
        format!("Couldn't load configuration from {path}: {kind} error ({msg})")
    };
    let setup = || -> mlua::Result<(Table, Table)> {
        let globals = lua.globals();
        // Modules next to an anchored configuration can be required from it.
        if let Some(anchor_dir) = anchor_dir {
            let package: Table = globals.get("package")?;
            let package_path: String = package.get("path")?;
            package.set(
                "path",
                format!("{anchor_dir}/?.lua;{anchor_dir}/?/init.lua;{package_path}"),
            )?;
        }
        let special: Table = lua.create_table()?;
        let files = lua.create_table()?;
        let files_mt = lua.create_table()?;
        files_mt.set(
            "__index",
            lua.create_function(|lua, (files, key): (Table, Value)| {
                let t = lua.create_table()?;
                files.raw_set(key, t.clone())?;
                Ok(t)
            })?,
        )?;
        files.set_metatable(Some(files_mt.clone()))?;
        special.set("files", files)?;
        special.set("stds", lua.create_table()?)?;
        let env = lua.create_table()?;
        let env_mt = lua.create_table()?;
        let special_index = special.clone();
        let globals_index = globals;
        env_mt.set(
            "__index",
            lua.create_function(move |_, (_env, key): (Table, Value)| {
                if let Value::String(s) = &key
                    && matches!(&*s.as_bytes(), b"files" | b"stds")
                {
                    return special_index.raw_get::<Value>(key);
                }
                globals_index.get::<Value>(key)
            })?,
        )?;
        let special_newindex = special.clone();
        let files_mt_newindex = files_mt;
        env_mt.set(
            "__newindex",
            lua.create_function(move |_, (env, key, value): (Table, Value, Value)| {
                if let Value::String(s) = &key
                    && matches!(&*s.as_bytes(), b"files" | b"stds")
                {
                    if let Value::Table(t) = &value
                        && &*s.as_bytes() == b"files"
                    {
                        t.set_metatable(Some(files_mt_newindex.clone()))?;
                    }
                    return special_newindex.raw_set(key, value);
                }
                env.raw_set(key, value)
            })?,
        )?;
        env.set_metatable(Some(env_mt))?;
        Ok((env, special))
    };
    let (env, special) = setup().map_err(|e| fail("runtime", e.to_string()))?;
    let func = lua
        .load(src.as_slice())
        .set_name("chunk")
        .set_environment(env.clone())
        .into_function()
        .map_err(|e| match e {
            mlua::Error::SyntaxError { message, .. } => {
                fail("syntax", strip_chunk_prefix(&message))
            }
            other => fail("syntax", other.to_string()),
        })?;
    let ret: Value = func.call(()).map_err(|e| match e {
        mlua::Error::RuntimeError(message) => fail("runtime", strip_chunk_prefix(&message)),
        mlua::Error::CallbackError { cause, .. } => {
            fail("runtime", strip_chunk_prefix(&cause.to_string()))
        }
        other => fail("runtime", strip_chunk_prefix(&other.to_string())),
    })?;
    let finish = || -> mlua::Result<()> {
        if let Value::Table(ret) = ret {
            for pair in ret.pairs::<Value, Value>() {
                let (k, v) = pair?;
                env.set(k, v)?;
            }
        }
        env.set_metatable(None)?;
        for pair in special.pairs::<Value, Value>() {
            let (k, v) = pair?;
            env.raw_set(k, v)?;
        }
        Ok(())
    };
    finish().map_err(|e| fail("runtime", e.to_string()))?;
    let mut seen = HashSet::new();
    convert_table(&env, &mut seen).map_err(|e| fail("runtime", format!("{e:#}")))
}

/// `config.load_config`: finds the configuration (`path` relative to the
/// current directory or its ancestors) and loads it; the fallback config
/// when the default file is not found.
pub fn load_config(path: Option<&str>, current_dir: &str) -> Result<Config> {
    let Some(path) = path else {
        return Ok(fallback_config());
    };
    // A configuration given by absolute path is not anchored: its paths are
    // relative to the current directory, as in luacheck's `locate_config`.
    let (config_path, anchor_dir) = if is_absolute(path) {
        (path.to_string(), None)
    } else if let Some((anchor_dir, rel_dir)) = find_file(current_dir, path) {
        (join(&rel_dir, path), Some(anchor_dir))
    } else {
        if path != ".luacheckrc" {
            bail!("Couldn't find configuration file {path}");
        }
        return Ok(fallback_config());
    };
    let options =
        run_config(&config_path, anchor_dir.as_deref()).map_err(|e| anyhow::anyhow!(e))?;
    let LVal::Table(options) = options else {
        bail!("configuration {config_path} did not produce a table");
    };
    Ok(Config {
        options: add_default_path_options(&options),
        path: Some(config_path),
        anchor_dir,
    })
}

fn add_stds_from_config(conf: &Config, stds: &mut Stds) -> Result<(), String> {
    let Some(value) = conf.options.get("stds") else {
        return Ok(());
    };
    let LVal::Table(table) = value else {
        return Err(format!(
            "invalid option 'stds': table expected, got {}",
            value.type_name()
        ));
    };
    let mut named: Vec<(Vec<u8>, LVal)> = table
        .hash
        .iter()
        .filter_map(|(k, v)| match k {
            LKey::Str(name) => Some((name.clone(), v.clone())),
            _ => None,
        })
        .collect();
    named.sort_by(|a, b| a.0.cmp(&b.0));
    for (name, std) in named {
        let LVal::Table(std) = std else {
            return Err(format!(
                "invalid custom std '{}': table expected, got {}",
                String::from_utf8_lossy(&name),
                std.type_name()
            ));
        };
        standards::validate_std_table(&std).map_err(|err| {
            format!(
                "invalid custom std '{}': {err}",
                String::from_utf8_lossy(&name)
            )
        })?;
        stds.insert(name, std);
    }
    Ok(())
}

fn validate_config(conf: &Config, stds: &Stds) -> Result<(), String> {
    options::validate(
        &options::top_options(),
        &LVal::Table(conf.options.clone()),
        stds,
    )?;
    if let Some(LVal::Table(files)) = conf.options.get("files") {
        for (path, opts) in &files.hash {
            if let LKey::Str(path) = path {
                options::validate(&options::all_options(), opts, stds).map_err(|err| {
                    format!(
                        "invalid options for path '{}': {err}",
                        String::from_utf8_lossy(path)
                    )
                })?;
            }
        }
    }
    Ok(())
}

/// `config.stack_configs`: the configs (later ones override earlier ones)
/// and the stds they define, validated.
pub struct ConfigStack {
    pub configs: Vec<Config>,
    pub stds: Stds,
}

pub fn stack_configs(configs: Vec<Config>) -> Result<ConfigStack> {
    let mut stds: Stds = standards::builtin_standards();
    let prefix = |conf: &Config| match &conf.path {
        Some(path) => format!("in config loaded from {path}: "),
        None => String::new(),
    };
    for conf in &configs {
        add_stds_from_config(conf, &mut stds)
            .map_err(|e| anyhow::anyhow!("{}{e}", prefix(conf)))?;
    }
    for conf in &configs {
        validate_config(conf, &stds).map_err(|e| anyhow::anyhow!("{}{e}", prefix(conf)))?;
    }
    Ok(ConfigStack { configs, stds })
}

impl ConfigStack {
    /// `exclude_files` and `include_files` as absolute globs.
    pub fn file_filters(&self, current_dir: &str) -> (Vec<String>, Vec<String>) {
        let mut exclude = Vec::new();
        let mut include = Vec::new();
        for conf in &self.configs {
            let anchor_dir = conf
                .anchor_dir
                .clone()
                .unwrap_or_else(|| current_dir.to_string());
            for (option, out) in [
                ("include_files", &mut include),
                ("exclude_files", &mut exclude),
            ] {
                if let Some(LVal::Table(globs)) = conf.options.get(option) {
                    for glob in globs.strings() {
                        out.push(normalize(&join(
                            &anchor_dir,
                            &String::from_utf8_lossy(&glob),
                        )));
                    }
                }
            }
        }
        (exclude, include)
    }

    /// `ConfigStack:get_options`: the option stack for a file.
    pub fn get_options(
        &self,
        filename: &str,
        current_dir: &str,
    ) -> Result<OptionStack, pattern::PatternError> {
        let mut res: OptionStack = Vec::new();
        for conf in &self.configs {
            res.push(conf.options.clone());
            let Some(LVal::Table(files)) = conf.options.get("files") else {
                continue;
            };
            let abs_filename = normalize(&join(current_dir, filename));
            let anchor_dir = match &conf.anchor_dir {
                Some(dir) if dir.is_empty() => split_base(current_dir).0.to_string(),
                Some(dir) => dir.clone(),
                None => current_dir.to_string(),
            };
            let mut matching: Vec<(String, Rc<LTable>)> = Vec::new();
            for (glob, opts) in &files.hash {
                let (LKey::Str(glob), LVal::Table(opts)) = (glob, opts) else {
                    continue;
                };
                let abs_glob = normalize(&join(&anchor_dir, &String::from_utf8_lossy(glob)));
                if glob_match(&abs_glob, &abs_filename)? {
                    matching.push((abs_glob, opts.clone()));
                }
            }
            matching.sort_by(|a, b| {
                if glob_less(&a.0, &b.0) {
                    Ordering::Less
                } else if glob_less(&b.0, &a.0) {
                    Ordering::Greater
                } else {
                    Ordering::Equal
                }
            });
            res.extend(matching.into_iter().map(|(_, opts)| opts));
        }
        Ok(res)
    }
}

/// Builds the command-line options table (an unanchored config).
pub fn cli_config(entries: BTreeMap<&'static str, LVal>) -> Config {
    let hash = entries
        .into_iter()
        .map(|(k, v)| (LKey::Str(k.as_bytes().to_vec()), v))
        .collect();
    Config {
        options: Rc::new(LTable::new(Vec::new(), hash)),
        path: None,
        anchor_dir: None,
    }
}

/// `matches_any` over globs.
pub fn matches_any(globs: &[String], filename: &str) -> Result<bool, pattern::PatternError> {
    for glob in globs {
        if glob_match(glob, filename)? {
            return Ok(true);
        }
    }
    Ok(false)
}
