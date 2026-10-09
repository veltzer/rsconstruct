//! Lua values as luacheck's option code sees them: options, standards and
//! configuration are plain Lua tables, validated by type at run time.
//!
//! A table keeps its array part (`ipairs`) apart from its other keys, and
//! has an identity: luacheck caches normalized options by option table
//! identity.

use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};

use super::stages::lua_number_to_string;

#[derive(Clone, Debug)]
pub enum LVal {
    Bool(bool),
    Num(f64),
    Str(Vec<u8>),
    Table(Rc<LTable>),
    /// A value of another type, by its `type()` name (`function`, ...).
    Other(&'static str),
}

#[derive(Clone, Debug, PartialEq)]
pub enum LKey {
    Str(Vec<u8>),
    Num(f64),
    Bool(bool),
    Other(&'static str),
}

#[derive(Debug, Default)]
pub struct LTable {
    pub id: usize,
    /// `t[1]`, `t[2]`, ... up to the first nil.
    pub array: Vec<LVal>,
    /// Every other key, in iteration order.
    pub hash: Vec<(LKey, LVal)>,
}

fn next_id() -> usize {
    static NEXT: AtomicUsize = AtomicUsize::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

impl LTable {
    pub fn new(array: Vec<LVal>, hash: Vec<(LKey, LVal)>) -> Self {
        Self {
            id: next_id(),
            array,
            hash,
        }
    }

    /// `t[key]` for a string key.
    pub fn get(&self, key: &str) -> Option<&LVal> {
        self.get_bytes(key.as_bytes())
    }

    pub fn get_bytes(&self, key: &[u8]) -> Option<&LVal> {
        self.hash.iter().find_map(|(k, v)| match k {
            LKey::Str(s) if s == key => Some(v),
            _ => None,
        })
    }

    /// `pairs(t)`: the array part as numeric keys, then the rest.
    pub fn pairs(&self) -> Vec<(LKey, LVal)> {
        let mut out: Vec<(LKey, LVal)> = self
            .array
            .iter()
            .enumerate()
            .map(|(i, v)| (LKey::Num((i + 1) as f64), v.clone()))
            .collect();
        out.extend(self.hash.iter().cloned());
        out
    }

    /// The values of `t[1..]` that are strings, assuming validation.
    pub fn strings(&self) -> Vec<Vec<u8>> {
        self.array
            .iter()
            .filter_map(|v| match v {
                LVal::Str(s) => Some(s.clone()),
                _ => None,
            })
            .collect()
    }
}

impl LVal {
    pub fn str(s: &str) -> Self {
        Self::Str(s.as_bytes().to_vec())
    }

    pub fn table(array: Vec<Self>, hash: Vec<(LKey, Self)>) -> Self {
        Self::Table(Rc::new(LTable::new(array, hash)))
    }

    /// `type(v)`.
    pub const fn type_name(&self) -> &'static str {
        match self {
            Self::Bool(_) => "boolean",
            Self::Num(_) => "number",
            Self::Str(_) => "string",
            Self::Table(_) => "table",
            Self::Other(name) => name,
        }
    }

    pub const fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(b) => Some(*b),
            _ => None,
        }
    }

    /// Lua truthiness of a present value (`nil` is represented by absence).
    pub const fn truthy(&self) -> bool {
        !matches!(self, Self::Bool(false))
    }
}

impl LKey {
    pub const fn type_name(&self) -> &'static str {
        match self {
            Self::Str(_) => "string",
            Self::Num(_) => "number",
            Self::Bool(_) => "boolean",
            Self::Other(name) => name,
        }
    }
}

/// `("%.20g"):format(n)`.
pub fn format_20g(n: f64) -> String {
    super::stages::format_g(n, 20)
}

/// `tostring(n)` for a number, as Lua 5.1 prints it.
pub fn number_to_string(n: f64) -> String {
    lua_number_to_string(n)
}
