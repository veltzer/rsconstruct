use crate::config::variables::{
    value_to_toml_inline, remove_vars_section, extract_var_names, substitute_variables,
};

// Tests for value_to_toml_inline

#[test]
fn value_to_toml_inline_string() {
    let value = toml::Value::String("hello".into());
    assert_eq!(value_to_toml_inline(&value), "\"hello\"");
}

#[test]
fn value_to_toml_inline_string_with_quotes() {
    let value = toml::Value::String("say \"hello\"".into());
    assert_eq!(value_to_toml_inline(&value), "\"say \\\"hello\\\"\"");
}

#[test]
fn value_to_toml_inline_string_with_backslash() {
    let value = toml::Value::String("path\\to\\file".into());
    assert_eq!(value_to_toml_inline(&value), "\"path\\\\to\\\\file\"");
}

#[test]
fn value_to_toml_inline_integer() {
    let value = toml::Value::Integer(42);
    assert_eq!(value_to_toml_inline(&value), "42");
}

#[test]
fn value_to_toml_inline_negative_integer() {
    let value = toml::Value::Integer(-123);
    assert_eq!(value_to_toml_inline(&value), "-123");
}

#[test]
fn value_to_toml_inline_float() {
    let value = toml::Value::Float(2.72);
    assert_eq!(value_to_toml_inline(&value), "2.72");
}

#[test]
fn value_to_toml_inline_boolean_true() {
    let value = toml::Value::Boolean(true);
    assert_eq!(value_to_toml_inline(&value), "true");
}

#[test]
fn value_to_toml_inline_boolean_false() {
    let value = toml::Value::Boolean(false);
    assert_eq!(value_to_toml_inline(&value), "false");
}

#[test]
fn value_to_toml_inline_array_of_strings() {
    let value = toml::Value::Array(vec![
        toml::Value::String("a".into()),
        toml::Value::String("b".into()),
        toml::Value::String("c".into()),
    ]);
    assert_eq!(value_to_toml_inline(&value), "[\"a\", \"b\", \"c\"]");
}

#[test]
fn value_to_toml_inline_array_of_integers() {
    let value = toml::Value::Array(vec![
        toml::Value::Integer(1),
        toml::Value::Integer(2),
        toml::Value::Integer(3),
    ]);
    assert_eq!(value_to_toml_inline(&value), "[1, 2, 3]");
}

#[test]
fn value_to_toml_inline_empty_array() {
    let value = toml::Value::Array(vec![]);
    assert_eq!(value_to_toml_inline(&value), "[]");
}

#[test]
fn value_to_toml_inline_table() {
    let mut table = toml::map::Map::new();
    table.insert("key".into(), toml::Value::String("value".into()));
    let value = toml::Value::Table(table);
    assert_eq!(value_to_toml_inline(&value), "{ key = \"value\" }");
}

// Tests for remove_vars_section

#[test]
fn remove_vars_section_basic() {
    let content = "[vars]\nfoo = \"bar\"\n\n[other]\nkey = \"value\"\n";
    let result = remove_vars_section(content);
    assert!(!result.contains("[vars]"));
    assert!(!result.contains("foo = \"bar\""));
    assert!(result.contains("[other]"));
    assert!(result.contains("key = \"value\""));
}

#[test]
fn remove_vars_section_at_end() {
    let content = "[other]\nkey = \"value\"\n\n[vars]\nfoo = \"bar\"\n";
    let result = remove_vars_section(content);
    assert!(!result.contains("[vars]"));
    assert!(!result.contains("foo = \"bar\""));
    assert!(result.contains("[other]"));
    assert!(result.contains("key = \"value\""));
}

#[test]
fn remove_vars_section_no_vars() {
    let content = "[other]\nkey = \"value\"\n";
    let result = remove_vars_section(content);
    assert_eq!(result, "[other]\nkey = \"value\"\n");
}

#[test]
fn remove_vars_section_multiple_vars() {
    let content = "[vars]\nfoo = \"bar\"\nbaz = [1, 2, 3]\n\n[other]\nkey = \"value\"\n";
    let result = remove_vars_section(content);
    assert!(!result.contains("[vars]"));
    assert!(!result.contains("foo = \"bar\""));
    assert!(!result.contains("baz = [1, 2, 3]"));
    assert!(result.contains("[other]"));
}

// Tests for extract_var_names

#[test]
fn extract_var_names_basic() {
    let content = "[vars]\nfoo = \"bar\"\nbaz = [1, 2]\n\n[other]\nkey = \"value\"\n";
    let names = extract_var_names(content);
    assert_eq!(names, vec!["foo", "baz"]);
}

#[test]
fn extract_var_names_no_vars_section() {
    let content = "[other]\nkey = \"value\"\n";
    let names = extract_var_names(content);
    assert!(names.is_empty());
}

#[test]
fn extract_var_names_empty_vars_section() {
    let content = "[vars]\n\n[other]\nkey = \"value\"\n";
    let names = extract_var_names(content);
    assert!(names.is_empty());
}

#[test]
fn extract_var_names_with_comments() {
    let content = "[vars]\n# This is a comment\nfoo = \"bar\"\n# Another comment\nbaz = 42\n";
    let names = extract_var_names(content);
    assert_eq!(names, vec!["foo", "baz"]);
}

#[test]
fn extract_var_names_with_whitespace() {
    let content = "[vars]\n  foo   =   \"bar\"\n\tbaz\t=\t42\n";
    let names = extract_var_names(content);
    assert_eq!(names, vec!["foo", "baz"]);
}

// Tests for substitute_variables

#[test]
fn substitute_variables_string() {
    let content = "[vars]\nmy_dir = \"templates\"\n\n[processor]\nscan_dir = \"${my_dir}\"\n";
    let result = substitute_variables(content).expect("variable substitution failed");
    assert!(result.contains("scan_dir = \"templates\""));
    assert!(!result.contains("${my_dir}"));
    assert!(!result.contains("[vars]"));
}

#[test]
fn substitute_variables_array() {
    let content = "[vars]\nexcludes = [\"/a/\", \"/b/\"]\n\n[processor]\nexclude_dirs = \"${excludes}\"\n";
    let result = substitute_variables(content).expect("variable substitution failed");
    assert!(result.contains("exclude_dirs = [\"/a/\", \"/b/\"]"));
    assert!(!result.contains("${excludes}"));
}

#[test]
fn substitute_variables_multiple_uses() {
    let content = "[vars]\nval = \"shared\"\n\n[a]\nx = \"${val}\"\n\n[b]\ny = \"${val}\"\n";
    let result = substitute_variables(content).expect("variable substitution failed");
    assert!(result.contains("x = \"shared\""));
    assert!(result.contains("y = \"shared\""));
}

#[test]
fn substitute_variables_no_vars_section() {
    let content = "[processor]\nscan_dir = \"src\"\n";
    let result = substitute_variables(content).expect("variable substitution failed");
    assert_eq!(result, content);
}

#[test]
fn substitute_variables_undefined_error() {
    let content = "[processor]\nscan_dir = \"${undefined}\"\n";
    let result = substitute_variables(content);
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("Undefined variable"));
    assert!(err.contains("undefined"));
}

#[test]
fn substitute_variables_undefined_with_vars_section() {
    let content = "[vars]\nfoo = \"bar\"\n\n[processor]\nx = \"${foo}\"\ny = \"${missing}\"\n";
    let result = substitute_variables(content);
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("missing"));
}

#[test]
fn substitute_variables_integer() {
    let content = "[vars]\ncount = 42\n\n[processor]\nvalue = \"${count}\"\n";
    let result = substitute_variables(content).expect("variable substitution failed");
    assert!(result.contains("value = 42"));
}

#[test]
fn substitute_variables_boolean() {
    let content = "[vars]\nenabled = true\n\n[processor]\nflag = \"${enabled}\"\n";
    let result = substitute_variables(content).expect("variable substitution failed");
    assert!(result.contains("flag = true"));
}
