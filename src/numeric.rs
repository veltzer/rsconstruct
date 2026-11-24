//! Integer to float conversions that say what they do.
//!
//! `n as f64` rounds an integer wider than 53 bits to the nearest float
//! without saying so. The functions here do the same rounding (to nearest,
//! ties to even, so the result is bit-identical to the cast) but spell it
//! out: the integer is split into two exactly representable halves and
//! recombined with one fused multiply-add, which rounds once.

/// A length, count or offset as an `i64`. A `usize` that indexes memory
/// never exceeds `isize::MAX`, so it always fits.
pub fn to_i64(n: usize) -> i64 {
    i64::try_from(n).expect("a length or offset fits an i64")
}

/// A count of files, pages or table entries as a `u32`.
pub fn to_u32(n: usize) -> u32 {
    u32::try_from(n).expect("a count of files or pages fits a u32")
}

/// A duration in whole microseconds, saturating at `i64::MAX`.
pub fn duration_micros(d: std::time::Duration) -> i64 {
    i64::try_from(d.as_micros()).unwrap_or(i64::MAX)
}

/// A duration in whole milliseconds, saturating at `u64::MAX`.
pub fn duration_millis(d: std::time::Duration) -> u64 {
    u64::try_from(d.as_millis()).unwrap_or(u64::MAX)
}

/// A length or count as an `i32`; for the lengths of a source line or a
/// token, which are far below `i32::MAX`.
pub fn to_i32(n: usize) -> i32 {
    i32::try_from(n).expect("a line or token length fits an i32")
}

/// A length, count or offset as an `isize`. A `usize` that indexes memory
/// never exceeds `isize::MAX`, so it always fits.
pub fn to_isize(n: usize) -> isize {
    isize::try_from(n).expect("a length or offset fits an isize")
}

/// The low eight bits of `n`, as C's conversion of an `int` to a `char`
/// keeps them.
pub fn low_byte(n: u32) -> u8 {
    u8::try_from(n & 0xFF).expect("masked to eight bits")
}

/// A position or size a parser reported as `u64`, as a `usize`. It
/// describes a buffer held in memory, so it fits.
pub fn u64_to_usize(n: u64) -> usize {
    usize::try_from(n).expect("a position in an in-memory buffer fits a usize")
}

/// `n` as the nearest `f64`.
pub fn u64_to_f64(n: u64) -> f64 {
    let high = u32::try_from(n >> 32).expect("the upper half of a u64 fits a u32");
    let low = u32::try_from(n & u64::from(u32::MAX)).expect("the lower half of a u64 fits a u32");
    f64::from(high).mul_add(4_294_967_296.0, f64::from(low))
}

/// `n` as the nearest `f64`.
pub fn i64_to_f64(n: i64) -> f64 {
    let magnitude = u64_to_f64(n.unsigned_abs());
    if n < 0 { -magnitude } else { magnitude }
}

/// `n` as the nearest `f64`.
pub fn usize_to_f64(n: usize) -> f64 {
    u64_to_f64(u64::try_from(n).expect("a usize fits a u64"))
}

/// `f` truncated toward zero, saturating at the ends of the `i64` range,
/// with NaN as 0: what `f as i64` does, done on the bits of the float.
pub fn f64_to_i64(f: f64) -> i64 {
    // 2^63, the first magnitude that no longer fits.
    const LIMIT: f64 = 9_223_372_036_854_775_808.0;
    if f.is_nan() {
        return 0;
    }
    let whole = f.trunc();
    if whole >= LIMIT {
        return i64::MAX;
    }
    if whole <= -LIMIT {
        return i64::MIN;
    }
    let bits = whole.to_bits();
    let biased_exponent = (bits >> 52) & 0x7FF;
    if biased_exponent < 1023 {
        // Below one in magnitude, so zero once truncated.
        return 0;
    }
    // The float is `mantissa * 2^(exponent - 52)` with a 53-bit mantissa.
    let exponent = biased_exponent - 1023;
    let mantissa = (bits & ((1 << 52) - 1)) | (1 << 52);
    let magnitude = if exponent >= 52 {
        mantissa << (exponent - 52)
    } else {
        mantissa >> (52 - exponent)
    };
    let magnitude = i64::try_from(magnitude).expect("a magnitude below 2^63 fits an i64");
    if bits >> 63 == 1 {
        -magnitude
    } else {
        magnitude
    }
}

/// `f` truncated toward zero and clamped to the `i32` range.
pub fn f64_to_i32(f: f64) -> i32 {
    let clamped = f64_to_i64(f).clamp(i64::from(i32::MIN), i64::from(i32::MAX));
    i32::try_from(clamped).expect("the value was clamped to the i32 range")
}

/// `f` truncated toward zero and clamped to the `u32` range.
pub fn f64_to_u32(f: f64) -> u32 {
    let clamped = f64_to_i64(f).clamp(0, i64::from(u32::MAX));
    u32::try_from(clamped).expect("the value was clamped to the u32 range")
}

/// `f` truncated toward zero, with negative values as 0.
pub fn f64_to_usize(f: f64) -> usize {
    usize::try_from(f64_to_i64(f).max(0)).expect("a non-negative i64 fits a usize")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Parsing the decimal text is correctly rounded, so it is an oracle
    /// independent of the arithmetic under test.
    fn parsed(n: &impl ToString) -> f64 {
        n.to_string().parse().unwrap()
    }

    #[test]
    fn unsigned_matches_the_correctly_rounded_value() {
        for n in [
            0,
            1,
            4_294_967_295,
            4_294_967_296,
            (1 << 53) - 1,
            1 << 53,
            (1 << 53) + 1,
            (1 << 53) + 3,
            u64::MAX - 1024,
            u64::MAX,
        ] {
            assert_eq!(u64_to_f64(n), parsed(&n), "{n}");
        }
    }

    #[test]
    fn signed_matches_the_correctly_rounded_value() {
        for n in [
            0,
            -1,
            i64::from(i32::MIN),
            -(1 << 53) - 1,
            -(1 << 53) - 3,
            i64::MIN,
            i64::MAX,
        ] {
            assert_eq!(i64_to_f64(n), parsed(&n), "{n}");
        }
    }

    #[test]
    fn float_to_integer_truncates_and_saturates() {
        for (f, expected) in [
            (0.0, 0),
            (-0.0, 0),
            (0.999, 0),
            (-0.999, 0),
            (1.0, 1),
            (1.9, 1),
            (-1.9, -1),
            (4_503_599_627_370_496.0, 4_503_599_627_370_496),
            (123_456_789.987, 123_456_789),
            (-123_456_789.987, -123_456_789),
            (9.2e18, 9_200_000_000_000_000_000),
            (9.3e18, i64::MAX),
            (-9.3e18, i64::MIN),
            (f64::INFINITY, i64::MAX),
            (f64::NEG_INFINITY, i64::MIN),
            (f64::NAN, 0),
        ] {
            assert_eq!(f64_to_i64(f), expected, "{f}");
        }
    }

    #[test]
    fn float_to_narrower_integers_clamps() {
        assert_eq!(f64_to_i32(3e10), i32::MAX);
        assert_eq!(f64_to_i32(-3e10), i32::MIN);
        assert_eq!(f64_to_i32(-7.5), -7);
        assert_eq!(f64_to_u32(-1.0), 0);
        assert_eq!(f64_to_u32(3e10), u32::MAX);
        assert_eq!(f64_to_u32(99.9), 99);
        assert_eq!(f64_to_usize(-5.0), 0);
        assert_eq!(f64_to_usize(41.9), 41);
    }

    #[test]
    fn usize_matches_the_correctly_rounded_value() {
        for n in [0, 7, usize::MAX] {
            assert_eq!(usize_to_f64(n), parsed(&n), "{n}");
        }
    }
}
