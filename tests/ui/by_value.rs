use native_vtable::vtable;
#[vtable]
unsafe trait Bad { unsafe fn tick(self); }
