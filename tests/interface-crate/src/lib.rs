#![no_std]
pub mod engine {
    use core::ffi::c_void as Context;
    use native_vtable::vtable;

    #[vtable(path = engine::Base)]
    pub unsafe trait Base {
        unsafe fn context(&self) -> *const Context;
        unsafe fn tick(&mut self, delta: f32);
    }

    #[vtable(path = engine::Viewer)]
    pub unsafe trait Viewer: Base {
        unsafe fn is_visible(&self) -> bool {
            true
        }
    }
}
pub use engine::Viewer as PublicViewer;
