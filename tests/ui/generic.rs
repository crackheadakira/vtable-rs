use native_vtable::vtable;
#[vtable]
unsafe trait Bad { unsafe fn tick<T>(&self, value:T); }
