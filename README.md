# native-vtable

**BE WARNED: AI was used heavily in the creation of this. This was used for a modding project, and should likely not be used elsewhere.**

Traits and ordinary trait implementations for native primary vtables. The
runtime is `no_std`, uses no allocator, and leaves object structs unchanged.

```rust
use native_vtable::{vtable, vtable_impl, VTable};

#[vtable]
pub unsafe trait ScreenVtable {
    unsafe fn update(&mut self, progress: f32);
    unsafe fn is_active(&self) -> bool;
}

#[vtable]
pub unsafe trait ViewerVtable: ScreenVtable {
    unsafe fn setup(&mut self);
}

#[repr(C)]
struct ModMenu {
    vptr: *const ScreenVtableLayout,
    timer: f32,
}

#[vtable_impl]
unsafe impl ViewerVtable for ModMenu {
    unsafe fn update(&mut self, progress: f32) {
        self.timer += progress;
    }

    unsafe fn setup(&mut self) {
        self.timer = 0.0;
    }
}

unsafe extern "C" fn native_is_active(_: *const ()) -> bool { true }

fn main() -> Result<(), native_vtable::MissingSlot> {
    let native = ScreenVtableLayout {
        update: None,
        is_active: Some(native_is_active),
    };
    let replacement = ModMenu::vtable_builder().inherit(&native).build()?;
    let mut menu = ModMenu { vptr: &native, timer: 0.0 };
    unsafe {
        native_vtable::install(&mut menu.vptr, &replacement);
        replacement.base.call_update(&mut menu, 2.0);
        assert!(menu.is_active());
    }
    assert_eq!(menu.timer, 2.0);
    Ok(())
}
```

## Building and installing

Import `VTable` to use `Object::vtable_builder()`. The builder starts empty,
optionally copies a native table or ancestor prefix, then patches your explicit
overrides. A method body supplied in the trait is a fallback for an otherwise
empty slot. Copied native methods take precedence over those fallback bodies.
`build()` returns `MissingSlot` for unresolved declared methods. Generated
forwarders are never installed as callbacks.

Calling `inherit` more than once copies each prefix in order; it does not skip
null entries. Only the same table type or a declared ancestor is accepted.

To call an original implementation from an override, keep the original table
and use `unsafe { original.call_update(self, progress) }`. Calling your own
override again would recurse. See [the runnable menu example](examples/menu.rs).

`install` receives the address of the existing vptr field and a pointer to your
replacement table. The table must remain at a stable address for every native
call. Storage, restoration, allocation and destruction belong to the caller.
The existing vptr field may retain its ancestor table pointer type.

## Slots and raw receivers

Methods occupy pointer-sized slots in declaration order. A derived trait adds
slots after its full parent prefix. For a partially mapped 64-bit native table:

```rust
use native_vtable::vtable;

#[vtable]
pub unsafe trait NativeVtable {
    #[vtable(raw_this)]
    unsafe fn destroy(this: *mut Self, flags: u32);

    #[vtable(offset = 0x18)]
    unsafe fn update(&mut self, progress: f32);
}
```

The interval at `0x08..0x18` becomes an opaque `[usize; 2]` field. It is copied
verbatim when inheriting that table. Uncopied opaque slots remain zero;
`build()` validates declared methods only. Preserve all slots the game can use
by inheriting a sufficiently complete native table. Explicit byte offsets
require a 64-bit target and cannot overlap earlier slots.

Each layout exposes `OFFSET_METHOD_NAME` constants for its own methods and
`VTableLayout::SLOT_COUNT` / `slot(index)` for the entire hierarchy. Opaque slots
have no method metadata. Callable fields are nullable `unsafe extern "C"`
function pointers with a single erased native `this` argument.

Use `raw_this` for callbacks where making a reference is inappropriate, such as
destruction. These remain static trait methods with `*mut Self` or `*const Self`
as the first argument. Direct raw pointers to `Self` are also supported in other
arguments and return values.

## Modules and crates

For nested definitions, provide the canonical path relative to the crate root:

```rust
pub mod ui {
    #[native_vtable::vtable(path = ui::Screen)]
    pub unsafe trait Screen {
        unsafe fn update(&mut self);
    }
}
```

The final path segment must name the trait; do not include `crate::`.
Public traits can be imported, aliased or reexported into another crate and
extended there. A companion macro travels with the trait through Rust's macro
namespace. Signature adapters are generated in the defining module, so imported
argument types retain their meaning. The dependency name is `native-vtable`;
renaming that dependency is not currently supported.

rust-analyzer expands these procedural macros. Enable proc-macro expansion,
import the traits you call, and use its “Expand macro recursively” command to
inspect the actual traits, layouts and adapters. Method signatures and impl
bodies are ordinary Rust. This has compiler coverage; interactive editor
completion has not been manually verified.
