//! `Map[K, V]` objects: insertion-ordered hash maps over keys and values described by
//! [`TypeInfo`] descriptors.

use crate::TypeInfo;
use crate::array::{
    LoomArray, loom_array_drop, loom_array_get, loom_array_len, loom_array_new, loom_array_push,
};
use crate::string::LoomString;
use std::collections::HashMap;

/// The heap object behind a Loom `Map[K, V]`.
#[derive(Debug)]
pub struct LoomMap {
    keys: *mut LoomArray,
    values: *mut LoomArray,
    key_info: *const TypeInfo,
    index: HashMap<u64, Vec<usize>>,
}

impl LoomMap {
    fn key_info(&self) -> &TypeInfo {
        // SAFETY: descriptors are static data.
        unsafe { &*self.key_info }
    }

    fn len(&self) -> usize {
        // SAFETY: the key array is owned by the map and live.
        unsafe { &*self.keys }.len
    }

    fn key_ptr(&self, i: usize) -> *mut u8 {
        // SAFETY: `i` is in bounds of the live key array.
        unsafe { (*self.keys).element(i) }
    }

    fn value_ptr(&self, i: usize) -> *mut u8 {
        // SAFETY: `i` is in bounds of the live value array.
        unsafe { (*self.values).element(i) }
    }

    fn hash(&self, key: *const u8) -> u64 {
        match self.key_info().hash {
            // SAFETY: `key` points to a key value.
            Some(hash) => unsafe { hash(key) },
            None => 0,
        }
    }

    fn find(&self, key: *const u8) -> Option<usize> {
        let positions = self.index.get(&self.hash(key))?;
        let eq = self.key_info().eq?;
        positions
            .iter()
            .copied()
            // SAFETY: both point to key values.
            .find(|&i| unsafe { eq(self.key_ptr(i), key) } != 0)
    }

    fn rebuild_index(&mut self) {
        self.index.clear();
        for i in 0..self.len() {
            let h = self.hash(self.key_ptr(i));
            self.index.entry(h).or_default().push(i);
        }
    }
}

/// Borrows the map behind a handle.
///
/// # Safety
///
/// `m` must be a live, unaliased map handle.
unsafe fn get<'a>(m: *mut LoomMap) -> &'a mut LoomMap {
    // SAFETY: guaranteed by the caller.
    unsafe { &mut *m }
}

/// A new empty map.
#[unsafe(no_mangle)]
pub extern "C" fn loom_map_new(
    key_info: *const TypeInfo,
    value_info: *const TypeInfo,
) -> *mut LoomMap {
    Box::into_raw(Box::new(LoomMap {
        keys: loom_array_new(key_info),
        values: loom_array_new(value_info),
        key_info,
        index: HashMap::new(),
    }))
}

/// The number of entries.
///
/// # Safety
///
/// `m` must be live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_map_len(m: *mut LoomMap) -> i64 {
    // SAFETY: `m` is live.
    i64::try_from(unsafe { get(m) }.len()).unwrap_or(i64::MAX)
}

/// Inserts the key and value at the pointers (both moved in). If the key was present, its
/// old value is moved to `old` and the new key is dropped; returns 1 then, 0 otherwise.
///
/// # Safety
///
/// `m` must be live; the pointers must hold a key and a value; `old` must have room for a value.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_map_insert(
    m: *mut LoomMap,
    key: *mut u8,
    value: *const u8,
    old: *mut u8,
) -> u8 {
    // SAFETY: `m` is live.
    let map = unsafe { get(m) };
    // SAFETY: the value array is live.
    let value_info = unsafe { &*(*map.values).info };
    if let Some(i) = map.find(key) {
        // SAFETY: entry `i` exists; its value moves to `old`, the new one takes its place.
        unsafe {
            std::ptr::copy_nonoverlapping(map.value_ptr(i), old, value_info.size);
            std::ptr::copy_nonoverlapping(value, map.value_ptr(i), value_info.size);
        }
        if let Some(drop_key) = map.key_info().drop {
            // SAFETY: the incoming key is not stored, so it is dropped here.
            unsafe { drop_key(key) };
        }
        return 1;
    }
    let h = map.hash(key);
    let position = map.len();
    // SAFETY: both arrays are live and the pointers hold elements.
    unsafe {
        loom_array_push(map.keys, key);
        loom_array_push(map.values, value);
    }
    map.index.entry(h).or_default().push(position);
    0
}

/// A pointer to the value for `key`, or null.
///
/// # Safety
///
/// `m` must be live and `key` must hold a key.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_map_get(m: *mut LoomMap, key: *const u8) -> *mut u8 {
    // SAFETY: `m` is live.
    let map = unsafe { get(m) };
    map.find(key)
        .map_or(std::ptr::null_mut(), |i| map.value_ptr(i))
}

/// A pointer to the value for `key`; panics if there is none.
///
/// # Safety
///
/// `m` must be live, `key` must hold a key, and the file pointer must be valid UTF-8.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_map_index(
    m: *mut LoomMap,
    key: *const u8,
    file: *const u8,
    file_len: usize,
    line: u32,
    column: u32,
) -> *mut u8 {
    // SAFETY: forwarded.
    let found = unsafe { loom_map_get(m, key) };
    if found.is_null() {
        // SAFETY: forwarded.
        let file = unsafe { crate::str_from(file, file_len) };
        crate::panic_at("key not found in map", file, line, column);
    }
    found
}

/// Removes `key`, moving its value to `out`; returns 0 if it was absent.
///
/// # Safety
///
/// `m` must be live, `key` must hold a key, and `out` must have room for a value.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_map_remove(m: *mut LoomMap, key: *const u8, out: *mut u8) -> u8 {
    // SAFETY: `m` is live.
    let map = unsafe { get(m) };
    let Some(i) = map.find(key) else { return 0 };
    let (here, nowhere) = (b"<map>".as_ptr(), 5);
    let index = i64::try_from(i).unwrap_or(i64::MAX);
    let mut scratch = vec![0u8; map.key_info().size.max(1)];
    // SAFETY: entry `i` exists; the key is removed and dropped, the value moved to `out`.
    unsafe {
        crate::array::loom_array_remove(map.keys, index, scratch.as_mut_ptr(), here, nowhere, 0, 0);
        crate::array::loom_array_remove(map.values, index, out, here, nowhere, 0, 0);
        if let Some(drop_key) = map.key_info().drop {
            drop_key(scratch.as_mut_ptr());
        }
    }
    map.rebuild_index();
    1
}

/// Pointers to the key and value of entry `index` (in insertion order).
///
/// # Safety
///
/// `m` must be live; the file pointer must be valid UTF-8.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_map_entry(
    m: *mut LoomMap,
    index: i64,
    key_out: *mut *mut u8,
    value_out: *mut *mut u8,
    file: *const u8,
    file_len: usize,
    line: u32,
    column: u32,
) {
    // SAFETY: `m` is live.
    let map = unsafe { get(m) };
    // SAFETY: forwarded; the arrays are live.
    unsafe {
        *key_out = loom_array_get(map.keys, index, file, file_len, line, column);
        *value_out = loom_array_get(map.values, index, file, file_len, line, column);
    }
}

/// The calling convention of an update closure (ADR 0021 §3): (result, environment,
/// value); the result is `Unit`, so nothing is written to it.
pub type UpdateFn = unsafe extern "C" fn(*mut u8, *mut u8, *mut u8);

/// Calls the closure with the value for `key`, inserting `default` first if absent. The
/// key and default are moved in (and dropped if not stored).
///
/// # Safety
///
/// `m` must be live; the pointers must hold a key and a value; `code`/`env` must be a
/// closure taking a `mut` value.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_map_update(
    m: *mut LoomMap,
    key: *mut u8,
    default: *mut u8,
    code: UpdateFn,
    env: *mut u8,
) {
    // SAFETY: `m` is live.
    let map = unsafe { get(m) };
    let i = if let Some(i) = map.find(key) {
        // SAFETY: the incoming key and default are not stored, so they are dropped.
        unsafe {
            if let Some(drop_key) = map.key_info().drop {
                drop_key(key);
            }
            if let Some(drop_value) = (*(*map.values).info).drop {
                drop_value(default);
            }
        }
        i
    } else {
        let h = map.hash(key);
        let position = map.len();
        // SAFETY: both arrays are live and the pointers hold elements.
        unsafe {
            loom_array_push(map.keys, key);
            loom_array_push(map.values, default);
        }
        map.index.entry(h).or_default().push(position);
        position
    };
    let value = map.value_ptr(i);
    let mut unit = 0u8;
    // SAFETY: the closure receives a pointer to a live value it may mutate.
    unsafe { code(&raw mut unit, env, value) };
}

/// A clone of the map.
///
/// # Safety
///
/// `m` must be live with clone glue for keys and values (or plain data).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_map_clone(m: *mut LoomMap) -> *mut LoomMap {
    // SAFETY: `m` is live.
    let map = unsafe { get(m) };
    Box::into_raw(Box::new(LoomMap {
        // SAFETY: the arrays are live.
        keys: unsafe { crate::array::loom_array_clone(map.keys) },
        // SAFETY: the arrays are live.
        values: unsafe { crate::array::loom_array_clone(map.values) },
        key_info: map.key_info,
        index: map.index.clone(),
    }))
}

/// Drops the entries and frees the map.
///
/// # Safety
///
/// `m` must be live and not used afterwards.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_map_drop(m: *mut LoomMap) {
    if m.is_null() {
        return;
    }
    // SAFETY: the caller transfers ownership.
    let map = unsafe { Box::from_raw(m) };
    // SAFETY: the arrays are owned by the map.
    unsafe {
        loom_array_drop(map.keys);
        loom_array_drop(map.values);
    }
}

/// Whether two maps have the same entries (in any order).
///
/// # Safety
///
/// Both must be live maps of the same types with eq glue.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_map_eq(a: *mut LoomMap, b: *mut LoomMap) -> u8 {
    // SAFETY: both are live; they are only read, so they may be the same map.
    let (left, right) = (unsafe { &*a }, unsafe { &*b });
    if left.len() != right.len() {
        return 0;
    }
    // SAFETY: the value array is live.
    let Some(eq) = (unsafe { &*(*left.values).info }).eq else {
        return 0;
    };
    for index in 0..left.len() {
        match right.find(left.key_ptr(index)) {
            // SAFETY: both point to values.
            Some(other) if unsafe { eq(left.value_ptr(index), right.value_ptr(other)) } != 0 => {}
            _ => return 0,
        }
    }
    1
}

/// Appends `{k => v, …}`.
///
/// # Safety
///
/// `m` and `out` must be live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_map_debug(m: *mut LoomMap, out: *mut LoomString) {
    // SAFETY: `m` is live.
    let map = unsafe { get(m) };
    // SAFETY: `out` is live and distinct from the map.
    unsafe { &mut *out }.push_str("{");
    // SAFETY: the arrays are live.
    let (key_debug, value_debug) =
        unsafe { ((*(*map.keys).info).debug, (*(*map.values).info).debug) };
    for i in 0..map.len() {
        // SAFETY: `out` is live; the glue appends to it.
        unsafe {
            if i > 0 {
                (*out).push_str(", ");
            }
            if let Some(d) = key_debug {
                d(map.key_ptr(i), out);
            }
            (*out).push_str(" => ");
            if let Some(d) = value_debug {
                d(map.value_ptr(i), out);
            }
        }
    }
    // SAFETY: `out` is live.
    unsafe { &mut *out }.push_str("}");
}

/// Keeps `loom_array_len` reachable for map users that iterate by index.
///
/// # Safety
///
/// `m` must be live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_map_keys_len(m: *mut LoomMap) -> i64 {
    // SAFETY: `m` is live.
    unsafe { loom_array_len(get(m).keys) }
}
