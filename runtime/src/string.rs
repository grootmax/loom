//! `String` and `Bytes` objects and the string intrinsics.

use crate::TypeInfo;
use crate::array::LoomArray;

/// The heap object behind a Loom `String` (UTF-8) or `Bytes`.
#[derive(Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LoomString {
    bytes: Vec<u8>,
}

impl LoomString {
    /// The text (the runtime only ever stores valid UTF-8 in strings).
    #[must_use]
    pub fn as_str(&self) -> &str {
        std::str::from_utf8(&self.bytes).unwrap_or("")
    }

    /// A new string object on the heap.
    #[must_use]
    pub fn boxed(text: String) -> *mut Self {
        crate::track(1);
        Box::into_raw(Box::new(Self {
            bytes: text.into_bytes(),
        }))
    }

    /// Appends text.
    pub fn push_str(&mut self, text: &str) {
        self.bytes.extend_from_slice(text.as_bytes());
    }
}

/// Borrows the string behind a handle.
///
/// # Safety
///
/// `s` must be a live string handle.
unsafe fn get<'a>(s: *const LoomString) -> &'a LoomString {
    // SAFETY: the caller guarantees a live handle.
    unsafe { &*s }
}

/// A string from static UTF-8 bytes.
///
/// # Safety
///
/// `ptr` must point to `len` bytes of valid UTF-8.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_string_from_static(ptr: *const u8, len: usize) -> *mut LoomString {
    // SAFETY: forwarded from the caller.
    let text = unsafe { crate::str_from(ptr, len) };
    LoomString::boxed(text.to_owned())
}

/// A new empty string.
#[unsafe(no_mangle)]
pub extern "C" fn loom_string_new() -> *mut LoomString {
    LoomString::boxed(String::new())
}

/// A copy of a string.
///
/// # Safety
///
/// `s` must be a live string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_string_clone(s: *const LoomString) -> *mut LoomString {
    crate::track(1);
    // SAFETY: the caller passes a live string.
    Box::into_raw(Box::new(unsafe { get(s) }.clone()))
}

/// Frees a string.
///
/// # Safety
///
/// `s` must be a live string handle that is not used afterwards.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_string_drop(s: *mut LoomString) {
    if !s.is_null() {
        crate::track(-1);
        // SAFETY: the caller transfers ownership of a handle from `Box::into_raw`.
        drop(unsafe { Box::from_raw(s) });
    }
}

/// Appends `other` to `s`.
///
/// # Safety
///
/// Both must be live strings; they may be the same object.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_string_push_str(s: *mut LoomString, other: *const LoomString) {
    // SAFETY: both are live; copying `other` first handles aliasing.
    let text = unsafe { get(other) }.as_str().to_owned();
    // SAFETY: `s` is live and not otherwise borrowed during this call.
    unsafe { &mut *s }.push_str(&text);
}

/// Appends a character.
///
/// # Safety
///
/// `s` must be a live string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_string_push_char(s: *mut LoomString, c: u32) {
    let c = char::from_u32(c).unwrap_or('\u{FFFD}');
    let mut buffer = [0; 4];
    // SAFETY: `s` is live.
    unsafe { &mut *s }.push_str(c.encode_utf8(&mut buffer));
}

/// Writes a string to standard output.
///
/// # Safety
///
/// `s` must be a live string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_write_stdout(s: *const LoomString) {
    // SAFETY: `s` is live.
    crate::write_stdout(unsafe { get(s) }.as_str().as_bytes());
}

/// The number of characters.
///
/// # Safety
///
/// `s` must be a live string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_string_length(s: *const LoomString) -> i64 {
    // SAFETY: `s` is live.
    i64::try_from(unsafe { get(s) }.as_str().chars().count()).unwrap_or(i64::MAX)
}

/// The number of bytes.
///
/// # Safety
///
/// `s` must be a live string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_string_byte_length(s: *const LoomString) -> i64 {
    // SAFETY: `s` is live.
    i64::try_from(unsafe { get(s) }.bytes.len()).unwrap_or(i64::MAX)
}

/// Whether two strings are equal.
///
/// # Safety
///
/// Both must be live strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_string_eq(a: *const LoomString, b: *const LoomString) -> u8 {
    // SAFETY: both are live.
    u8::from(unsafe { get(a) } == unsafe { get(b) })
}

/// Compares two strings: -1, 0 or 1.
///
/// # Safety
///
/// Both must be live strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_string_cmp(a: *const LoomString, b: *const LoomString) -> i8 {
    // SAFETY: both are live.
    match unsafe { get(a) }.cmp(unsafe { get(b) }) {
        std::cmp::Ordering::Less => -1,
        std::cmp::Ordering::Equal => 0,
        std::cmp::Ordering::Greater => 1,
    }
}

/// Hashes a string.
///
/// # Safety
///
/// `s` must be a live string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_string_hash(s: *const LoomString) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    // SAFETY: `s` is live.
    unsafe { get(s) }.hash(&mut hasher);
    hasher.finish()
}

/// A new string from a conversion: 0 upper, 1 lower, 2 trim, 3 trim start, 4 trim end.
///
/// # Safety
///
/// `s` must be a live string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_string_convert(
    s: *const LoomString,
    operation: u32,
) -> *mut LoomString {
    // SAFETY: `s` is live.
    let text = unsafe { get(s) }.as_str();
    LoomString::boxed(match operation {
        0 => text.to_uppercase(),
        1 => text.to_lowercase(),
        2 => text.trim().to_owned(),
        3 => text.trim_start().to_owned(),
        4 => text.trim_end().to_owned(),
        _ => text.to_owned(),
    })
}

/// The characters, as an `Array[Char]` with element descriptor `char_info`.
///
/// # Safety
///
/// `s` must be a live string; `char_info` must describe a 4-byte `Char`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_string_chars(
    s: *const LoomString,
    char_info: *const TypeInfo,
) -> *mut LoomArray {
    let array = crate::array::loom_array_new(char_info);
    // SAFETY: `s` is live.
    for c in unsafe { get(s) }.as_str().chars() {
        let value = u32::from(c);
        // SAFETY: the array was just created for 4-byte elements.
        unsafe { crate::array::loom_array_push(array, std::ptr::from_ref(&value).cast()) };
    }
    array
}

/// A string from an `Array[Char]`.
///
/// # Safety
///
/// `chars` must be a live array of 4-byte `Char` elements.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_string_from_chars(chars: *const LoomArray) -> *mut LoomString {
    // SAFETY: `chars` is live.
    let array = unsafe { &*chars };
    let text: String = (0..array.len)
        .filter_map(|i| {
            // SAFETY: `i` is in bounds and elements are `u32`.
            let code = unsafe { array.element(i).cast::<u32>().read_unaligned() };
            char::from_u32(code)
        })
        .collect();
    LoomString::boxed(text)
}

fn strings_array(
    parts: impl Iterator<Item = String>,
    string_info: *const TypeInfo,
) -> *mut LoomArray {
    let array = crate::array::loom_array_new(string_info);
    for part in parts {
        let handle = LoomString::boxed(part);
        // SAFETY: the array holds string handles (pointer-sized); we move the handle in.
        unsafe { crate::array::loom_array_push(array, std::ptr::from_ref(&handle).cast()) };
    }
    array
}

/// The words of a string.
///
/// # Safety
///
/// `s` must be live; `string_info` must describe `String` elements.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_string_split_whitespace(
    s: *const LoomString,
    string_info: *const TypeInfo,
) -> *mut LoomArray {
    // SAFETY: `s` is live.
    let text = unsafe { get(s) }.as_str();
    strings_array(text.split_whitespace().map(str::to_owned), string_info)
}

/// The parts between occurrences of `separator` (each character if it is empty).
///
/// # Safety
///
/// Both strings must be live; `string_info` must describe `String` elements.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_string_split(
    s: *const LoomString,
    separator: *const LoomString,
    string_info: *const TypeInfo,
) -> *mut LoomArray {
    // SAFETY: both are live.
    let (text, sep) = (
        unsafe { get(s) }.as_str(),
        unsafe { get(separator) }.as_str(),
    );
    if sep.is_empty() {
        strings_array(text.chars().map(|c| c.to_string()), string_info)
    } else {
        strings_array(text.split(sep).map(str::to_owned), string_info)
    }
}

/// The character index of `pattern` in `s`, or -1.
///
/// # Safety
///
/// Both must be live strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_string_find(s: *const LoomString, pattern: *const LoomString) -> i64 {
    // SAFETY: both are live.
    let (text, pattern) = (unsafe { get(s) }.as_str(), unsafe { get(pattern) }.as_str());
    text.find(pattern).map_or(-1, |byte| {
        i64::try_from(text[..byte].chars().count()).unwrap_or(-1)
    })
}

/// The characters from `start` up to `stop`; panics if out of range.
///
/// # Safety
///
/// `s` must be live; the file pointer must be valid UTF-8.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_string_substring(
    s: *const LoomString,
    start: i64,
    stop: i64,
    file: *const u8,
    file_len: usize,
    line: u32,
    column: u32,
) -> *mut LoomString {
    // SAFETY: `s` is live.
    let chars: Vec<char> = unsafe { get(s) }.as_str().chars().collect();
    let length = i64::try_from(chars.len()).unwrap_or(i64::MAX);
    if start < 0 || stop < start || stop > length {
        // SAFETY: forwarded.
        let file = unsafe { crate::str_from(file, file_len) };
        crate::panic_at(
            &format!("substring {start}..{stop} is out of bounds for a string of length {length}"),
            file,
            line,
            column,
        );
    }
    let (start, stop) = (
        usize::try_from(start).unwrap_or(0),
        usize::try_from(stop).unwrap_or(0),
    );
    LoomString::boxed(chars[start..stop].iter().collect())
}

/// `s` repeated `count` times.
///
/// # Safety
///
/// `s` must be live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_string_repeat(s: *const LoomString, count: i64) -> *mut LoomString {
    // SAFETY: `s` is live.
    LoomString::boxed(
        unsafe { get(s) }
            .as_str()
            .repeat(usize::try_from(count).unwrap_or(0)),
    )
}

/// A character's text.
#[unsafe(no_mangle)]
pub extern "C" fn loom_char_to_string(c: u32) -> *mut LoomString {
    LoomString::boxed(char::from_u32(c).map(String::from).unwrap_or_default())
}

/// Character classes: 0 alphabetic, 1 alphanumeric, 2 digit, 3 whitespace, 4 upper, 5 lower.
#[unsafe(no_mangle)]
pub extern "C" fn loom_char_is(c: u32, class: u32) -> u8 {
    let Some(c) = char::from_u32(c) else { return 0 };
    u8::from(match class {
        0 => c.is_alphabetic(),
        1 => c.is_alphanumeric(),
        2 => c.is_ascii_digit(),
        3 => c.is_whitespace(),
        4 => c.is_uppercase(),
        5 => c.is_lowercase(),
        _ => false,
    })
}

/// Simple case mapping: 0 upper, 1 lower.
#[unsafe(no_mangle)]
pub extern "C" fn loom_char_convert(c: u32, operation: u32) -> u32 {
    let Some(ch) = char::from_u32(c) else {
        return c;
    };
    let mut mapped = if operation == 0 {
        ch.to_uppercase().collect::<Vec<_>>()
    } else {
        ch.to_lowercase().collect::<Vec<_>>()
    };
    if mapped.len() == 1 {
        u32::from(mapped.remove(0))
    } else {
        c
    }
}

/// Appends a debug rendering of a string (quoted).
///
/// # Safety
///
/// Both must be live strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_string_debug(s: *const LoomString, out: *mut LoomString) {
    // SAFETY: both are live; the text is copied before `out` is mutated.
    let text = format!("{:?}", unsafe { get(s) }.as_str());
    // SAFETY: `out` is live.
    unsafe { &mut *out }.push_str(&text);
}

/// Appends a debug rendering of a character (quoted, escaped).
///
/// # Safety
///
/// `out` must be a live string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_char_debug(c: u32, out: *mut LoomString) {
    let text = format!("{:?}", char::from_u32(c).unwrap_or('\u{FFFD}'));
    // SAFETY: `out` is live.
    unsafe { &mut *out }.push_str(&text);
}
