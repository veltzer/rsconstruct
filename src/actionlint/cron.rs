//! Cron schedule parsing and scheduling, a port of the parts of
//! robfig/cron v3 that actionlint uses: the five-field standard parser
//! (minute, hour, day of month, month, day of week) and `Next`, which gives
//! the first run after a time.

use crate::actionlint::yaml::go_quote;

const STAR_BIT: u64 = 1 << 63;

struct Bounds {
    min: u32,
    max: u32,
    names: &'static [(&'static str, u32)],
}

const MINUTES: Bounds = Bounds {
    min: 0,
    max: 59,
    names: &[],
};
const HOURS: Bounds = Bounds {
    min: 0,
    max: 23,
    names: &[],
};
const DOM: Bounds = Bounds {
    min: 1,
    max: 31,
    names: &[],
};
const MONTHS: Bounds = Bounds {
    min: 1,
    max: 12,
    names: &[
        ("jan", 1),
        ("feb", 2),
        ("mar", 3),
        ("apr", 4),
        ("may", 5),
        ("jun", 6),
        ("jul", 7),
        ("aug", 8),
        ("sep", 9),
        ("oct", 10),
        ("nov", 11),
        ("dec", 12),
    ],
};
const DOW: Bounds = Bounds {
    min: 0,
    max: 6,
    names: &[
        ("sun", 0),
        ("mon", 1),
        ("tue", 2),
        ("wed", 3),
        ("thu", 4),
        ("fri", 5),
        ("sat", 6),
    ],
};

#[derive(Debug, Clone, Copy)]
pub struct Schedule {
    minute: u64,
    hour: u64,
    dom: u64,
    month: u64,
    dow: u64,
}

/// Parses a standard five-field cron spec; the error text is robfig/cron's.
pub fn parse(spec: &str) -> Result<Schedule, String> {
    if spec.is_empty() {
        return Err("empty spec string".to_string());
    }
    let mut spec = spec;
    if spec.starts_with("TZ=") || spec.starts_with("CRON_TZ=") {
        let (Some(i), Some(eq)) = (spec.find(' '), spec.find('=')) else {
            return Err(format!(
                "provided bad location {spec}: unknown time zone {spec}"
            ));
        };
        let name = &spec[eq + 1..i];
        if !crate::actionlint::rules::events::timezone_exists(name) {
            return Err(format!(
                "provided bad location {name}: unknown time zone {name}"
            ));
        }
        spec = spec[i..].trim();
    }
    if spec.starts_with('@') {
        return Err(format!("parser does not accept descriptors: {spec}"));
    }
    let fields: Vec<&str> = spec.split_whitespace().collect();
    if fields.len() != 5 {
        return Err(format!(
            "expected exactly 5 fields, found {}: [{}]",
            fields.len(),
            fields.join(" ")
        ));
    }
    Ok(Schedule {
        minute: get_field(fields[0], &MINUTES)?,
        hour: get_field(fields[1], &HOURS)?,
        dom: get_field(fields[2], &DOM)?,
        month: get_field(fields[3], &MONTHS)?,
        dow: get_field(fields[4], &DOW)?,
    })
}

fn get_field(field: &str, r: &Bounds) -> Result<u64, String> {
    let mut bits = 0u64;
    for expr in field.split(',').filter(|s| !s.is_empty()) {
        bits |= get_range(expr, r)?;
    }
    Ok(bits)
}

fn get_range(expr: &str, r: &Bounds) -> Result<u64, String> {
    let range_and_step: Vec<&str> = expr.split('/').collect();
    let low_and_high: Vec<&str> = range_and_step[0].split('-').collect();
    let single_digit = low_and_high.len() == 1;
    let (start, mut end, mut extra) = if low_and_high[0] == "*" || low_and_high[0] == "?" {
        (r.min, r.max, STAR_BIT)
    } else {
        let start = parse_int_or_name(low_and_high[0], r.names)?;
        let end = match low_and_high.len() {
            1 => start,
            2 => parse_int_or_name(low_and_high[1], r.names)?,
            _ => return Err(format!("too many hyphens: {expr}")),
        };
        (start, end, 0u64)
    };
    let step = match range_and_step.len() {
        1 => 1,
        2 => {
            let step = must_parse_int(range_and_step[1])?;
            if single_digit {
                end = r.max;
            }
            if step > 1 {
                extra = 0;
            }
            step
        }
        _ => return Err(format!("too many slashes: {expr}")),
    };
    if start < r.min {
        return Err(format!(
            "beginning of range ({start}) below minimum ({}): {expr}",
            r.min
        ));
    }
    if end > r.max {
        return Err(format!(
            "end of range ({end}) above maximum ({}): {expr}",
            r.max
        ));
    }
    if start > end {
        return Err(format!(
            "beginning of range ({start}) beyond end of range ({end}): {expr}"
        ));
    }
    if step == 0 {
        return Err(format!("step of range should be a positive number: {expr}"));
    }
    Ok(get_bits(start, end, step) | extra)
}

fn parse_int_or_name(expr: &str, names: &[(&str, u32)]) -> Result<u32, String> {
    let lower = expr.to_lowercase();
    if let Some((_, v)) = names.iter().find(|(n, _)| *n == lower) {
        return Ok(*v);
    }
    must_parse_int(expr)
}

fn must_parse_int(expr: &str) -> Result<u32, String> {
    let body = expr.strip_prefix(['+', '-']).unwrap_or(expr);
    if body.is_empty() || !body.bytes().all(|b| b.is_ascii_digit()) {
        return Err(format!(
            "failed to parse int from {expr}: strconv.Atoi: parsing {}: invalid syntax",
            go_quote(expr)
        ));
    }
    let num: i64 = expr.parse().map_err(|_| {
        format!(
            "failed to parse int from {expr}: strconv.Atoi: parsing {}: value out of range",
            go_quote(expr)
        )
    })?;
    if num < 0 {
        return Err(format!("negative number ({num}) not allowed: {expr}"));
    }
    u32::try_from(num).map_err(|_| format!("end of range ({num}) above maximum: {expr}"))
}

const fn get_bits(min: u32, max: u32, step: u32) -> u64 {
    if step == 1 {
        return !(u64::MAX << (max + 1)) & (u64::MAX << min);
    }
    let mut bits = 0u64;
    let mut i = min;
    while i <= max {
        bits |= 1 << i;
        i += step;
    }
    bits
}

/// A UTC civil time with second precision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Civil {
    year: i64,
    month: u32,
    day: u32,
    hour: u32,
    minute: u32,
    second: u32,
}

const fn days_in_month(year: i64, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        _ => {
            if (year % 4 == 0 && year % 100 != 0) || year % 400 == 0 {
                29
            } else {
                28
            }
        }
    }
}

/// Days since 1970-01-01 (Howard Hinnant's algorithm).
fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (i64::from(m) + 9) % 12;
    let doy = (153 * mp + 2) / 5 + i64::from(d) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

impl Civil {
    const EPOCH: Self = Self {
        year: 1970,
        month: 1,
        day: 1,
        hour: 0,
        minute: 0,
        second: 0,
    };

    fn to_seconds(self) -> i64 {
        days_from_civil(self.year, self.month, self.day) * 86_400
            + i64::from(self.hour) * 3600
            + i64::from(self.minute) * 60
            + i64::from(self.second)
    }

    /// 0 = Sunday.
    fn weekday(self) -> u32 {
        let days = days_from_civil(self.year, self.month, self.day);
        u32::try_from((days + 4).rem_euclid(7)).unwrap_or(0)
    }

    fn add_seconds(self, secs: i64) -> Self {
        let total = self.to_seconds() + secs;
        let days = total.div_euclid(86_400);
        let rem = total.rem_euclid(86_400);
        // Civil from days (Hinnant).
        let z = days + 719_468;
        let era = z.div_euclid(146_097);
        let doe = z - era * 146_097;
        let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
        let y = yoe + era * 400;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let d = doy - (153 * mp + 2) / 5 + 1;
        let m = if mp < 10 { mp + 3 } else { mp - 9 };
        let y = if m <= 2 { y + 1 } else { y };
        Self {
            year: y,
            month: u32::try_from(m).unwrap_or(1),
            day: u32::try_from(d).unwrap_or(1),
            hour: u32::try_from(rem / 3600).unwrap_or(0),
            minute: u32::try_from(rem % 3600 / 60).unwrap_or(0),
            second: u32::try_from(rem % 60).unwrap_or(0),
        }
    }

    fn add_months(self, n: i64) -> Self {
        // Go's AddDate normalizes an overflowing day into the next month.
        let total = self.year * 12 + i64::from(self.month) - 1 + n;
        let year = total.div_euclid(12);
        let month = u32::try_from(total.rem_euclid(12) + 1).unwrap_or(1);
        let dim = days_in_month(year, month);
        let base = Self {
            year,
            month,
            day: self.day.min(dim),
            ..self
        };
        let overflow = i64::from(self.day) - i64::from(base.day);
        base.add_seconds(overflow * 86_400)
    }
}

impl Schedule {
    /// robfig/cron's `Next`: the first time after `t` (seconds since the
    /// epoch, UTC) the schedule matches; `None` when none within five years.
    pub fn next(&self, t: i64) -> Option<i64> {
        let mut t = Civil::EPOCH.add_seconds(t + 1);
        let mut added = false;
        let year_limit = t.year + 5;
        'wrap: loop {
            if t.year > year_limit {
                return None;
            }
            while (1u64 << t.month) & self.month == 0 {
                if !added {
                    added = true;
                    t = Civil {
                        day: 1,
                        hour: 0,
                        minute: 0,
                        second: 0,
                        ..t
                    };
                }
                t = t.add_months(1);
                if t.month == 1 {
                    continue 'wrap;
                }
            }
            while !self.day_matches(t) {
                if !added {
                    added = true;
                    t = Civil {
                        hour: 0,
                        minute: 0,
                        second: 0,
                        ..t
                    };
                }
                t = t.add_seconds(86_400);
                if t.day == 1 {
                    continue 'wrap;
                }
            }
            while (1u64 << t.hour) & self.hour == 0 {
                if !added {
                    added = true;
                    t = Civil {
                        minute: 0,
                        second: 0,
                        ..t
                    };
                }
                t = t.add_seconds(3600);
                if t.hour == 0 {
                    continue 'wrap;
                }
            }
            while (1u64 << t.minute) & self.minute == 0 {
                if !added {
                    added = true;
                    t = Civil { second: 0, ..t };
                }
                t = t.add_seconds(60);
                if t.minute == 0 {
                    continue 'wrap;
                }
            }
            // The seconds field is fixed at 0 for five-field specs.
            while t.second != 0 {
                if !added {
                    added = true;
                }
                t = t.add_seconds(1);
                if t.second == 0 {
                    continue 'wrap;
                }
            }
            return Some(t.to_seconds());
        }
    }

    fn day_matches(&self, t: Civil) -> bool {
        let day_of_month_ok = (1u64 << t.day) & self.dom > 0;
        let weekday_ok = (1u64 << t.weekday()) & self.dow > 0;
        if self.dom & STAR_BIT > 0 || self.dow & STAR_BIT > 0 {
            return day_of_month_ok && weekday_ok;
        }
        day_of_month_ok || weekday_ok
    }
}
