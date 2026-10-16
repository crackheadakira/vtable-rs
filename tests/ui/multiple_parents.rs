use native_vtable::vtable;
#[vtable]
unsafe trait Bad: First + Second { unsafe fn tick(&self); }
