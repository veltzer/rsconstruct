//! The parts of version.pm PPI and the policies use: parsing a version
//! number as `version->parse` / `version->new` do, and comparing two.

use std::cmp::Ordering;

use super::Class;

/// A version as version.pm compares it: its components, a decimal's
/// fraction split in groups of three digits (`5.036` is `5.36`).
#[derive(Clone, Debug)]
pub struct Version(Vec<u64>);

impl Version {
    /// `version->parse($s)`, or the message version.pm dies with.
    pub fn parse_checked(s: &[u8]) -> Result<Self, &'static str> {
        prescan(s)?;
        Self::parse(s).ok_or("Invalid version format (non-numeric data)")
    }

    /// `version->parse($s)` once `prescan` has accepted it; `None` where
    /// version.pm dies ("Invalid version format").
    pub fn parse(s: &[u8]) -> Option<Self> {
        if prescan(s).is_err() {
            return None;
        }
        let s = s.trim_ascii();
        let dotted = s.first() == Some(&b'v') || super::count_byte(s, b'.') > 1;
        if dotted {
            // An underscore joins digits (`v1.2_3` is v1.23).
            let body: Vec<u8> = s
                .strip_prefix(b"v")
                .unwrap_or(s)
                .iter()
                .copied()
                .filter(|&c| c != b'_')
                .collect();
            // `.5.01` is v0.5.1: an empty component is 0.
            let parts: Vec<u64> = body
                .split(|&c| c == b'.')
                .map(|p| {
                    if p.is_empty() {
                        Some(0)
                    } else {
                        parse_digits(p)
                    }
                })
                .collect::<Option<_>>()?;
            if parts.is_empty() {
                return None;
            }
            return Some(Self(parts));
        }
        // An underscore may only follow the decimal point ("alpha without
        // decimal" otherwise).
        let (int, frac): (&[u8], Vec<u8>) = match s.iter().position(|&c| c == b'.') {
            Some(dot) => (
                &s[..dot],
                s[dot + 1..]
                    .iter()
                    .copied()
                    .filter(|&c| c != b'_')
                    .collect(),
            ),
            None => (s, Vec::new()),
        };
        let frac = frac.as_slice();
        // `.006` is 0.006.
        let major = if int.is_empty() && !frac.is_empty() {
            Some(0)
        } else {
            parse_digits(int)
        };
        let mut parts = vec![major?];
        if !frac.is_empty() && !frac.iter().all(u8::is_ascii_digit) {
            return None;
        }
        for chunk in frac.chunks(3) {
            let mut group = 0;
            for i in 0..3 {
                group = group * 10 + chunk.get(i).map_or(0, |d| u64::from(d - b'0'));
            }
            parts.push(group);
        }
        Some(Self(parts))
    }

    /// The decimal version `major.minor` (minor in thousandths): `5.035` is
    /// `decimal(5, 35)`.
    pub fn decimal(major: u64, minor: u64) -> Self {
        Self(vec![major, minor])
    }

    /// A dotted version `vA.B.C`.
    pub fn dotted(parts: &[u64]) -> Self {
        Self(parts.to_vec())
    }
}

impl PartialEq for Version {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Eq for Version {}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> Ordering {
        let n = self.0.len().max(other.0.len());
        for i in 0..n {
            let a = self.0.get(i).copied().unwrap_or(0);
            let b = other.0.get(i).copied().unwrap_or(0);
            match a.cmp(&b) {
                Ordering::Equal => {}
                unequal => return unequal,
            }
        }
        Ordering::Equal
    }
}

/// vutil.c's `prescan_version` (lax): whether version.pm accepts a
/// string, or its "Invalid version format" message.
fn prescan(s: &[u8]) -> Result<(), &'static str> {
    let s = s.trim_ascii_start();
    let at = |i: usize| s.get(i).copied().unwrap_or(0);
    let is_end = |c: u8| c == 0 || c == b';' || c.is_ascii_whitespace() || c == b'{' || c == b'}';
    let mut saw_decimal = 0;
    let mut alpha = false;
    let mut dotted = at(0) == b'v';
    let mut d = usize::from(dotted);
    if dotted && !at(d).is_ascii_digit() {
        return Err(
            "Invalid version format (dotted-decimal versions require at least three parts)",
        );
    }
    if !dotted {
        if at(d) == b'-' {
            return Err("Invalid version format (negative version number)");
        }
        while at(d).is_ascii_digit() {
            d += 1;
        }
        if at(d) == b'.' {
            saw_decimal += 1;
            d += 1;
        } else if is_end(at(d)) {
            if d == 0 {
                return Err("Invalid version format (version required)");
            }
            return finish(s, d, saw_decimal);
        } else if d == 0 {
            return Err("Invalid version format (non-numeric data)");
        } else if at(d) == b'_' {
            if at(d + 1).is_ascii_digit() {
                return Err("Invalid version format (alpha without decimal)");
            }
            return Err("Invalid version format (misplaced underscore)");
        } else {
            return Err("Invalid version format (non-numeric data)");
        }
        if !at(d).is_ascii_digit() && !is_end(at(d)) {
            return Err("Invalid version format (fractional part required)");
        }
        while at(d).is_ascii_digit() {
            d += 1;
            if at(d) == b'.' && at(d - 1).is_ascii_digit() {
                if alpha {
                    return Err("Invalid version format (underscores before decimal)");
                }
                // A second decimal point: start over as dotted-decimal
                // (the decimal points seen so far still count).
                d = 0;
                dotted = true;
                break;
            }
            if at(d) == b'_' {
                if alpha {
                    return Err("Invalid version format (multiple underscores)");
                }
                if !at(d + 1).is_ascii_digit() {
                    return Err("Invalid version format (misplaced underscore)");
                }
                d += 1;
                alpha = true;
            }
        }
        if !dotted {
            return finish(s, d, saw_decimal);
        }
    }
    // Dotted decimal: the integer part, then digit groups.
    while at(d).is_ascii_digit() {
        d += 1;
    }
    if at(d) == b'.' {
        saw_decimal += 1;
        d += 1;
    } else {
        return finish(s, d, saw_decimal);
    }
    while at(d).is_ascii_digit() {
        while at(d).is_ascii_digit() {
            d += 1;
        }
        if at(d) == b'_' {
            if alpha {
                return Err("Invalid version format (multiple underscores)");
            }
            d += 1;
            alpha = true;
        } else if at(d) == b'.' {
            if alpha {
                return Err("Invalid version format (underscores before decimal)");
            }
            saw_decimal += 1;
            d += 1;
        } else {
            break;
        }
    }
    finish(s, d, saw_decimal)
}

/// `version_prescan_finish`.
fn finish(s: &[u8], mut d: usize, saw_decimal: usize) -> Result<(), &'static str> {
    while s.get(d).is_some_and(u8::is_ascii_whitespace) {
        d += 1;
    }
    let c = s.get(d).copied().unwrap_or(0);
    if !c.is_ascii_digit() && c != 0 && c != b';' && c != b'{' && c != b'}' {
        return Err("Invalid version format (non-numeric data)");
    }
    if saw_decimal > 1 && d > 0 && s[d - 1] == b'.' {
        return Err("Invalid version format (trailing decimal)");
    }
    Ok(())
}

/// A Number token's `literal` as Perl stringifies it, `None` where it
/// returns nothing (a token with an `_error`).
pub fn number_literal(class: Class, text: &[u8]) -> Option<Vec<u8>> {
    let cleaned: Vec<u8> = text.iter().copied().filter(|&c| c != b'_').collect();
    let cleaned = cleaned.strip_prefix(b"+").unwrap_or(&cleaned);
    let (neg, body) = match cleaned.strip_prefix(b"-") {
        Some(rest) => (true, rest),
        None => (false, cleaned),
    };
    let signed = |v: f64| if neg { -v } else { v };
    let value = match class {
        Class::NumberVersion => {
            let body = text.strip_prefix(b"v").unwrap_or(text);
            return Some(
                body.split(|&c| c == b'.')
                    .filter_map(|p| std::str::from_utf8(p).ok()?.parse::<u32>().ok())
                    .map(|n| n as u8)
                    .collect(),
            );
        }
        Class::NumberHex => {
            let digits = body
                .strip_prefix(b"0x")
                .or_else(|| body.strip_prefix(b"0X"))
                .unwrap_or(body);
            let digits: String = digits
                .iter()
                .take_while(|c| c.is_ascii_hexdigit())
                .map(|&c| c as char)
                .collect();
            signed(
                u64::from_str_radix(if digits.is_empty() { "0" } else { &digits }, 16).ok()? as f64,
            )
        }
        Class::NumberBinary => {
            let digits = body
                .strip_prefix(b"0b")
                .or_else(|| body.strip_prefix(b"0B"))
                .unwrap_or(body);
            if digits.iter().any(|&c| c != b'0' && c != b'1') {
                return None;
            }
            let digits = std::str::from_utf8(digits).ok()?;
            signed(if digits.is_empty() {
                0.0
            } else {
                u64::from_str_radix(digits, 2).ok()? as f64
            })
        }
        Class::NumberOctal => {
            let digits = body
                .strip_prefix(b"0o")
                .or_else(|| body.strip_prefix(b"0O"))
                .unwrap_or(body);
            if digits.iter().any(|&c| c == b'8' || c == b'9') {
                return None;
            }
            let digits: String = digits
                .iter()
                .take_while(|c| (b'0'..=b'7').contains(c))
                .map(|&c| c as char)
                .collect();
            signed(if digits.is_empty() {
                0.0
            } else {
                u64::from_str_radix(&digits, 8).ok()? as f64
            })
        }
        Class::NumberExp => {
            if text.last().is_some_and(|&c| c == b'e' || c == b'E') {
                return None;
            }
            let s = std::str::from_utf8(body).ok()?;
            let s = if s.starts_with('.') {
                format!("0{s}")
            } else {
                s.to_string()
            };
            signed(s.parse::<f64>().ok()?)
        }
        _ => {
            let s = std::str::from_utf8(body).ok()?;
            let s = if s.starts_with('.') {
                format!("0{s}")
            } else {
                s.to_string()
            };
            let s = s.strip_suffix('.').unwrap_or(&s);
            signed(s.parse::<f64>().unwrap_or(0.0))
        }
    };
    Some(perl_number(value).into_bytes())
}

/// Perl's stringification of a number (`%.15g`, integers as integers).
#[allow(clippy::float_cmp)] // An exact integer test, as Perl's is.
fn perl_number(v: f64) -> String {
    if v == v.trunc() && v.abs() < 1e15 {
        return format!("{}", v as i64);
    }
    let exp = v.abs().log10().floor() as i32;
    if (-5..15).contains(&exp) {
        let decimals = (14 - exp).max(0) as usize;
        let s = format!("{v:.decimals$}");
        let s = s.trim_end_matches('0');
        return s.strip_suffix('.').unwrap_or(s).to_string();
    }
    let s = format!("{v:.14e}");
    let (mantissa, exponent) = s.split_once('e').unwrap_or((&s, "0"));
    let mantissa = mantissa.trim_end_matches('0');
    let mantissa = mantissa.strip_suffix('.').unwrap_or(mantissa);
    let exponent: i32 = exponent.parse().unwrap_or(0);
    let sign = if exponent < 0 { '-' } else { '+' };
    format!("{mantissa}e{sign}{:02}", exponent.abs())
}

fn parse_digits(s: &[u8]) -> Option<u64> {
    if s.is_empty() || !s.iter().all(u8::is_ascii_digit) {
        return None;
    }
    let mut n: u64 = 0;
    for &d in s {
        n = n.saturating_mul(10).saturating_add(u64::from(d - b'0'));
    }
    Some(n)
}

/// A Number token's `literal`, as a number (`None` where PPI's is not a
/// number: a v-string, a token with an error).
pub fn numeric_literal(text: &[u8]) -> Option<f64> {
    let s: Vec<u8> = text.iter().copied().filter(|&c| c != b'_').collect();
    let s = s.strip_prefix(b"+").unwrap_or(&s);
    let (neg, s) = match s.strip_prefix(b"-") {
        Some(rest) => (true, rest),
        None => (false, s),
    };
    let value = if let Some(hex) = s.strip_prefix(b"0x").or_else(|| s.strip_prefix(b"0X")) {
        u64::from_str_radix(std::str::from_utf8(hex).ok()?, 16).ok()? as f64
    } else if let Some(bin) = s.strip_prefix(b"0b").or_else(|| s.strip_prefix(b"0B")) {
        u64::from_str_radix(std::str::from_utf8(bin).ok()?, 2).ok()? as f64
    } else if s.len() > 1 && s[0] == b'0' && s.iter().all(u8::is_ascii_digit) {
        u64::from_str_radix(std::str::from_utf8(&s[1..]).ok()?, 8).ok()? as f64
    } else {
        std::str::from_utf8(s).ok()?.parse::<f64>().ok()?
    };
    Some(if neg { -value } else { value })
}

#[cfg(test)]
mod tests {
    use super::Version;

    fn v(s: &str) -> Version {
        Version::parse(s.as_bytes()).expect("parses")
    }

    #[test]
    fn decimal_and_dotted_compare_as_version_pm() {
        assert!(v("5.036") >= Version::decimal(5, 35));
        assert!(v("v5.36") >= Version::decimal(5, 35));
        assert!(v("5.36.0") >= Version::decimal(5, 35));
        assert!(v("5.034") < Version::decimal(5, 35));
        assert!(v("5.010_001") > Version::decimal(5, 10));
        assert!(v("5.4") > Version::decimal(5, 6));
        assert_eq!(v("5.006"), Version::decimal(5, 6));
        assert!(v("v5.12") >= Version::dotted(&[5, 11, 0]));
        assert!(Version::parse(b"0x10").is_none());
        assert!(Version::parse(b"1_2.3").is_none());
        assert_eq!(v(".006"), Version::dotted(&[0, 6, 0]));
        assert_eq!(v("5."), Version::dotted(&[5]));
        assert_eq!(v("v1.2_3"), Version::dotted(&[1, 23]));
        assert_eq!(v("1.2.3_4"), Version::dotted(&[1, 2, 34]));
        assert_eq!(v(".5.01"), Version::dotted(&[0, 5, 1]));
    }
}
