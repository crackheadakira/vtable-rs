#![no_std]
#![doc = include_str!("../README.md")]

pub use native_vtable_macros::{__expand, native, vtable, vtable_impl};

use core::sync::atomic::{AtomicUsize, Ordering};

/// Maps a native symbol name to its absolute address. Supplied by the caller of
/// [`native`], so this crate stays independent of how addresses are found.
pub type Resolver = fn(&'static str) -> Option<usize>;

/// A native function whose symbol the resolver could not supply.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Unresolved {
    pub symbol: &'static str,
}

impl core::fmt::Display for Unresolved {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "unresolved native symbol: {}", self.symbol)
    }
}

impl core::error::Error for Unresolved {}

/// The cached address of one [`native`] function. Resolved on first use; a
/// failed lookup is retried on the next call.
#[doc(hidden)]
pub struct NativeAddr(AtomicUsize);

impl NativeAddr {
    pub const fn new() -> Self {
        Self(AtomicUsize::new(0))
    }

    #[inline]
    pub fn get(&self, symbol: &'static str, resolve: Resolver) -> Result<usize, Unresolved> {
        let cached = self.0.load(Ordering::Relaxed);
        if cached != 0 {
            return Ok(cached);
        }

        match resolve(symbol) {
            Some(addr) if addr != 0 => {
                self.0.store(addr, Ordering::Relaxed);
                Ok(addr)
            }
            _ => Err(Unresolved { symbol }),
        }
    }
}

impl Default for NativeAddr {
    fn default() -> Self {
        Self::new()
    }
}

/// A native primary table layout.
///
/// # Safety
/// Implementors must have a C layout containing pointer-sized slots, accept zero
/// bits in every field, and report all unresolved callable slots from `missing`.
pub unsafe trait VTableLayout: Copy {
    const SLOT_COUNT: usize;
    fn slot(index: usize) -> Option<Slot>;
    fn missing(&self) -> Option<&'static str>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Slot {
    pub name: &'static str,
    pub index: usize,
}

/// Associates an object with its primary table.
///
/// # Safety
/// Objects must have their primary vptr at offset zero and match their declared
/// native layout. Implementations must build callbacks for this concrete type.
pub unsafe trait VTable: Sized {
    type Table: VTableLayout;
    fn vtable_builder() -> Builder<Self::Table>;
}

/// `Self` is an ABI-compatible prefix of `T`.
///
/// # Safety
/// `Self` must match the leading bytes of `T` in size, alignment and slot types.
pub unsafe trait PrefixOf<T: VTableLayout>: VTableLayout {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MissingSlot(pub &'static str);

impl core::fmt::Display for MissingSlot {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "unresolved virtual method: {}", self.0)
    }
}

impl core::error::Error for MissingSlot {}

pub struct Builder<T: VTableLayout> {
    table: T,
    patch: fn(&mut T),
}

impl<T: VTableLayout> Builder<T> {
    #[doc(hidden)]
    pub fn new(patch: fn(&mut T)) -> Self {
        Self {
            table: unsafe { core::mem::zeroed() },
            patch,
        }
    }

    /// Copy a native table (or ancestor prefix) before applying Rust overrides.
    pub fn inherit<P: PrefixOf<T>>(mut self, parent: &P) -> Self {
        unsafe {
            core::ptr::copy_nonoverlapping(
                (parent as *const P).cast::<u8>(),
                (&mut self.table as *mut T).cast::<u8>(),
                core::mem::size_of::<P>(),
            );
        }
        self
    }

    pub fn build(mut self) -> Result<T, MissingSlot> {
        (self.patch)(&mut self.table);
        match self.table.missing() {
            Some(name) => Err(MissingSlot(name)),
            None => Ok(self.table),
        }
    }
}

/// Replace an existing primary vptr. The caller owns the object's and table's
/// lifetimes and must ensure the table matches the object's native ABI.
///
/// # Safety
/// `slot` must be writable, and `table` must remain live at a stable address for
/// every dispatch through it. Its callbacks must accept the concrete object.
/// The caller must also exclude concurrent reads or writes of the vptr.
pub unsafe fn install<P: PrefixOf<T>, T: VTableLayout>(slot: *mut *const P, table: *const T) {
    unsafe { slot.write(table.cast()) };
}
