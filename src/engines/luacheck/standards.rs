//! luacheck's `standards` module: validating standard tables in their user
//! format, merging them into a normalized definition tree, and the
//! built-in standards (generated from luacheck's sources into
//! `tables.json` by `scripts/gen-iluacheck-tables.lua`).

use std::collections::BTreeMap;
use std::rc::Rc;

use super::value::{LKey, LTable, LVal, format_20g};

/// A normalized definition: a global or a field.
#[derive(Clone, Debug, Default)]
pub struct Def {
    pub read_only: Option<bool>,
    pub other_fields: Option<bool>,
    pub fields: Option<BTreeMap<Vec<u8>, Self>>,
    pub deep_read_only: bool,
}

/// Validates a fields table (or the `globals`/`read_globals` of a std);
/// returns the error and the index of the table holding it.
fn validate_fields(
    fields: Option<&LVal>,
    is_root: bool,
    index: Option<String>,
) -> Result<(), (String, Option<String>)> {
    let Some(fields) = fields else {
        return Ok(());
    };
    let field_type = if is_root { "global" } else { "field" };
    let LVal::Table(table) = fields else {
        return Err((
            format!("{field_type}s table expected, got {}", fields.type_name()),
            index,
        ));
    };
    for (key, value) in table.pairs() {
        match &key {
            LKey::Str(name) => {
                let new_index = format!(
                    "{}.{}",
                    index.as_deref().unwrap_or(""),
                    String::from_utf8_lossy(name)
                );
                let LVal::Table(def) = &value else {
                    return Err((
                        format!(
                            "{field_type} description table expected, got {}",
                            value.type_name()
                        ),
                        Some(new_index),
                    ));
                };
                if let Some(read_only) = def.get("read_only")
                    && !matches!(read_only, LVal::Bool(_))
                {
                    return Err((
                        format!(
                            "invalid value of option 'read_only': boolean expected, got {}",
                            read_only.type_name()
                        ),
                        Some(new_index),
                    ));
                }
                if let Some(other_fields) = def.get("other_fields")
                    && !matches!(other_fields, LVal::Bool(_))
                {
                    return Err((
                        format!(
                            "invalid value of option 'other_fields': boolean expected, got {}",
                            other_fields.type_name()
                        ),
                        Some(new_index),
                    ));
                }
                validate_fields(
                    def.get("fields"),
                    false,
                    Some(format!("{new_index}.fields")),
                )?;
            }
            _ => {
                if !matches!(value, LVal::Str(_)) {
                    let key_as_string = match &key {
                        LKey::Num(n) => format_20g(*n),
                        other => format!("<{}>", other.type_name()),
                    };
                    let new_index = format!("{}[{key_as_string}]", index.as_deref().unwrap_or(""));
                    return Err((
                        format!(
                            "string expected as {field_type} name, got {}",
                            value.type_name()
                        ),
                        Some(new_index),
                    ));
                }
            }
        }
    }
    Ok(())
}

/// `standards.validate_globals_table`.
pub fn validate_globals_table(globals: &LVal) -> Result<(), String> {
    validate_fields(Some(globals), true, None).map_err(|(err, index)| match index {
        Some(index) => format!("in field {index}: {err}"),
        None => err,
    })
}

/// `standards.validate_std_table`.
pub fn validate_std_table(std_table: &LTable) -> Result<(), String> {
    let result = validate_fields(std_table.get("globals"), true, Some(".globals".to_string()))
        .and_then(|()| {
            validate_fields(
                std_table.get("read_globals"),
                true,
                Some(".read_globals".to_string()),
            )
        });
    result.map_err(|(err, index)| format!("in field {}: {err}", index.unwrap_or_default()))
}

fn infinitely_indexable_def() -> Rc<LTable> {
    Rc::new(LTable::new(
        Vec::new(),
        vec![(LKey::Str(b"other_fields".to_vec()), LVal::Bool(true))],
    ))
}

fn add_fields(
    def: &mut Def,
    fields: Option<&LVal>,
    overwrite: bool,
    ignore_array_part: bool,
    default_read_only: Option<bool>,
) {
    let Some(LVal::Table(fields)) = fields else {
        return;
    };
    for (key, value) in fields.pairs() {
        let is_string = matches!(key, LKey::Str(_));
        if !is_string && ignore_array_part {
            continue;
        }
        let (field_name, field_def) = match (key, value) {
            (LKey::Str(name), LVal::Table(def)) => (name, def),
            (_, LVal::Str(name)) if !is_string => (name, infinitely_indexable_def()),
            _ => continue,
        };
        let existing = def
            .fields
            .get_or_insert_with(BTreeMap::new)
            .entry(field_name)
            .or_default();
        let new_read_only = field_def
            .get("read_only")
            .and_then(LVal::as_bool)
            .or(default_read_only);
        if let Some(new_read_only) = new_read_only
            && (overwrite || !new_read_only)
        {
            existing.read_only = Some(new_read_only);
        }
        if let Some(other_fields) = field_def.get("other_fields").and_then(LVal::as_bool)
            && (overwrite || other_fields)
        {
            existing.other_fields = Some(other_fields);
        }
        add_fields(existing, field_def.get("fields"), overwrite, false, None);
    }
}

/// `standards.add_std_table`.
pub fn add_std_table(
    final_std: &mut Def,
    std_table: &LTable,
    overwrite: bool,
    ignore_top_array_part: bool,
) {
    add_fields(
        final_std,
        std_table.get("globals"),
        overwrite,
        ignore_top_array_part,
        Some(false),
    );
    add_fields(
        final_std,
        std_table.get("read_globals"),
        overwrite,
        ignore_top_array_part,
        Some(true),
    );
}

/// `standards.overwrite_field`.
pub fn overwrite_field(final_std: &mut Def, field_names: &[Vec<u8>], read_only: bool) {
    let mut field_def = final_std;
    for field_name in field_names {
        let fields = field_def.fields.get_or_insert_with(BTreeMap::new);
        field_def = fields.entry(field_name.clone()).or_insert_with(|| Def {
            read_only: Some(read_only),
            ..Def::default()
        });
    }
    *field_def = Def {
        read_only: Some(read_only),
        other_fields: Some(true),
        fields: None,
        deep_read_only: false,
    };
}

/// `standards.remove_field`.
pub fn remove_field(final_std: &mut Def, field_names: &[Vec<u8>]) {
    let Some((last, parents)) = field_names.split_last() else {
        return;
    };
    let mut field_def = &mut *final_std;
    for field_name in parents {
        match field_def
            .fields
            .as_mut()
            .and_then(|f| f.get_mut(field_name))
        {
            Some(next) => field_def = next,
            None => return,
        }
    }
    if let Some(fields) = field_def.fields.as_mut() {
        fields.remove(last);
    }
}

fn infer_deep_read_only_statuses(def: &mut Def, read_only: bool) {
    let mut deep_read_only = !def.other_fields.unwrap_or(false) || read_only;
    if let Some(fields) = def.fields.as_mut() {
        for field_def in fields.values_mut() {
            let field_read_only = field_def.read_only.unwrap_or(read_only);
            infer_deep_read_only_statuses(field_def, field_read_only);
            deep_read_only = deep_read_only && field_read_only && field_def.deep_read_only;
        }
    }
    if deep_read_only {
        def.deep_read_only = true;
    }
}

/// `standards.finalize`.
pub fn finalize(final_std: &mut Def) {
    infer_deep_read_only_statuses(final_std, true);
}

fn json_to_lval(value: &serde_json::Value) -> LVal {
    match value {
        serde_json::Value::Bool(b) => LVal::Bool(*b),
        serde_json::Value::Number(n) => LVal::Num(n.as_f64().unwrap_or(0.0)),
        serde_json::Value::String(s) => LVal::Str(s.chars().map(|c| c as u32 as u8).collect()),
        serde_json::Value::Object(map) => {
            let array = map
                .get("a")
                .and_then(serde_json::Value::as_array)
                .map(|items| items.iter().map(json_to_lval).collect())
                .unwrap_or_default();
            let hash = map
                .get("h")
                .and_then(serde_json::Value::as_array)
                .map(|pairs| {
                    pairs
                        .iter()
                        .filter_map(|pair| {
                            let pair = pair.as_array()?;
                            let key = pair.first()?.as_str()?;
                            Some((
                                LKey::Str(key.chars().map(|c| c as u32 as u8).collect()),
                                json_to_lval(pair.get(1)?),
                            ))
                        })
                        .collect()
                })
                .unwrap_or_default();
            LVal::table(array, hash)
        }
        _ => LVal::Other("nil"),
    }
}

/// The built-in standards by name, as user-format std tables.
pub fn builtin_standards() -> BTreeMap<Vec<u8>, Rc<LTable>> {
    let mut out = BTreeMap::new();
    for (name, value) in &super::tables().standards {
        if let LVal::Table(table) = json_to_lval(value) {
            out.insert(name.as_bytes().to_vec(), table);
        }
    }
    out
}
