//! Expression value types, a port of actionlint's `expr_type.go`.

use std::collections::BTreeMap;
use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub enum ExprType {
    /// Unknown statically; anything goes.
    Any,
    Null,
    Number,
    Bool,
    String,
    Object(ObjectType),
    Array(ArrayType),
}

/// An object: known properties plus, unless strict, a type every other
/// property has (`Any` for a loose object).
#[derive(Debug, Clone, PartialEq)]
pub struct ObjectType {
    pub props: BTreeMap<String, ExprType>,
    /// `None` means strict: no unknown properties.
    pub mapped: Option<Box<ExprType>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ArrayType {
    pub elem: Box<ExprType>,
    /// Derived from object filtering (`foo.*`).
    pub deref: bool,
}

impl ObjectType {
    pub fn empty() -> Self {
        Self {
            props: BTreeMap::new(),
            mapped: Some(Box::new(ExprType::Any)),
        }
    }

    pub fn new(props: BTreeMap<String, ExprType>) -> Self {
        Self {
            props,
            mapped: Some(Box::new(ExprType::Any)),
        }
    }

    pub const fn empty_strict() -> Self {
        Self {
            props: BTreeMap::new(),
            mapped: None,
        }
    }

    pub const fn strict(props: BTreeMap<String, ExprType>) -> Self {
        Self {
            props,
            mapped: None,
        }
    }

    pub fn map(t: ExprType) -> Self {
        Self {
            props: BTreeMap::new(),
            mapped: Some(Box::new(t)),
        }
    }

    pub const fn is_strict(&self) -> bool {
        self.mapped.is_none()
    }

    pub fn is_loose(&self) -> bool {
        matches!(self.mapped.as_deref(), Some(ExprType::Any))
    }

    pub fn make_loose(&mut self) {
        self.mapped = Some(Box::new(ExprType::Any));
    }
}

impl fmt::Display for ObjectType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(m) = &self.mapped {
            if self.is_loose() {
                f.write_str("object")
            } else {
                write!(f, "{{string => {m}}}")
            }
        } else {
            let parts: Vec<String> = self
                .props
                .iter()
                .map(|(k, v)| format!("{k}: {v}"))
                .collect();
            write!(f, "{{{}}}", parts.join("; "))
        }
    }
}

impl fmt::Display for ExprType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Any => f.write_str("any"),
            Self::Null => f.write_str("null"),
            Self::Number => f.write_str("number"),
            Self::Bool => f.write_str("bool"),
            Self::String => f.write_str("string"),
            Self::Object(o) => o.fmt(f),
            Self::Array(a) => write!(f, "array<{}>", a.elem),
        }
    }
}

impl ExprType {
    pub fn array(elem: Self, deref: bool) -> Self {
        Self::Array(ArrayType {
            elem: Box::new(elem),
            deref,
        })
    }

    /// Whether a value of `other` can be assigned where `self` is expected.
    pub fn assignable(&self, other: &Self) -> bool {
        match self {
            Self::Any => true,
            Self::Null => matches!(other, Self::Null | Self::Any),
            Self::Number => matches!(other, Self::Number | Self::Any),
            // Anything converts to bool: `if: ${{ steps.foo }}`.
            Self::Bool => true,
            Self::String => matches!(other, Self::String | Self::Number | Self::Any),
            Self::Object(ty) => match other {
                Self::Any => true,
                Self::Object(other) => {
                    if let Some(mapped) = &ty.mapped {
                        if let Some(other_mapped) = &other.mapped {
                            return mapped.assignable(other_mapped);
                        }
                        return other.props.values().all(|t| mapped.assignable(t));
                    }
                    if let Some(other_mapped) = &other.mapped {
                        return ty.props.values().all(|t| t.assignable(other_mapped));
                    }
                    other
                        .props
                        .iter()
                        .all(|(n, r)| ty.props.get(n).is_some_and(|l| l.assignable(r)))
                }
                _ => false,
            },
            Self::Array(ty) => match other {
                Self::Any => true,
                Self::Array(other) => ty.elem.assignable(&other.elem),
                _ => false,
            },
        }
    }

    /// The type of a value that is either `self` or `other`; `Any` when
    /// they conflict.
    pub fn merge(&self, other: &Self) -> Self {
        match self {
            Self::Any => Self::Any,
            Self::Null => {
                if matches!(other, Self::Null) {
                    Self::Null
                } else {
                    Self::Any
                }
            }
            Self::Number => match other {
                Self::Number => Self::Number,
                Self::String => Self::String,
                _ => Self::Any,
            },
            Self::Bool => match other {
                Self::Bool => Self::Bool,
                Self::String => Self::String,
                _ => Self::Any,
            },
            Self::String => match other {
                Self::String | Self::Number | Self::Bool => Self::String,
                _ => Self::Any,
            },
            Self::Object(ty) => match other {
                Self::Object(other) => {
                    if ty.props.is_empty() && other.is_loose() {
                        return Self::Object(other.clone());
                    }
                    if other.props.is_empty() && ty.is_loose() {
                        return Self::Object(ty.clone());
                    }
                    let mut mapped: Option<Self> = match (&ty.mapped, &other.mapped) {
                        (None, m) => m.as_deref().cloned(),
                        (Some(m), None) => Some((**m).clone()),
                        (Some(m), Some(o)) => Some(m.merge(o)),
                    };
                    let mut props = ty.props.clone();
                    for (n, r) in &other.props {
                        if let Some(l) = props.get(n) {
                            let merged = l.merge(r);
                            props.insert(n.clone(), merged);
                        } else {
                            props.insert(n.clone(), r.clone());
                            if let Some(m) = &mapped {
                                mapped = Some(m.merge(r));
                            }
                        }
                    }
                    Self::Object(ObjectType {
                        props,
                        mapped: mapped.map(Box::new),
                    })
                }
                _ => Self::Any,
            },
            Self::Array(ty) => match other {
                Self::Array(other) => {
                    if matches!(*ty.elem, Self::Any) {
                        return Self::Array(ty.clone());
                    }
                    if matches!(*other.elem, Self::Any) {
                        return Self::Array(other.clone());
                    }
                    Self::Array(ArrayType {
                        elem: Box::new(ty.elem.merge(&other.elem)),
                        deref: false,
                    })
                }
                _ => Self::Any,
            },
        }
    }
}

/// The type of a JSON value, as `fromJSON()` of a literal yields it.
pub fn type_of_json_value(v: &serde_json::Value) -> ExprType {
    match v {
        serde_json::Value::Bool(_) => ExprType::Bool,
        serde_json::Value::Number(_) => ExprType::Number,
        serde_json::Value::String(_) => ExprType::String,
        serde_json::Value::Array(items) => {
            let mut elem: Option<ExprType> = None;
            for e in items {
                let t = type_of_json_value(e);
                elem = Some(match elem {
                    None => t,
                    Some(prev) => prev.merge(&t),
                });
            }
            ExprType::array(elem.unwrap_or(ExprType::Any), false)
        }
        serde_json::Value::Object(map) => {
            let props = map
                .iter()
                .map(|(k, v)| (k.clone(), type_of_json_value(v)))
                .collect();
            ExprType::Object(ObjectType::strict(props))
        }
        serde_json::Value::Null => ExprType::Null,
    }
}
