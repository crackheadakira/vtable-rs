use native_vtable::vtable;
#[vtable]
unsafe trait Bad {
    unsafe fn tick(&self);
    #[vtable(offset=0)]
    unsafe fn tock(&self);
}
