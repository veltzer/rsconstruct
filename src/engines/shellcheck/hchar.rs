//! Haskell's `Data.Char` predicates, which go by Unicode general category.

use unicode_general_category::{GeneralCategory, get_general_category};

/// `isAlpha` / `isLetter`.
pub fn is_alpha(c: char) -> bool {
    matches!(
        get_general_category(c),
        GeneralCategory::UppercaseLetter
            | GeneralCategory::LowercaseLetter
            | GeneralCategory::TitlecaseLetter
            | GeneralCategory::ModifierLetter
            | GeneralCategory::OtherLetter
    )
}

/// `isUpper`.
pub fn is_upper(c: char) -> bool {
    matches!(
        get_general_category(c),
        GeneralCategory::UppercaseLetter | GeneralCategory::TitlecaseLetter
    )
}

/// `isLower`.
pub fn is_lower(c: char) -> bool {
    get_general_category(c) == GeneralCategory::LowercaseLetter
}

/// `isAlphaNum`: letters and numbers.
pub fn is_alpha_num(c: char) -> bool {
    is_alpha(c)
        || matches!(
            get_general_category(c),
            GeneralCategory::DecimalNumber
                | GeneralCategory::LetterNumber
                | GeneralCategory::OtherNumber
        )
}

pub const fn is_oct_digit(c: char) -> bool {
    matches!(c, '0'..='7')
}

/// `isSpace`.
pub fn is_space(c: char) -> bool {
    let u = c as u32;
    if u <= 0x377 {
        u == 32 || (9..=13).contains(&u) || u == 0xa0
    } else {
        get_general_category(c) == GeneralCategory::SpaceSeparator
    }
}

/// `isPrint`: everything but control, format, separators other than
/// space, surrogates, private use and unassigned.
pub fn is_print(c: char) -> bool {
    !matches!(
        get_general_category(c),
        GeneralCategory::Control
            | GeneralCategory::Format
            | GeneralCategory::LineSeparator
            | GeneralCategory::ParagraphSeparator
            | GeneralCategory::Surrogate
            | GeneralCategory::PrivateUse
            | GeneralCategory::Unassigned
    )
}

/// `toLower` (simple case mapping).
pub fn to_lower(c: char) -> char {
    let mut it = c.to_lowercase();
    match (it.next(), it.next()) {
        (Some(l), None) => l,
        (Some(l), Some(_)) if c == '\u{130}' => l,
        _ => c,
    }
}

/// `toUpper` (simple case mapping).
pub fn to_upper(c: char) -> char {
    let mut it = c.to_uppercase();
    match (it.next(), it.next()) {
        (Some(u), None) => u,
        _ => c,
    }
}

pub fn lower_string(s: &str) -> String {
    s.chars().map(to_lower).collect()
}
