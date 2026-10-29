//! The runtime functions native code calls (ADR 0021 §4): their C signatures, and their
//! addresses for the JIT.

use cranelift_codegen::ir::{AbiParam, Signature, Type, types};
use cranelift_codegen::isa::CallConv;

/// A runtime function: its symbol, its signature (see [`signature`]) and its address in
/// this process.
pub struct RuntimeFn {
    /// The symbol.
    pub name: &'static str,
    /// Parameter and result codes: `p` pointer, `z` usize, `b` u8, `c` i8, `w` u32, `l`
    /// i64, `q` u64, `d` f64; parameters, then `>`, then the result (if any).
    pub sig: &'static str,
    /// The function's address, for the JIT.
    pub addr: *const u8,
}

macro_rules! runtime {
    ($($module:ident :: $name:ident = $sig:literal;)*) => {
        /// Every runtime function native code may call.
        pub fn functions() -> Vec<RuntimeFn> {
            vec![$(RuntimeFn {
                name: stringify!($name),
                sig: $sig,
                addr: loom_runtime::$module::$name as *const u8,
            }),*]
        }

        /// The signature code of runtime function `name`.
        pub fn sig_of(name: &str) -> Option<&'static str> {
            match name {
                $(stringify!($name) => Some($sig),)*
                _ => None,
            }
        }
    };
}

runtime! {
    root::loom_flush = ">";
    root::loom_panic = "pzpzww>";
    root::loom_panic_string = "ppzww>";
    root::loom_exit_error = "p>";
    root::loom_alloc = "zz>p";
    root::loom_free = "pzz>";
    root::loom_name_code = "p>w";
    root::loom_int_arith = "wwppwbp>b";
    root::loom_int_pow = "pwwbp>b";
    string::loom_string_from_static = "pz>p";
    string::loom_string_new = ">p";
    string::loom_string_clone = "p>p";
    string::loom_string_drop = "p>";
    string::loom_string_push_str = "pp>";
    string::loom_string_push_char = "pw>";
    string::loom_write_stdout = "p>";
    string::loom_string_length = "p>l";
    string::loom_string_byte_length = "p>l";
    string::loom_string_eq = "pp>b";
    string::loom_string_cmp = "pp>c";
    string::loom_string_hash = "p>q";
    string::loom_string_convert = "pw>p";
    string::loom_string_chars = "pp>p";
    string::loom_string_from_chars = "p>p";
    string::loom_string_split_whitespace = "pp>p";
    string::loom_string_split = "ppp>p";
    string::loom_string_find = "pp>l";
    string::loom_string_substring = "pllpzww>p";
    string::loom_string_repeat = "pl>p";
    string::loom_char_to_string = "w>p";
    string::loom_char_is = "ww>b";
    string::loom_char_convert = "ww>w";
    string::loom_string_debug = "pp>";
    string::loom_char_debug = "wp>";
    array::loom_array_new = "p>p";
    array::loom_array_len = "p>l";
    array::loom_array_get = "plpzww>p";
    array::loom_array_push = "pp>";
    array::loom_array_pop = "pp>b";
    array::loom_array_insert = "plppzww>";
    array::loom_array_remove = "plppzww>";
    array::loom_array_slice = "pll>p";
    array::loom_array_reverse = "p>";
    array::loom_array_sort_by = "ppp>";
    array::loom_array_clone = "p>p";
    array::loom_array_drop = "p>";
    array::loom_array_eq = "pp>b";
    array::loom_array_cmp = "pp>c";
    array::loom_array_hash = "p>q";
    array::loom_array_debug = "pp>";
    map::loom_map_new = "pp>p";
    map::loom_map_len = "p>l";
    map::loom_map_insert = "pppp>b";
    map::loom_map_get = "pp>p";
    map::loom_map_index = "pppzww>p";
    map::loom_map_remove = "ppp>b";
    map::loom_map_entry = "plpppzww>";
    map::loom_map_update = "ppppp>";
    map::loom_map_clone = "p>p";
    map::loom_map_drop = "p>";
    map::loom_map_eq = "pp>b";
    map::loom_map_debug = "pp>";
    num::loom_int_to_string = "l>p";
    num::loom_uint_to_string = "q>p";
    num::loom_int128_to_string = "qqb>p";
    num::loom_float_to_string = "db>p";
    num::loom_int_parse = "pwbp>b";
    num::loom_float_parse = "pp>b";
    num::loom_float_round_to = "dl>d";
    num::loom_float_rem = "dd>d";
    num::loom_float_math = "dw>d";
    num::loom_debug_text = "pzp>";
}

/// The Cranelift type of a signature code.
fn code_type(code: char, ptr: Type) -> AbiParam {
    match code {
        'b' => AbiParam::new(types::I8).uext(),
        'c' => AbiParam::new(types::I8).sext(),
        'w' => AbiParam::new(types::I32),
        'l' | 'q' => AbiParam::new(types::I64),
        'd' => AbiParam::new(types::F64),
        _ => AbiParam::new(ptr),
    }
}

/// The signature of runtime function `name`.
pub fn signature(name: &str, ptr: Type, call_conv: CallConv) -> Signature {
    let code = sig_of(name).unwrap_or(">");
    let (params, ret) = code.split_once('>').unwrap_or((code, ""));
    let mut sig = Signature::new(call_conv);
    sig.params = params.chars().map(|c| code_type(c, ptr)).collect();
    sig.returns = ret.chars().map(|c| code_type(c, ptr)).collect();
    sig
}
