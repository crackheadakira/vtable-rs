use native_vtable::vtable;
#[vtable]
unsafe trait Bad {type Output; unsafe fn tick(&self)->Self::Output;}
