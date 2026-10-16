use native_vtable::{vtable,vtable_impl};
#[vtable]
unsafe trait Base { unsafe fn tick(&self); }
struct Object;
#[vtable_impl]
unsafe impl Base for Object { unsafe fn other(&self) {} }
