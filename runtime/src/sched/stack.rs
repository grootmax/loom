//! Task stacks (ADR 0022 §1): a virtual-memory reservation with an inaccessible guard
//! region at its low end. On Unix the pages are committed lazily by the OS; on Windows
//! the whole stack is committed.

/// Size of the guard region (a multiple of every supported page size).
pub const GUARD: usize = 64 * 1024;

/// Granularity of stack sizes.
const GRANULE: usize = 64 * 1024;

/// How much of a stack is committed up front on Windows.
#[cfg(windows)]
const WINDOWS_COMMIT: usize = 64 * 1024;

/// A task stack.
#[derive(Debug)]
pub struct Stack {
    /// The lowest address of the reservation (the start of the guard region).
    base: *mut u8,
    /// The size of the reservation, guard included.
    size: usize,
}

// SAFETY: a stack is plain memory owned by one task at a time; moving the owner between
// threads (a task resuming on another worker) is what stacks are for.
unsafe impl Send for Stack {}

#[cfg(unix)]
mod sys {
    use std::ffi::{c_int, c_long, c_void};

    pub const PROT_NONE: c_int = 0;
    pub const PROT_READ: c_int = 1;
    pub const PROT_WRITE: c_int = 2;
    pub const MAP_PRIVATE: c_int = 0x02;
    #[cfg(target_os = "linux")]
    pub const MAP_ANONYMOUS: c_int = 0x20;
    #[cfg(not(target_os = "linux"))]
    pub const MAP_ANONYMOUS: c_int = 0x1000;
    #[cfg(target_os = "linux")]
    pub const MAP_NORESERVE: c_int = 0x4000;
    #[cfg(not(target_os = "linux"))]
    pub const MAP_NORESERVE: c_int = 0x40;

    unsafe extern "C" {
        pub fn mmap(
            addr: *mut c_void,
            len: usize,
            prot: c_int,
            flags: c_int,
            fd: c_int,
            offset: i64,
        ) -> *mut c_void;
        pub fn munmap(addr: *mut c_void, len: usize) -> c_int;
        pub fn mprotect(addr: *mut c_void, len: usize, prot: c_int) -> c_int;
    }

    #[allow(dead_code, reason = "kept for page-size queries")]
    pub type Long = c_long;
}

#[cfg(windows)]
mod sys {
    use std::ffi::c_void;

    pub const PAGE_GUARD: u32 = 0x100;
    pub const MEM_COMMIT: u32 = 0x1000;
    pub const MEM_RESERVE: u32 = 0x2000;
    pub const MEM_RELEASE: u32 = 0x8000;
    pub const PAGE_READWRITE: u32 = 0x04;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        pub fn VirtualAlloc(addr: *mut c_void, size: usize, kind: u32, protect: u32)
        -> *mut c_void;
        pub fn VirtualFree(addr: *mut c_void, size: usize, kind: u32) -> i32;
    }
}

impl Stack {
    /// A new stack with at least `usable` bytes above the guard region.
    ///
    /// # Panics
    ///
    /// If the operating system refuses the reservation.
    #[must_use]
    pub fn new(usable: usize) -> Self {
        let size = usable.next_multiple_of(GRANULE) + GUARD;
        let base = Self::reserve(size);
        assert!(
            !base.is_null(),
            "cannot reserve a task stack of {size} bytes"
        );
        Self { base, size }
    }

    #[cfg(unix)]
    fn reserve(size: usize) -> *mut u8 {
        use sys::{MAP_ANONYMOUS, MAP_NORESERVE, MAP_PRIVATE, PROT_NONE, PROT_READ, PROT_WRITE};
        // SAFETY: an anonymous private mapping at an address of the kernel's choice.
        let p = unsafe {
            sys::mmap(
                std::ptr::null_mut(),
                size,
                PROT_READ | PROT_WRITE,
                MAP_PRIVATE | MAP_ANONYMOUS | MAP_NORESERVE,
                -1,
                0,
            )
        };
        if p as isize == -1 {
            return std::ptr::null_mut();
        }
        // SAFETY: the guard region is the first `GUARD` bytes of the mapping just made.
        if unsafe { sys::mprotect(p, GUARD, PROT_NONE) } != 0 {
            // SAFETY: unmapping the mapping just made.
            unsafe { sys::munmap(p, size) };
            return std::ptr::null_mut();
        }
        p.cast()
    }

    /// On Windows, the stack is reserved and only its top committed; a guard page below
    /// the committed part lets the system grow it, as it grows thread stacks, using the
    /// stack bounds the context switch keeps in the thread information block.
    #[cfg(windows)]
    fn reserve(size: usize) -> *mut u8 {
        use sys::{MEM_COMMIT, MEM_RESERVE, PAGE_GUARD, PAGE_READWRITE};
        // SAFETY: a fresh reservation at an address of the system's choice.
        let p =
            unsafe { sys::VirtualAlloc(std::ptr::null_mut(), size, MEM_RESERVE, PAGE_READWRITE) };
        if p.is_null() {
            return std::ptr::null_mut();
        }
        let base = p.cast::<u8>();
        // SAFETY: both ranges lie inside the reservation just made.
        unsafe {
            let committed = base.add(size - WINDOWS_COMMIT);
            sys::VirtualAlloc(committed.cast(), WINDOWS_COMMIT, MEM_COMMIT, PAGE_READWRITE);
            sys::VirtualAlloc(
                committed.sub(4096).cast(),
                4096,
                MEM_COMMIT,
                PAGE_READWRITE | PAGE_GUARD,
            );
        }
        base
    }

    /// The usable size of a stack made by `Stack::new(usable)`.
    #[must_use]
    pub fn usable_for(usable: usize) -> usize {
        usable.next_multiple_of(GRANULE)
    }

    /// The usable size of this stack.
    #[must_use]
    pub fn usable(&self) -> usize {
        self.size - GUARD
    }

    /// The highest address of the stack (16-byte aligned); stacks grow down from it.
    #[must_use]
    pub fn top(&self) -> *mut u8 {
        // SAFETY: one past the end of the reservation.
        unsafe { self.base.add(self.size) }
    }

    /// The lowest usable address (just above the guard region).
    #[must_use]
    pub fn limit(&self) -> *mut u8 {
        // SAFETY: inside the reservation.
        unsafe { self.base.add(GUARD) }
    }

    /// The lowest address the context may use at first: the committed bottom on Windows
    /// (the system moves it down as the stack grows), the limit elsewhere.
    #[must_use]
    pub fn initial_limit(&self) -> *mut u8 {
        #[cfg(windows)]
        // SAFETY: inside the reservation.
        unsafe {
            self.top().sub(WINDOWS_COMMIT)
        }
        #[cfg(not(windows))]
        self.limit()
    }

    /// The start of the reservation.
    #[must_use]
    pub fn base(&self) -> *mut u8 {
        self.base
    }

    /// Whether `address` lies in this stack's guard region.
    #[must_use]
    pub fn in_guard(&self, address: usize) -> bool {
        let base = self.base as usize;
        (base..base + GUARD).contains(&address)
    }
}

impl Drop for Stack {
    fn drop(&mut self) {
        #[cfg(unix)]
        // SAFETY: the mapping was made by `reserve` with this size and is no longer used.
        unsafe {
            sys::munmap(self.base.cast(), self.size);
        }
        #[cfg(windows)]
        // SAFETY: the allocation was made by `reserve` and is no longer used.
        unsafe {
            sys::VirtualFree(self.base.cast(), 0, sys::MEM_RELEASE);
        }
    }
}
