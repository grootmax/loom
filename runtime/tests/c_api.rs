//! Integration tests exercising all exported C-ABI functions of `loom_runtime`.

#![allow(
    unsafe_code,
    reason = "tests exercise the C ABI functions which take raw pointers and are unsafe"
)]

use loom_runtime::TypeInfo;
use loom_runtime::array::{
    loom_array_clone, loom_array_cmp, loom_array_debug, loom_array_drop, loom_array_eq,
    loom_array_get, loom_array_hash, loom_array_insert, loom_array_len, loom_array_new,
    loom_array_pop, loom_array_push, loom_array_remove, loom_array_reverse, loom_array_slice,
    loom_array_sort_by,
};
use loom_runtime::map::{
    loom_map_clone, loom_map_debug, loom_map_drop, loom_map_entry, loom_map_eq, loom_map_get,
    loom_map_index, loom_map_insert, loom_map_keys_len, loom_map_len, loom_map_new,
    loom_map_remove, loom_map_update,
};
use loom_runtime::num::{
    loom_debug_float, loom_debug_int, loom_debug_text, loom_float_parse, loom_float_round_to,
    loom_float_to_string, loom_int_parse, loom_int_to_string, loom_int128_to_string,
    loom_uint_to_string,
};
use loom_runtime::string::{
    LoomString, loom_char_convert, loom_char_is, loom_char_to_string, loom_string_byte_length,
    loom_string_chars, loom_string_clone, loom_string_cmp, loom_string_convert, loom_string_debug,
    loom_string_drop, loom_string_eq, loom_string_find, loom_string_from_chars,
    loom_string_from_static, loom_string_hash, loom_string_length, loom_string_new,
    loom_string_push_char, loom_string_push_str, loom_string_repeat, loom_string_split,
    loom_string_split_whitespace, loom_string_substring, loom_write_stdout,
};
use loom_runtime::{loom_alloc, loom_flush, loom_free};

// --- Test-side TypeInfo Descriptors ---

static I64_INFO: TypeInfo = TypeInfo {
    size: std::mem::size_of::<i64>(),
    align: std::mem::align_of::<i64>(),
    drop: None,
    clone: None,
    eq: Some(i64_eq),
    cmp: Some(i64_cmp),
    hash: Some(i64_hash),
    debug: Some(i64_debug),
};

static I64_NO_DEBUG_INFO: TypeInfo = TypeInfo {
    size: std::mem::size_of::<i64>(),
    align: std::mem::align_of::<i64>(),
    drop: None,
    clone: None,
    eq: Some(i64_eq),
    cmp: Some(i64_cmp),
    hash: Some(i64_hash),
    debug: None,
};

unsafe extern "C" fn i64_eq(a: *const u8, b: *const u8) -> u8 {
    // SAFETY: pointers hold valid i64 values.
    let va = unsafe { a.cast::<i64>().read_unaligned() };
    // SAFETY: pointers hold valid i64 values.
    let vb = unsafe { b.cast::<i64>().read_unaligned() };
    u8::from(va == vb)
}

unsafe extern "C" fn i64_cmp(a: *const u8, b: *const u8) -> i8 {
    // SAFETY: pointers hold valid i64 values.
    let va = unsafe { a.cast::<i64>().read_unaligned() };
    // SAFETY: pointers hold valid i64 values.
    let vb = unsafe { b.cast::<i64>().read_unaligned() };
    match va.cmp(&vb) {
        std::cmp::Ordering::Less => -1,
        std::cmp::Ordering::Equal => 0,
        std::cmp::Ordering::Greater => 1,
    }
}

unsafe extern "C" fn i64_hash(ptr: *const u8) -> u64 {
    use std::hash::{Hash, Hasher};
    // SAFETY: pointer holds valid i64 value.
    let val = unsafe { ptr.cast::<i64>().read_unaligned() };
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    val.hash(&mut hasher);
    hasher.finish()
}

unsafe extern "C" fn i64_debug(ptr: *const u8, out: *mut LoomString) {
    // SAFETY: pointer holds valid i64 value.
    let val = unsafe { ptr.cast::<i64>().read_unaligned() };
    // SAFETY: out is a live LoomString pointer.
    unsafe { loom_debug_int(val, out) };
}

static CHAR_INFO: TypeInfo = TypeInfo {
    size: std::mem::size_of::<u32>(),
    align: std::mem::align_of::<u32>(),
    drop: None,
    clone: None,
    eq: Some(u32_eq),
    cmp: Some(u32_cmp),
    hash: Some(u32_hash),
    debug: Some(u32_debug),
};

unsafe extern "C" fn u32_eq(a: *const u8, b: *const u8) -> u8 {
    // SAFETY: pointers hold valid u32 values.
    let va = unsafe { a.cast::<u32>().read_unaligned() };
    // SAFETY: pointers hold valid u32 values.
    let vb = unsafe { b.cast::<u32>().read_unaligned() };
    u8::from(va == vb)
}

unsafe extern "C" fn u32_cmp(a: *const u8, b: *const u8) -> i8 {
    // SAFETY: pointers hold valid u32 values.
    let va = unsafe { a.cast::<u32>().read_unaligned() };
    // SAFETY: pointers hold valid u32 values.
    let vb = unsafe { b.cast::<u32>().read_unaligned() };
    match va.cmp(&vb) {
        std::cmp::Ordering::Less => -1,
        std::cmp::Ordering::Equal => 0,
        std::cmp::Ordering::Greater => 1,
    }
}

unsafe extern "C" fn u32_hash(ptr: *const u8) -> u64 {
    use std::hash::{Hash, Hasher};
    // SAFETY: pointer holds valid u32 value.
    let val = unsafe { ptr.cast::<u32>().read_unaligned() };
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    val.hash(&mut hasher);
    hasher.finish()
}

unsafe extern "C" fn u32_debug(ptr: *const u8, out: *mut LoomString) {
    // SAFETY: pointer holds valid u32 value.
    let val = unsafe { ptr.cast::<u32>().read_unaligned() };
    // SAFETY: out is a live LoomString pointer.
    unsafe { loom_debug_int(i64::from(val), out) };
}

static STRING_HANDLE_INFO: TypeInfo = TypeInfo {
    size: std::mem::size_of::<*mut LoomString>(),
    align: std::mem::align_of::<*mut LoomString>(),
    drop: Some(string_handle_drop),
    clone: Some(string_handle_clone),
    eq: Some(string_handle_eq),
    cmp: Some(string_handle_cmp),
    hash: Some(string_handle_hash),
    debug: Some(string_handle_debug),
};

unsafe extern "C" fn string_handle_drop(ptr: *mut u8) {
    // SAFETY: ptr points to a valid *mut LoomString handle.
    let handle = unsafe { ptr.cast::<*mut LoomString>().read_unaligned() };
    // SAFETY: handle is a live LoomString pointer.
    unsafe { loom_string_drop(handle) };
}

unsafe extern "C" fn string_handle_clone(src: *const u8, dst: *mut u8) {
    // SAFETY: src points to a valid *mut LoomString handle.
    let handle = unsafe { src.cast::<*mut LoomString>().read_unaligned() };
    // SAFETY: handle is a live LoomString.
    let cloned = unsafe { loom_string_clone(handle) };
    // SAFETY: dst is a valid uninitialized slot for *mut LoomString.
    unsafe { dst.cast::<*mut LoomString>().write_unaligned(cloned) };
}

unsafe extern "C" fn string_handle_eq(a: *const u8, b: *const u8) -> u8 {
    // SAFETY: a points to a valid *mut LoomString.
    let ha = unsafe { a.cast::<*mut LoomString>().read_unaligned() };
    // SAFETY: b points to a valid *mut LoomString.
    let hb = unsafe { b.cast::<*mut LoomString>().read_unaligned() };
    // SAFETY: handles are live strings.
    unsafe { loom_string_eq(ha, hb) }
}

unsafe extern "C" fn string_handle_cmp(a: *const u8, b: *const u8) -> i8 {
    // SAFETY: a points to a valid *mut LoomString.
    let ha = unsafe { a.cast::<*mut LoomString>().read_unaligned() };
    // SAFETY: b points to a valid *mut LoomString.
    let hb = unsafe { b.cast::<*mut LoomString>().read_unaligned() };
    // SAFETY: handles are live strings.
    unsafe { loom_string_cmp(ha, hb) }
}

unsafe extern "C" fn string_handle_hash(ptr: *const u8) -> u64 {
    // SAFETY: ptr points to a valid *mut LoomString.
    let handle = unsafe { ptr.cast::<*mut LoomString>().read_unaligned() };
    // SAFETY: handle is a live string.
    unsafe { loom_string_hash(handle) }
}

unsafe extern "C" fn string_handle_debug(ptr: *const u8, out: *mut LoomString) {
    // SAFETY: ptr points to a valid *mut LoomString.
    let handle = unsafe { ptr.cast::<*mut LoomString>().read_unaligned() };
    // SAFETY: handle and out are live strings.
    unsafe { loom_string_debug(handle, out) };
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Point {
    x: i32,
    y: i32,
}

static POINT_INFO: TypeInfo = TypeInfo {
    size: std::mem::size_of::<Point>(),
    align: std::mem::align_of::<Point>(),
    drop: None,
    clone: Some(point_clone),
    eq: Some(point_eq),
    cmp: Some(point_cmp),
    hash: Some(point_hash),
    debug: Some(point_debug),
};

unsafe extern "C" fn point_clone(src: *const u8, dst: *mut u8) {
    // SAFETY: src points to a Point.
    let val = unsafe { src.cast::<Point>().read_unaligned() };
    // SAFETY: dst has room for a Point.
    unsafe { dst.cast::<Point>().write_unaligned(val) };
}

unsafe extern "C" fn point_eq(a: *const u8, b: *const u8) -> u8 {
    // SAFETY: pointers hold Point values.
    let pa = unsafe { a.cast::<Point>().read_unaligned() };
    // SAFETY: pointers hold Point values.
    let pb = unsafe { b.cast::<Point>().read_unaligned() };
    u8::from(pa == pb)
}

unsafe extern "C" fn point_cmp(a: *const u8, b: *const u8) -> i8 {
    // SAFETY: pointers hold Point values.
    let pa = unsafe { a.cast::<Point>().read_unaligned() };
    // SAFETY: pointers hold Point values.
    let pb = unsafe { b.cast::<Point>().read_unaligned() };
    match (pa.x, pa.y).cmp(&(pb.x, pb.y)) {
        std::cmp::Ordering::Less => -1,
        std::cmp::Ordering::Equal => 0,
        std::cmp::Ordering::Greater => 1,
    }
}

unsafe extern "C" fn point_hash(ptr: *const u8) -> u64 {
    use std::hash::{Hash, Hasher};
    // SAFETY: pointer holds Point value.
    let pt = unsafe { ptr.cast::<Point>().read_unaligned() };
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    (pt.x, pt.y).hash(&mut hasher);
    hasher.finish()
}

unsafe extern "C" fn point_debug(ptr: *const u8, out: *mut LoomString) {
    // SAFETY: pointer holds Point value.
    let pt = unsafe { ptr.cast::<Point>().read_unaligned() };
    let text = format!("Point({}, {})", pt.x, pt.y);
    // SAFETY: text is valid UTF-8.
    let s = unsafe { loom_string_from_static(text.as_ptr(), text.len()) };
    // SAFETY: s and out are live strings.
    unsafe { loom_string_push_str(out, s) };
    // SAFETY: s is a live string to drop.
    unsafe { loom_string_drop(s) };
}

// --- Assert Helper ---

fn assert_string_eq(s: *const LoomString, expected: &str) {
    // SAFETY: expected text as static string.
    let expected_s = unsafe { loom_string_from_static(expected.as_ptr(), expected.len()) };
    // SAFETY: s and expected_s are live strings.
    let eq = unsafe { loom_string_eq(s, expected_s) };
    // SAFETY: drop expected_s.
    unsafe { loom_string_drop(expected_s) };
    assert_eq!(eq, 1, "String comparison failed for expected: {expected}");
}

// --- String Tests ---

#[test]
fn test_string_construction_push_conversions() {
    // SAFETY: valid static text.
    let s1 = unsafe { loom_string_from_static(b"hello".as_ptr(), 5) };
    assert_string_eq(s1, "hello");

    let s2 = loom_string_new();
    assert_string_eq(s2, "");

    // SAFETY: s1 and s2 are live.
    unsafe {
        assert_eq!(loom_string_length(s1), 5);
        assert_eq!(loom_string_byte_length(s1), 5);
        assert_eq!(loom_string_length(s2), 0);
        assert_eq!(loom_string_byte_length(s2), 0);
    }

    // Push str and push char
    // SAFETY: s2 is live.
    unsafe {
        loom_string_push_str(s2, s1);
    }
    // SAFETY: create space string.
    let space = unsafe { loom_string_from_static(b" world".as_ptr(), 6) };
    // SAFETY: s2 and space are live.
    unsafe {
        loom_string_push_str(s2, space);
        loom_string_drop(space);
        loom_string_push_char(s2, u32::from('!'));
        loom_string_push_char(s2, u32::from('😊'));
    }
    assert_string_eq(s2, "hello world!😊");

    // SAFETY: s2 is live.
    unsafe {
        assert_eq!(loom_string_length(s2), 13);
        assert_eq!(loom_string_byte_length(s2), 16);
    }

    // Aliased push_str (s2 pushed to s2)
    // SAFETY: s2 is live.
    unsafe {
        loom_string_push_str(s2, s2);
    }
    assert_string_eq(s2, "hello world!😊hello world!😊");

    // Conversions
    // SAFETY: s_padded is created with valid text.
    let s_padded = unsafe { loom_string_from_static(b"  Hello World  ".as_ptr(), 15) };
    // SAFETY: operation 0: upper
    let s_upper = unsafe { loom_string_convert(s_padded, 0) };
    assert_string_eq(s_upper, "  HELLO WORLD  ");

    // SAFETY: operation 1: lower
    let s_lower = unsafe { loom_string_convert(s_padded, 1) };
    assert_string_eq(s_lower, "  hello world  ");

    // SAFETY: operation 2: trim
    let s_trim = unsafe { loom_string_convert(s_padded, 2) };
    assert_string_eq(s_trim, "Hello World");

    // SAFETY: operation 3: trim start
    let s_trim_start = unsafe { loom_string_convert(s_padded, 3) };
    assert_string_eq(s_trim_start, "Hello World  ");

    // SAFETY: operation 4: trim end
    let s_trim_end = unsafe { loom_string_convert(s_padded, 4) };
    assert_string_eq(s_trim_end, "  Hello World");

    // SAFETY: operation 99: fallback
    let s_fallback = unsafe { loom_string_convert(s_padded, 99) };
    assert_string_eq(s_fallback, "  Hello World  ");

    // SAFETY: drop strings.
    unsafe {
        loom_string_drop(s1);
        loom_string_drop(s2);
        loom_string_drop(s_padded);
        loom_string_drop(s_upper);
        loom_string_drop(s_lower);
        loom_string_drop(s_trim);
        loom_string_drop(s_trim_start);
        loom_string_drop(s_trim_end);
        loom_string_drop(s_fallback);
    }
}

#[test]
fn test_string_split_find_substring_repeat_chars() {
    // Split whitespace
    // SAFETY: create text.
    let text = unsafe { loom_string_from_static(b"  foo  bar  baz  ".as_ptr(), 17) };
    // SAFETY: text and STRING_HANDLE_INFO are valid.
    let arr_ws = unsafe { loom_string_split_whitespace(text, &raw const STRING_HANDLE_INFO) };
    // SAFETY: arr_ws is live.
    unsafe {
        assert_eq!(loom_array_len(arr_ws), 3);
        let h0 = loom_array_get(arr_ws, 0, b"f.lm".as_ptr(), 4, 1, 1)
            .cast::<*mut LoomString>()
            .read_unaligned();
        assert_string_eq(h0, "foo");
        let h1 = loom_array_get(arr_ws, 1, b"f.lm".as_ptr(), 4, 1, 1)
            .cast::<*mut LoomString>()
            .read_unaligned();
        assert_string_eq(h1, "bar");
        let h2 = loom_array_get(arr_ws, 2, b"f.lm".as_ptr(), 4, 1, 1)
            .cast::<*mut LoomString>()
            .read_unaligned();
        assert_string_eq(h2, "baz");
        loom_array_drop(arr_ws);
    }

    // Split separator
    // SAFETY: create text and separator.
    let csv = unsafe { loom_string_from_static(b"a,b,c".as_ptr(), 5) };
    // SAFETY: create comma string.
    let comma = unsafe { loom_string_from_static(b",".as_ptr(), 1) };
    // SAFETY: arguments are live.
    let arr_split = unsafe { loom_string_split(csv, comma, &raw const STRING_HANDLE_INFO) };
    // SAFETY: arr_split is live.
    unsafe {
        assert_eq!(loom_array_len(arr_split), 3);
        loom_array_drop(arr_split);
    }

    // Split empty separator
    let empty_sep = loom_string_new();
    // SAFETY: arguments are live.
    let arr_chars_split =
        unsafe { loom_string_split(csv, empty_sep, &raw const STRING_HANDLE_INFO) };
    // SAFETY: arr_chars_split is live.
    unsafe {
        assert_eq!(loom_array_len(arr_chars_split), 5);
        loom_array_drop(arr_chars_split);
        loom_string_drop(empty_sep);
        loom_string_drop(comma);
        loom_string_drop(csv);
    }

    // Find and Substring and Repeat
    // SAFETY: create target and pattern.
    let haystack = unsafe { loom_string_from_static(b"hello world".as_ptr(), 11) };
    // SAFETY: create pattern strings.
    let needle = unsafe { loom_string_from_static(b"world".as_ptr(), 5) };
    // SAFETY: create pattern strings.
    let missing = unsafe { loom_string_from_static(b"xyz".as_ptr(), 3) };

    // SAFETY: arguments are live.
    unsafe {
        assert_eq!(loom_string_find(haystack, needle), 6);
        assert_eq!(loom_string_find(haystack, missing), -1);
    }

    // Substring
    // SAFETY: arguments are live.
    let sub = unsafe { loom_string_substring(haystack, 0, 5, b"f.lm".as_ptr(), 4, 1, 1) };
    assert_string_eq(sub, "hello");

    // Repeat
    // SAFETY: arguments are live.
    let rep = unsafe { loom_string_repeat(sub, 3) };
    assert_string_eq(rep, "hellohellohello");

    // Chars round trip
    // SAFETY: arguments are live.
    let char_arr = unsafe { loom_string_chars(haystack, &raw const CHAR_INFO) };
    // SAFETY: arguments are live.
    unsafe {
        assert_eq!(loom_array_len(char_arr), 11);
    }
    // SAFETY: arguments are live.
    let rebuilt = unsafe { loom_string_from_chars(char_arr) };
    assert_string_eq(rebuilt, "hello world");

    // SAFETY: drop objects.
    unsafe {
        loom_string_drop(text);
        loom_string_drop(haystack);
        loom_string_drop(needle);
        loom_string_drop(missing);
        loom_string_drop(sub);
        loom_string_drop(rep);
        loom_array_drop(char_arr);
        loom_string_drop(rebuilt);
    }
}

#[test]
fn test_string_eq_cmp_hash_debug_stdout_char() {
    // SAFETY: create strings.
    let a = unsafe { loom_string_from_static(b"abc".as_ptr(), 3) };
    // SAFETY: create strings.
    let b = unsafe { loom_string_from_static(b"abc".as_ptr(), 3) };
    // SAFETY: create strings.
    let c = unsafe { loom_string_from_static(b"def".as_ptr(), 3) };

    // SAFETY: arguments are live.
    unsafe {
        assert_eq!(loom_string_eq(a, b), 1);
        assert_eq!(loom_string_eq(a, c), 0);
        assert_eq!(loom_string_cmp(a, b), 0);
        assert_eq!(loom_string_cmp(a, c), -1);
        assert_eq!(loom_string_cmp(c, a), 1);
        assert_eq!(loom_string_hash(a), loom_string_hash(b));
    }

    // Debug
    let dbg_out = loom_string_new();
    // SAFETY: arguments are live.
    unsafe { loom_string_debug(a, dbg_out) };
    assert_string_eq(dbg_out, "\"abc\"");

    // Write stdout & flush
    // SAFETY: arguments are live.
    unsafe {
        loom_write_stdout(a);
        loom_flush();
    }

    // Char helpers
    let char_s = loom_char_to_string(u32::from('X'));
    assert_string_eq(char_s, "X");

    assert_eq!(loom_char_is(u32::from('A'), 0), 1);
    assert_eq!(loom_char_is(u32::from('1'), 1), 1);
    assert_eq!(loom_char_is(u32::from('1'), 2), 1);
    assert_eq!(loom_char_is(u32::from(' '), 3), 1);
    assert_eq!(loom_char_is(u32::from('A'), 4), 1);
    assert_eq!(loom_char_is(u32::from('a'), 5), 1);
    assert_eq!(loom_char_is(u32::from('a'), 99), 0);
    assert_eq!(loom_char_is(0x11_0000, 0), 0);

    assert_eq!(loom_char_convert(u32::from('a'), 0), u32::from('A'));
    assert_eq!(loom_char_convert(u32::from('B'), 1), u32::from('b'));

    // SAFETY: drop objects.
    unsafe {
        loom_string_drop(a);
        loom_string_drop(b);
        loom_string_drop(c);
        loom_string_drop(dbg_out);
        loom_string_drop(char_s);
    }
}

// --- Array Tests ---

#[test]
fn test_array_push_pop_get_insert_remove_slice_reverse() {
    let arr = loom_array_new(&raw const I64_INFO);
    // SAFETY: arr is live.
    unsafe {
        assert_eq!(loom_array_len(arr), 0);
    }

    let val10: i64 = 10;
    let val20: i64 = 20;
    let val30: i64 = 30;

    // Push elements
    // SAFETY: arr is live and val pointers hold i64 values.
    unsafe {
        loom_array_push(arr, std::ptr::from_ref(&val10).cast());
        loom_array_push(arr, std::ptr::from_ref(&val30).cast());
        assert_eq!(loom_array_len(arr), 2);
    }

    // Insert at index 1
    // SAFETY: arr is live.
    unsafe {
        loom_array_insert(
            arr,
            1,
            std::ptr::from_ref(&val20).cast(),
            b"f.lm".as_ptr(),
            4,
            1,
            1,
        );
        assert_eq!(loom_array_len(arr), 3);
    }

    // Get
    // SAFETY: arr is live.
    unsafe {
        let p0 = loom_array_get(arr, 0, b"f.lm".as_ptr(), 4, 1, 1)
            .cast::<i64>()
            .read_unaligned();
        let p1 = loom_array_get(arr, 1, b"f.lm".as_ptr(), 4, 1, 1)
            .cast::<i64>()
            .read_unaligned();
        let p2 = loom_array_get(arr, 2, b"f.lm".as_ptr(), 4, 1, 1)
            .cast::<i64>()
            .read_unaligned();
        assert_eq!(p0, 10);
        assert_eq!(p1, 20);
        assert_eq!(p2, 30);
    }

    // Slice
    // SAFETY: arr is live.
    let slice = unsafe { loom_array_slice(arr, 1, 3) };
    // SAFETY: slice is live.
    unsafe {
        assert_eq!(loom_array_len(slice), 2);
        let s0 = loom_array_get(slice, 0, b"f.lm".as_ptr(), 4, 1, 1)
            .cast::<i64>()
            .read_unaligned();
        let s1 = loom_array_get(slice, 1, b"f.lm".as_ptr(), 4, 1, 1)
            .cast::<i64>()
            .read_unaligned();
        assert_eq!(s0, 20);
        assert_eq!(s1, 30);
    }

    // Reverse
    // SAFETY: slice is live.
    unsafe {
        loom_array_reverse(slice);
        let r0 = loom_array_get(slice, 0, b"f.lm".as_ptr(), 4, 1, 1)
            .cast::<i64>()
            .read_unaligned();
        let r1 = loom_array_get(slice, 1, b"f.lm".as_ptr(), 4, 1, 1)
            .cast::<i64>()
            .read_unaligned();
        assert_eq!(r0, 30);
        assert_eq!(r1, 20);
    }

    // Remove
    let mut removed_val: i64 = 0;
    // SAFETY: arr is live and removed_val pointer has space for i64.
    unsafe {
        loom_array_remove(
            arr,
            0,
            std::ptr::from_mut(&mut removed_val).cast(),
            b"f.lm".as_ptr(),
            4,
            1,
            1,
        );
        assert_eq!(removed_val, 10);
        assert_eq!(loom_array_len(arr), 2);
    }

    // Pop
    let mut popped_val: i64 = 0;
    // SAFETY: arr is live.
    unsafe {
        let res = loom_array_pop(arr, std::ptr::from_mut(&mut popped_val).cast());
        assert_eq!(res, 1);
        assert_eq!(popped_val, 30);
        assert_eq!(loom_array_len(arr), 1);
    }

    // Pop empty
    let mut empty_popped: i64 = 0;
    // SAFETY: arr is live.
    unsafe {
        loom_array_pop(arr, std::ptr::from_mut(&mut empty_popped).cast());
        let res = loom_array_pop(arr, std::ptr::from_mut(&mut empty_popped).cast());
        assert_eq!(res, 0);
    }

    // SAFETY: drop arrays.
    unsafe {
        loom_array_drop(arr);
        loom_array_drop(slice);
        loom_array_drop(std::ptr::null_mut());
    }
}

unsafe extern "C" fn i64_sort_cmp(tag: *mut u32, _env: *mut u8, a: *const u8, b: *const u8) {
    // SAFETY: a and b hold i64 values.
    let va = unsafe { a.cast::<i64>().read_unaligned() };
    // SAFETY: a and b hold i64 values.
    let vb = unsafe { b.cast::<i64>().read_unaligned() };
    let res = match va.cmp(&vb) {
        std::cmp::Ordering::Less => 0,
        std::cmp::Ordering::Equal => 1,
        std::cmp::Ordering::Greater => 2,
    };
    // SAFETY: tag is writable.
    unsafe { *tag = res };
}

#[test]
fn test_array_sort_clone_eq_cmp_hash_drops() {
    let arr = loom_array_new(&raw const I64_INFO);
    let v3: i64 = 30;
    let v1: i64 = 10;
    let v2: i64 = 20;

    // SAFETY: arr is live.
    unsafe {
        loom_array_push(arr, std::ptr::from_ref(&v3).cast());
        loom_array_push(arr, std::ptr::from_ref(&v1).cast());
        loom_array_push(arr, std::ptr::from_ref(&v2).cast());
        loom_array_sort_by(arr, i64_sort_cmp, std::ptr::null_mut());

        let e0 = loom_array_get(arr, 0, b"f.lm".as_ptr(), 4, 1, 1)
            .cast::<i64>()
            .read_unaligned();
        let e1 = loom_array_get(arr, 1, b"f.lm".as_ptr(), 4, 1, 1)
            .cast::<i64>()
            .read_unaligned();
        let e2 = loom_array_get(arr, 2, b"f.lm".as_ptr(), 4, 1, 1)
            .cast::<i64>()
            .read_unaligned();
        assert_eq!(e0, 10);
        assert_eq!(e1, 20);
        assert_eq!(e2, 30);
    }

    // Clone & Eq & Cmp & Hash
    // SAFETY: arr is live.
    let cloned_arr = unsafe { loom_array_clone(arr) };
    // SAFETY: arr and cloned_arr are live.
    unsafe {
        assert_eq!(loom_array_eq(arr, cloned_arr), 1);
        assert_eq!(loom_array_cmp(arr, cloned_arr), 0);
        assert_eq!(loom_array_hash(arr), loom_array_hash(cloned_arr));
    }

    // Array debug when debug glue is None
    let no_debug_arr = loom_array_new(&raw const I64_NO_DEBUG_INFO);
    // SAFETY: no_debug_arr is live.
    unsafe {
        loom_array_push(no_debug_arr, std::ptr::from_ref(&v1).cast());
    }
    let debug_out_no_glue = loom_string_new();
    // SAFETY: arguments are live.
    unsafe { loom_array_debug(no_debug_arr, debug_out_no_glue) };
    assert_string_eq(debug_out_no_glue, "[]");

    // SAFETY: drop arrays and debug string.
    unsafe {
        loom_array_drop(arr);
        loom_array_drop(cloned_arr);
        loom_array_drop(no_debug_arr);
        loom_string_drop(debug_out_no_glue);
    }

    // Array of heap-allocated String handles
    let str_arr = loom_array_new(&raw const STRING_HANDLE_INFO);
    // SAFETY: create strings.
    let h1 = unsafe { loom_string_from_static(b"hello".as_ptr(), 5) };
    // SAFETY: create strings.
    let h2 = unsafe { loom_string_from_static(b"world".as_ptr(), 5) };
    // SAFETY: push string handles.
    unsafe {
        loom_array_push(str_arr, std::ptr::from_ref(&h1).cast());
        loom_array_push(str_arr, std::ptr::from_ref(&h2).cast());
    }

    // Clone array of string handles
    // SAFETY: str_arr is live.
    let str_arr_clone = unsafe { loom_array_clone(str_arr) };
    // SAFETY: str_arr and str_arr_clone are live.
    unsafe {
        assert_eq!(loom_array_eq(str_arr, str_arr_clone), 1);
        loom_array_drop(str_arr);
        loom_array_drop(str_arr_clone);
    }

    // Array of Point structs
    let pt_arr = loom_array_new(&raw const POINT_INFO);
    let p1 = Point { x: 1, y: 2 };
    let p2 = Point { x: 3, y: 4 };
    // SAFETY: pt_arr is live.
    unsafe {
        loom_array_push(pt_arr, std::ptr::from_ref(&p1).cast());
        loom_array_push(pt_arr, std::ptr::from_ref(&p2).cast());
    }

    // SAFETY: drop pt_arr.
    unsafe {
        loom_array_drop(pt_arr);
    }
}

#[test]

fn test_array_debug_with_element_glue() {
    let arr = loom_array_new(&raw const I64_INFO);
    let v1: i64 = 10;
    let v2: i64 = 20;
    let v3: i64 = 30;

    // SAFETY: arr is live.
    unsafe {
        loom_array_push(arr, std::ptr::from_ref(&v1).cast());
        loom_array_push(arr, std::ptr::from_ref(&v2).cast());
        loom_array_push(arr, std::ptr::from_ref(&v3).cast());
    }

    let debug_out = loom_string_new();
    // SAFETY: arguments are live.
    unsafe { loom_array_debug(arr, debug_out) };
    assert_string_eq(debug_out, "[10, 20, 30]");

    // SAFETY: drop array and string.
    unsafe {
        loom_array_drop(arr);
        loom_string_drop(debug_out);
    }
}

// --- Map Tests ---

#[test]
fn test_map_insert_get_index_remove_len() {
    let map = loom_map_new(&raw const STRING_HANDLE_INFO, &raw const I64_INFO);
    // SAFETY: map is live.
    unsafe {
        assert_eq!(loom_map_len(map), 0);
        assert_eq!(loom_map_keys_len(map), 0);
    }

    // Key 1
    // SAFETY: create key string.
    let mut k1 = unsafe { loom_string_from_static(b"a".as_ptr(), 1) };
    let v1: i64 = 100;
    let mut old_v: i64 = 0;

    // Insert new key
    // SAFETY: map, k1, v1 are live; old_v has room.
    unsafe {
        let res = loom_map_insert(
            map,
            std::ptr::from_mut(&mut k1).cast(),
            std::ptr::from_ref(&v1).cast(),
            std::ptr::from_mut(&mut old_v).cast(),
        );
        assert_eq!(res, 0);
        assert_eq!(loom_map_len(map), 1);
        assert_eq!(loom_map_keys_len(map), 1);
    }

    // Get & Index present key
    // SAFETY: create lookup key.
    let k1_lookup = unsafe { loom_string_from_static(b"a".as_ptr(), 1) };
    // SAFETY: map and k1_lookup are live.
    unsafe {
        let ptr_get = loom_map_get(map, std::ptr::from_ref(&k1_lookup).cast());
        assert!(!ptr_get.is_null());
        assert_eq!(ptr_get.cast::<i64>().read_unaligned(), 100);

        let ptr_idx = loom_map_index(
            map,
            std::ptr::from_ref(&k1_lookup).cast(),
            b"f.lm".as_ptr(),
            4,
            1,
            1,
        );
        assert!(!ptr_idx.is_null());
        assert_eq!(ptr_idx.cast::<i64>().read_unaligned(), 100);
    }

    // Get absent key
    // SAFETY: create absent key.
    let k_absent = unsafe { loom_string_from_static(b"b".as_ptr(), 1) };
    // SAFETY: map and k_absent are live.
    unsafe {
        let ptr_absent = loom_map_get(map, std::ptr::from_ref(&k_absent).cast());
        assert!(ptr_absent.is_null());
    }

    // Overwrite existing key
    // SAFETY: create new key with same content "a".
    let mut k1_overwrite = unsafe { loom_string_from_static(b"a".as_ptr(), 1) };
    let v1_new: i64 = 200;
    // SAFETY: map, k1_overwrite, v1_new are live; old_v has room.
    unsafe {
        let res = loom_map_insert(
            map,
            std::ptr::from_mut(&mut k1_overwrite).cast(),
            std::ptr::from_ref(&v1_new).cast(),
            std::ptr::from_mut(&mut old_v).cast(),
        );
        assert_eq!(res, 1);
        assert_eq!(old_v, 100);
    }

    // Remove present key
    let mut removed_v: i64 = 0;
    // SAFETY: map, k1_lookup, removed_v are live.
    unsafe {
        let res = loom_map_remove(
            map,
            std::ptr::from_ref(&k1_lookup).cast(),
            std::ptr::from_mut(&mut removed_v).cast(),
        );
        assert_eq!(res, 1);
        assert_eq!(removed_v, 200);
        assert_eq!(loom_map_len(map), 0);
    }

    // Remove absent key
    // SAFETY: map and k_absent are live.
    unsafe {
        let res = loom_map_remove(
            map,
            std::ptr::from_ref(&k_absent).cast(),
            std::ptr::from_mut(&mut removed_v).cast(),
        );
        assert_eq!(res, 0);
    }

    // SAFETY: drop keys and map.
    unsafe {
        loom_string_drop(k1_lookup);
        loom_string_drop(k_absent);
        loom_map_drop(map);
        loom_map_drop(std::ptr::null_mut());
    }
}

unsafe extern "C" fn update_add_five(_result: *mut u8, _env: *mut u8, val: *mut u8) {
    // SAFETY: val points to i64 in map.
    let current = unsafe { val.cast::<i64>().read_unaligned() };
    // SAFETY: write updated value.
    unsafe { val.cast::<i64>().write_unaligned(current + 5) };
}

#[test]
fn test_map_entry_update_clone_eq_debug() {
    let map = loom_map_new(&raw const STRING_HANDLE_INFO, &raw const I64_INFO);

    // SAFETY: create keys.
    let mut kx = unsafe { loom_string_from_static(b"x".as_ptr(), 1) };
    // SAFETY: create keys.
    let mut ky = unsafe { loom_string_from_static(b"y".as_ptr(), 1) };
    let vx: i64 = 10;
    let vy: i64 = 20;
    let mut old: i64 = 0;

    // SAFETY: insert entries.
    unsafe {
        loom_map_insert(
            map,
            std::ptr::from_mut(&mut kx).cast(),
            std::ptr::from_ref(&vx).cast(),
            std::ptr::from_mut(&mut old).cast(),
        );
        loom_map_insert(
            map,
            std::ptr::from_mut(&mut ky).cast(),
            std::ptr::from_ref(&vy).cast(),
            std::ptr::from_mut(&mut old).cast(),
        );
    }

    // Entry
    let mut key_out: *mut u8 = std::ptr::null_mut();
    let mut val_out: *mut u8 = std::ptr::null_mut();
    // SAFETY: map and pointers are live.
    unsafe {
        loom_map_entry(
            map,
            0,
            std::ptr::from_mut(&mut key_out),
            std::ptr::from_mut(&mut val_out),
            b"f.lm".as_ptr(),
            4,
            1,
            1,
        );
        assert!(!key_out.is_null());
        assert!(!val_out.is_null());
        let handle = key_out.cast::<*mut LoomString>().read_unaligned();
        assert_string_eq(handle, "x");
        assert_eq!(val_out.cast::<i64>().read_unaligned(), 10);
    }

    // Update existing key
    // SAFETY: create key "x".
    let mut kx_upd = unsafe { loom_string_from_static(b"x".as_ptr(), 1) };
    let unused_def: i64 = 999;
    // SAFETY: map and kx_upd are live.
    unsafe {
        loom_map_update(
            map,
            std::ptr::from_mut(&mut kx_upd).cast(),
            std::ptr::from_ref(&unused_def).cast::<u8>().cast_mut(),
            update_add_five,
            std::ptr::null_mut(),
        );
    }

    // Update absent key (inserts default and calls closure)
    // SAFETY: create key "z".
    let mut key_z_upd = unsafe { loom_string_from_static(b"z".as_ptr(), 1) };
    let def_z: i64 = 30;
    // SAFETY: map and key_z_upd are live.
    unsafe {
        loom_map_update(
            map,
            std::ptr::from_mut(&mut key_z_upd).cast(),
            std::ptr::from_ref(&def_z).cast::<u8>().cast_mut(),
            update_add_five,
            std::ptr::null_mut(),
        );
    }

    // Clone map
    // SAFETY: map is live.
    let cloned_map = unsafe { loom_map_clone(map) };
    // SAFETY: map and cloned_map are live.
    unsafe {
        assert_eq!(loom_map_eq(map, cloned_map), 1);
    }

    // Debug
    let debug_out = loom_string_new();
    // SAFETY: map and debug_out are live.
    unsafe { loom_map_debug(map, debug_out) };
    assert_string_eq(debug_out, "{\"x\" => 15, \"y\" => 20, \"z\" => 35}");

    // SAFETY: drop maps and debug string.
    unsafe {
        loom_map_drop(map);
        loom_map_drop(cloned_map);
        loom_string_drop(debug_out);
    }
}

// --- Number Tests ---

#[test]
fn test_number_formatting() {
    let s_int = loom_int_to_string(-42);
    assert_string_eq(s_int, "-42");

    let str_uint = loom_uint_to_string(42);
    assert_string_eq(str_uint, "42");

    let s_i128_u = loom_int128_to_string(u64::MAX, u64::MAX, 0);
    assert_string_eq(s_i128_u, "340282366920938463463374607431768211455");

    let s_i128_s = loom_int128_to_string(u64::MAX, u64::MAX, 1);
    assert_string_eq(s_i128_s, "-1");

    // Spec §7.12 rule 2 float examples
    let f1 = loom_float_to_string(7.0, 0);
    assert_string_eq(f1, "7.0");

    let f2 = loom_float_to_string(0.25, 0);
    assert_string_eq(f2, "0.25");

    let f3 = loom_float_to_string(1e16, 0);
    assert_string_eq(f3, "1e16");

    let f4 = loom_float_to_string(1.5e-7, 0);
    assert_string_eq(f4, "1.5e-7");

    let f_nan = loom_float_to_string(f64::NAN, 0);
    assert_string_eq(f_nan, "NaN");

    let f_inf = loom_float_to_string(f64::INFINITY, 0);
    assert_string_eq(f_inf, "inf");

    let f_neginf = loom_float_to_string(f64::NEG_INFINITY, 0);
    assert_string_eq(f_neginf, "-inf");

    // f32 float formatting
    let f32_val = loom_float_to_string(7.0, 1);
    assert_string_eq(f32_val, "7.0");

    // SAFETY: drop created strings.
    unsafe {
        loom_string_drop(s_int);
        loom_string_drop(str_uint);
        loom_string_drop(s_i128_u);
        loom_string_drop(s_i128_s);
        loom_string_drop(f1);
        loom_string_drop(f2);
        loom_string_drop(f3);
        loom_string_drop(f4);
        loom_string_drop(f_nan);
        loom_string_drop(f_inf);
        loom_string_drop(f_neginf);
        loom_string_drop(f32_val);
    }
}

#[test]
fn test_number_parsing_bounds() {
    let mut out_i64: i64 = 0;

    // I8 bounds
    // SAFETY: create strings.
    let s_i8_min = unsafe { loom_string_from_static(b"-128".as_ptr(), 4) };
    // SAFETY: create strings.
    let s_i8_max = unsafe { loom_string_from_static(b"127".as_ptr(), 3) };
    // SAFETY: create strings.
    let s_i8_oob = unsafe { loom_string_from_static(b"128".as_ptr(), 3) };

    // SAFETY: string and out_i64 are valid.
    unsafe {
        assert_eq!(loom_int_parse(s_i8_min, 8, 1, &raw mut out_i64), 1);
        assert_eq!(out_i64, -128);

        assert_eq!(loom_int_parse(s_i8_max, 8, 1, &raw mut out_i64), 1);
        assert_eq!(out_i64, 127);

        assert_eq!(loom_int_parse(s_i8_oob, 8, 1, &raw mut out_i64), 0);
    }

    // U8 bounds
    // SAFETY: create strings.
    let str_u8_min = unsafe { loom_string_from_static(b"0".as_ptr(), 1) };
    // SAFETY: create strings.
    let str_u8_max = unsafe { loom_string_from_static(b"255".as_ptr(), 3) };
    // SAFETY: create strings.
    let str_u8_oob = unsafe { loom_string_from_static(b"256".as_ptr(), 3) };

    // SAFETY: string and out_i64 are valid.
    unsafe {
        assert_eq!(loom_int_parse(str_u8_min, 8, 0, &raw mut out_i64), 1);
        assert_eq!(out_i64, 0);

        assert_eq!(loom_int_parse(str_u8_max, 8, 0, &raw mut out_i64), 1);
        assert_eq!(out_i64, 255);

        assert_eq!(loom_int_parse(str_u8_oob, 8, 0, &raw mut out_i64), 0);
    }

    // I64 bounds
    // SAFETY: create strings.
    let s_i64_min = unsafe { loom_string_from_static(b"-9223372036854775808".as_ptr(), 20) };
    // SAFETY: create strings.
    let s_i64_max = unsafe { loom_string_from_static(b"9223372036854775807".as_ptr(), 19) };

    // SAFETY: string and out_i64 are valid.
    unsafe {
        assert_eq!(loom_int_parse(s_i64_min, 64, 1, &raw mut out_i64), 1);
        assert_eq!(out_i64, i64::MIN);

        assert_eq!(loom_int_parse(s_i64_max, 64, 1, &raw mut out_i64), 1);
        assert_eq!(out_i64, i64::MAX);
    }

    // Float parsing
    let mut parsed_f64: f64 = 0.0;
    // SAFETY: create strings.
    let s_float1 = unsafe { loom_string_from_static(b"2.5".as_ptr(), 3) };
    // SAFETY: create strings.
    let s_float2 = unsafe { loom_string_from_static(b"invalid".as_ptr(), 7) };

    // SAFETY: string and parsed_f64 are valid.
    unsafe {
        assert_eq!(loom_float_parse(s_float1, &raw mut parsed_f64), 1);
        assert!((parsed_f64 - 2.5).abs() < 1e-6);

        assert_eq!(loom_float_parse(s_float2, &raw mut parsed_f64), 0);
    }

    // SAFETY: drop strings.
    unsafe {
        loom_string_drop(s_i8_min);
        loom_string_drop(s_i8_max);
        loom_string_drop(s_i8_oob);
        loom_string_drop(str_u8_min);
        loom_string_drop(str_u8_max);
        loom_string_drop(str_u8_oob);
        loom_string_drop(s_i64_min);
        loom_string_drop(s_i64_max);
        loom_string_drop(s_float1);
        loom_string_drop(s_float2);
    }
}

#[test]
fn test_float_round_and_debug_helpers() {
    assert!((loom_float_round_to(12.34567, 2) - 12.35).abs() < 1e-6);

    let out_dbg = loom_string_new();
    // SAFETY: out_dbg is live.
    unsafe {
        loom_debug_int(123, out_dbg);
        loom_debug_float(45.6, out_dbg);
        loom_debug_text(b" text".as_ptr(), 5, out_dbg);
    }
    assert_string_eq(out_dbg, "12345.6 text");

    // SAFETY: drop string.
    unsafe { loom_string_drop(out_dbg) };
}

// --- Memory Allocation Test ---

#[test]
fn test_loom_alloc_free() {
    let ptr = loom_alloc(64, 8);
    assert!(!ptr.is_null());
    // SAFETY: ptr points to allocated 64 bytes.
    unsafe {
        ptr.write_bytes(0xAB, 64);
        loom_free(ptr, 64, 8);
        loom_free(std::ptr::null_mut(), 64, 8);
    }
}
