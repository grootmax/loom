//! Integer helpers for the core intrinsics whose overflow behavior is chosen by the
//! caller (`checked_*`, `wrapping_*`, `saturating_*`, `pow`), and operation names.

use crate::string::LoomString;

/// The numeric code of an operation or class name passed to an intrinsic as a string
/// (`"add"`, `"upper"`, `"digit"`, …), as the code generator also computes for constants.
#[must_use]
pub fn name_code(name: &str) -> u32 {
    match name {
        "sub" | "lower" | "alphanumeric" | "abs" => 1,
        "mul" | "trim" | "digit" | "floor" => 2,
        "div" | "trim_start" | "whitespace" | "ceil" => 3,
        "trim_end" | "uppercase" => 4,
        "lowercase" => 5,
        // "add", "upper", "alphabetic", "sqrt" and unknown names.
        _ => 0,
    }
}

/// [`name_code`] of a Loom string.
///
/// # Safety
///
/// `name` must be a live string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_name_code(name: *const LoomString) -> u32 {
    // SAFETY: the caller passes a live string.
    name_code(unsafe { &*name }.as_str())
}

fn fits(value: i128, bits: u32, signed: bool) -> bool {
    if bits >= 128 {
        return signed || value >= 0;
    }
    if signed {
        let half = 1i128 << (bits - 1);
        (-half..half).contains(&value)
    } else {
        (0..(1i128 << bits)).contains(&value)
    }
}

fn wrap(value: i128, bits: u32, signed: bool) -> i128 {
    if bits >= 128 {
        return value;
    }
    let mask = (1i128 << bits) - 1;
    let low = value & mask;
    if signed && low >= 1i128 << (bits - 1) {
        low - (1i128 << bits)
    } else {
        low
    }
}

fn bounds(bits: u32, signed: bool) -> (i128, i128) {
    match (bits >= 128, signed) {
        (true, true) => (i128::MIN, i128::MAX),
        (true, false) => (0, i128::MAX),
        (false, true) => (-(1i128 << (bits - 1)), (1i128 << (bits - 1)) - 1),
        (false, false) => (0, (1i128 << bits) - 1),
    }
}

/// `left op right` for an integer type of `bits` bits, where `mode` is 0 (checked: returns 0
/// when the result does not fit or the divisor is zero), 1 (wrapping) or 2 (saturating),
/// and `op` is a [`name_code`] of `add`, `sub`, `mul` or `div`. Values are passed as
/// sign- or zero-extended 128-bit integers.
///
/// # Safety
///
/// The pointers must refer to readable (and, for `out`, writable) 16-byte integers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_int_arith(
    mode: u32,
    op: u32,
    left: *const i128,
    right: *const i128,
    bits: u32,
    signed: u8,
    out: *mut i128,
) -> u8 {
    // SAFETY: the caller passes readable integers.
    let (a, b) = unsafe { (left.read_unaligned(), right.read_unaligned()) };
    let signed = signed != 0;
    let raw = match op {
        0 => a.checked_add(b),
        1 => a.checked_sub(b),
        2 => a.checked_mul(b),
        _ => a.checked_div(b),
    };
    let result = match mode {
        0 => match raw.filter(|&v| fits(v, bits, signed)) {
            Some(v) => v,
            None => return 0,
        },
        1 => wrap(
            match op {
                0 => a.wrapping_add(b),
                1 => a.wrapping_sub(b),
                2 => a.wrapping_mul(b),
                _ => a.checked_div(b).unwrap_or(0),
            },
            bits,
            signed,
        ),
        _ => {
            if let Some(v) = raw.filter(|&v| fits(v, bits, signed)) {
                v
            } else {
                let negative = match op {
                    0 => a < 0 && b < 0,
                    1 => b > 0,
                    _ => (a < 0) != (b < 0),
                };
                let (min, max) = bounds(bits, signed);
                if negative { min } else { max }
            }
        }
    };
    // SAFETY: the caller passes a writable integer.
    unsafe { out.write_unaligned(result) };
    1
}

/// `base` raised to `exponent` for an integer type of `bits` bits; returns 0 on overflow.
///
/// # Safety
///
/// `base` must be readable and `out` writable (16-byte integers).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_int_pow(
    base: *const i128,
    exponent: u32,
    bits: u32,
    signed: u8,
    out: *mut i128,
) -> u8 {
    // SAFETY: the caller passes a readable integer.
    let base = unsafe { base.read_unaligned() };
    match base
        .checked_pow(exponent)
        .filter(|&v| fits(v, bits, signed != 0))
    {
        Some(v) => {
            // SAFETY: the caller passes a writable integer.
            unsafe { out.write_unaligned(v) };
            1
        }
        None => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::{loom_int_arith, name_code};

    fn arith(mode: u32, op: u32, a: i128, b: i128, bits: u32, signed: bool) -> Option<i128> {
        let mut out = 0i128;
        // SAFETY: all pointers refer to locals.
        let ok = unsafe {
            loom_int_arith(
                mode,
                op,
                &raw const a,
                &raw const b,
                bits,
                u8::from(signed),
                &raw mut out,
            )
        };
        (ok != 0).then_some(out)
    }

    #[test]
    fn modes() {
        assert_eq!(arith(0, 0, 127, 1, 8, true), None);
        assert_eq!(arith(1, 0, 127, 1, 8, true), Some(-128));
        assert_eq!(arith(2, 0, 127, 1, 8, true), Some(127));
        assert_eq!(arith(2, 1, 0, 1, 8, false), Some(0));
        assert_eq!(arith(0, 3, 1, 0, 64, true), None);
        assert_eq!(arith(2, 2, -100, 100, 8, true), Some(-128));
        assert_eq!(name_code("trim_end"), 4);
    }
}
