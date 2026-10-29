//! Switching between execution contexts (ADR 0022 §1).
//!
//! A suspended context is a stack pointer: [`switch`] pushes the callee-saved registers
//! of the running context on its stack, stores its stack pointer, loads another one and
//! pops that context's registers. A new context gets an initial frame ([`prepare`]) whose
//! "return address" is a trampoline that calls the task's entry function.

/// The symbol of a C function in assembly (Apple platforms prefix an underscore).
#[cfg(target_vendor = "apple")]
macro_rules! c_name {
    ($name:literal) => {
        concat!("_", $name)
    };
}
#[cfg(not(target_vendor = "apple"))]
macro_rules! c_name {
    ($name:literal) => {
        $name
    };
}

/// The saved stack pointer of a suspended context.
pub type Sp = *mut u8;

/// The entry function of a new context. It must never return; a finished task switches
/// away for the last time instead.
pub type Entry = unsafe extern "C" fn(*mut u8) -> !;

unsafe extern "C" {
    /// Saves the running context's stack pointer to `*save` and resumes `to`.
    ///
    /// # Safety
    ///
    /// `to` must be a stack pointer saved by `switch` or made by [`prepare`], whose
    /// context is not running.
    pub fn loom_context_switch(save: *mut Sp, to: Sp);
}

#[cfg(target_arch = "aarch64")]
core::arch::global_asm!(
    ".p2align 2",
    concat!(".globl ", c_name!("loom_context_switch")),
    concat!(c_name!("loom_context_switch"), ":"),
    "sub sp, sp, #0xa0",
    "stp x19, x20, [sp, #0x00]",
    "stp x21, x22, [sp, #0x10]",
    "stp x23, x24, [sp, #0x20]",
    "stp x25, x26, [sp, #0x30]",
    "stp x27, x28, [sp, #0x40]",
    "stp x29, x30, [sp, #0x50]",
    "stp d8, d9, [sp, #0x60]",
    "stp d10, d11, [sp, #0x70]",
    "stp d12, d13, [sp, #0x80]",
    "stp d14, d15, [sp, #0x90]",
    "mov x9, sp",
    "str x9, [x0]",
    "mov sp, x1",
    "ldp x19, x20, [sp, #0x00]",
    "ldp x21, x22, [sp, #0x10]",
    "ldp x23, x24, [sp, #0x20]",
    "ldp x25, x26, [sp, #0x30]",
    "ldp x27, x28, [sp, #0x40]",
    "ldp x29, x30, [sp, #0x50]",
    "ldp d8, d9, [sp, #0x60]",
    "ldp d10, d11, [sp, #0x70]",
    "ldp d12, d13, [sp, #0x80]",
    "ldp d14, d15, [sp, #0x90]",
    "add sp, sp, #0xa0",
    "ret",
    ".p2align 2",
    concat!(".globl ", c_name!("loom_context_trampoline")),
    concat!(c_name!("loom_context_trampoline"), ":"),
    "mov x0, x20",
    "mov x29, xzr",
    "blr x19",
    "brk #0",
);

#[cfg(all(target_arch = "x86_64", not(windows)))]
core::arch::global_asm!(
    ".p2align 4",
    concat!(".globl ", c_name!("loom_context_switch")),
    concat!(c_name!("loom_context_switch"), ":"),
    "push rbp",
    "push rbx",
    "push r12",
    "push r13",
    "push r14",
    "push r15",
    "sub rsp, 8",
    "stmxcsr [rsp]",
    "fnstcw [rsp + 4]",
    "mov [rdi], rsp",
    "mov rsp, rsi",
    "ldmxcsr [rsp]",
    "fldcw [rsp + 4]",
    "add rsp, 8",
    "pop r15",
    "pop r14",
    "pop r13",
    "pop r12",
    "pop rbx",
    "pop rbp",
    "ret",
    ".p2align 4",
    concat!(".globl ", c_name!("loom_context_trampoline")),
    concat!(c_name!("loom_context_trampoline"), ":"),
    "mov rdi, r13",
    "xor ebp, ebp",
    "and rsp, -16",
    "call r12",
    "ud2",
);

#[cfg(all(target_arch = "x86_64", windows))]
core::arch::global_asm!(
    ".p2align 4",
    concat!(".globl ", c_name!("loom_context_switch")),
    concat!(c_name!("loom_context_switch"), ":"),
    "push rbp",
    "push rbx",
    "push rdi",
    "push rsi",
    "push r12",
    "push r13",
    "push r14",
    "push r15",
    "push qword ptr gs:[0x08]",
    "push qword ptr gs:[0x10]",
    "push qword ptr gs:[0x1478]",
    "sub rsp, 168",
    "stmxcsr [rsp]",
    "fnstcw [rsp + 4]",
    "movups [rsp + 8], xmm6",
    "movups [rsp + 24], xmm7",
    "movups [rsp + 40], xmm8",
    "movups [rsp + 56], xmm9",
    "movups [rsp + 72], xmm10",
    "movups [rsp + 88], xmm11",
    "movups [rsp + 104], xmm12",
    "movups [rsp + 120], xmm13",
    "movups [rsp + 136], xmm14",
    "movups [rsp + 152], xmm15",
    "mov [rcx], rsp",
    "mov rsp, rdx",
    "ldmxcsr [rsp]",
    "fldcw [rsp + 4]",
    "movups xmm6, [rsp + 8]",
    "movups xmm7, [rsp + 24]",
    "movups xmm8, [rsp + 40]",
    "movups xmm9, [rsp + 56]",
    "movups xmm10, [rsp + 72]",
    "movups xmm11, [rsp + 88]",
    "movups xmm12, [rsp + 104]",
    "movups xmm13, [rsp + 120]",
    "movups xmm14, [rsp + 136]",
    "movups xmm15, [rsp + 152]",
    "add rsp, 168",
    "pop qword ptr gs:[0x1478]",
    "pop qword ptr gs:[0x10]",
    "pop qword ptr gs:[0x08]",
    "pop r15",
    "pop r14",
    "pop r13",
    "pop r12",
    "pop rsi",
    "pop rdi",
    "pop rbx",
    "pop rbp",
    "ret",
    ".p2align 4",
    concat!(".globl ", c_name!("loom_context_trampoline")),
    concat!(c_name!("loom_context_trampoline"), ":"),
    "mov rcx, r13",
    "xor ebp, ebp",
    "and rsp, -16",
    "sub rsp, 32",
    "call r12",
    "ud2",
);

unsafe extern "C" {
    fn loom_context_trampoline();
}

/// Writes `value` at `sp + offset`.
///
/// # Safety
///
/// The address must be inside a writable stack.
unsafe fn put(sp: Sp, offset: usize, value: usize) {
    // SAFETY: guaranteed by the caller.
    unsafe { sp.add(offset).cast::<usize>().write_unaligned(value) };
}

/// Prepares a new context on the stack `[limit, top)` that will call `entry(arg)` when
/// first switched to; returns its stack pointer.
///
/// # Safety
///
/// `top` must be the 16-byte-aligned end of a writable stack of at least a few hundred
/// bytes, not used by any other context; `base` and `limit` describe the same stack.
#[must_use]
pub unsafe fn prepare(
    top: *mut u8,
    limit: *mut u8,
    base: *mut u8,
    entry: Entry,
    arg: *mut u8,
) -> Sp {
    let trampoline = loom_context_trampoline as *const () as usize;
    let _ = (limit, base);
    #[cfg(target_arch = "aarch64")]
    // SAFETY: the frame lies just below `top`, inside the stack.
    unsafe {
        let sp = top.sub(0xa0);
        std::ptr::write_bytes(sp, 0, 0xa0);
        put(sp, 0x00, entry as *const () as usize);
        put(sp, 0x08, arg as usize);
        put(sp, 0x58, trampoline);
        sp
    }
    #[cfg(all(target_arch = "x86_64", not(windows)))]
    // SAFETY: the frame lies just below `top`, inside the stack.
    unsafe {
        let sp = top.sub(72);
        std::ptr::write_bytes(sp, 0, 72);
        sp.cast::<u32>().write_unaligned(0x1F80);
        sp.add(4).cast::<u16>().write_unaligned(0x037F);
        put(sp, 24, arg as usize); // r13
        put(sp, 32, entry as *const () as usize); // r12
        put(sp, 56, trampoline);
        sp
    }
    #[cfg(all(target_arch = "x86_64", windows))]
    // SAFETY: the frame lies just below `top`, inside the stack.
    unsafe {
        let sp = top.sub(272);
        std::ptr::write_bytes(sp, 0, 272);
        sp.cast::<u32>().write_unaligned(0x1F80);
        sp.add(4).cast::<u16>().write_unaligned(0x037F);
        // After the 168-byte block: DeallocationStack, StackLimit, StackBase.
        put(sp, 168, base as usize);
        put(sp, 176, limit as usize);
        put(sp, 184, top as usize);
        // Then r15, r14, r13, r12, rsi, rdi, rbx, rbp and the return address.
        put(sp, 192 + 16, arg as usize); // r13
        put(sp, 192 + 24, entry as *const () as usize); // r12
        put(sp, 192 + 64, trampoline);
        sp
    }
}
