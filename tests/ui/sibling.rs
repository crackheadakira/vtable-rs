use native_vtable::{vtable,vtable_impl,VTable};
#[vtable]
unsafe trait Base { unsafe fn tick(&self); }
#[vtable]
unsafe trait Left:Base { unsafe fn left(&self); }
#[vtable]
unsafe trait Right:Base { unsafe fn right(&self); }
struct Object;
#[vtable_impl]
unsafe impl Left for Object {}
fn invalid(table:&RightLayout) {let _=Object::vtable_builder().inherit(table);}
