use native_vtable::{vtable,vtable_impl};
#[vtable]
unsafe trait Base { unsafe fn tick(&self, value:u32); }
struct Object;
#[vtable_impl]
unsafe impl Base for Object { unsafe fn tick(&self, value:f32) {} }
