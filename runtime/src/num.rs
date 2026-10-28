//! Number formatting and parsing (spec §7.12 rule 2).

use crate::string::LoomString;

/// The text of a signed integer (up to 64 bits).
#[unsafe(no_mangle)]
pub extern "C" fn loom_int_to_string(value: i64) -> *mut LoomString {
    LoomString::boxed(value.to_string())
}

/// The text of an unsigned integer (up to 64 bits).
#[unsafe(no_mangle)]
pub extern "C" fn loom_uint_to_string(value: u64) -> *mut LoomString {
    LoomString::boxed(value.to_string())
}

/// The text of a 128-bit integer given as two halves.
#[unsafe(no_mangle)]
pub extern "C" fn loom_int128_to_string(low: u64, high: u64, signed: u8) -> *mut LoomString {
    let bits = (u128::from(high) << 64) | u128::from(low);
    LoomString::boxed(if signed != 0 {
        bits.cast_signed().to_string()
    } else {
        bits.to_string()
    })
}

/// The text of a float: the shortest round-trip digits, positional between 1e-4 and 1e16.
#[must_use]
pub fn float_text(value: f64) -> String {
    if value.is_nan() {
        "NaN".to_owned()
    } else if value.is_infinite() {
        if value > 0.0 {
            "inf".to_owned()
        } else {
            "-inf".to_owned()
        }
    } else {
        format!("{value:?}")
    }
}

/// The text of a float (`is_f32` formats the value as an `F32`).
#[unsafe(no_mangle)]
pub extern "C" fn loom_float_to_string(value: f64, is_f32: u8) -> *mut LoomString {
    if is_f32 != 0 {
        #[allow(clippy::cast_possible_truncation, reason = "the value is an F32")]
        let f = value as f32;
        let text = if f.is_nan() || f.is_infinite() {
            float_text(f64::from(f))
        } else {
            format!("{f:?}")
        };
        return LoomString::boxed(text);
    }
    LoomString::boxed(float_text(value))
}

/// Parses an integer of `bits` bits (`signed`), storing it in `out`; returns 0 on failure.
///
/// # Safety
///
/// `s` must be a live string and `out` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_int_parse(
    s: *const LoomString,
    bits: u32,
    signed: u8,
    out: *mut i64,
) -> u8 {
    // SAFETY: `s` is live.
    let text = unsafe { &*s }.as_str();
    let fits = if signed != 0 {
        text.parse::<i128>()
            .ok()
            .filter(|&v| bits >= 128 || (v >= -(1i128 << (bits - 1)) && v < (1i128 << (bits - 1))))
            .map(|v| v as i64)
    } else {
        text.parse::<u128>()
            .ok()
            .filter(|&v| bits >= 128 || v < (1u128 << bits))
            .map(|v| v as u64 as i64)
    };
    match fits {
        Some(v) => {
            // SAFETY: `out` is writable.
            unsafe { *out = v };
            1
        }
        None => 0,
    }
}

/// Parses a float, storing it in `out`; returns 0 on failure.
///
/// # Safety
///
/// `s` must be a live string and `out` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_float_parse(s: *const LoomString, out: *mut f64) -> u8 {
    // SAFETY: `s` is live.
    match unsafe { &*s }.as_str().trim().parse::<f64>() {
        Ok(v) => {
            // SAFETY: `out` is writable.
            unsafe { *out = v };
            1
        }
        Err(_) => 0,
    }
}

/// `value` rounded to `digits` decimal places, halves away from zero.
#[unsafe(no_mangle)]
pub extern "C" fn loom_float_round_to(value: f64, digits: i64) -> f64 {
    let factor = 10f64.powi(i32::try_from(digits).unwrap_or(0));
    (value * factor).round() / factor
}

/// Appends a debug rendering of an integer.
///
/// # Safety
///
/// `out` must be live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_debug_int(value: i64, out: *mut LoomString) {
    // SAFETY: `out` is live.
    unsafe { &mut *out }.push_str(&value.to_string());
}

/// Appends a debug rendering of a float.
///
/// # Safety
///
/// `out` must be live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_debug_float(value: f64, out: *mut LoomString) {
    // SAFETY: `out` is live.
    unsafe { &mut *out }.push_str(&float_text(value));
}

/// Appends static text (debug renderings of names and punctuation).
///
/// # Safety
///
/// `out` must be live; `ptr` must be valid UTF-8 of length `len`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_debug_text(ptr: *const u8, len: usize, out: *mut LoomString) {
    // SAFETY: forwarded.
    unsafe { &mut *out }.push_str(unsafe { crate::str_from(ptr, len) });
}
