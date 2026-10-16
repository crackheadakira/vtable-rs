#![no_std]
use native_vtable::{vtable, vtable_impl};
use vtable_test_interface::PublicViewer as ImportedViewer;

#[vtable]
pub unsafe trait Custom: ImportedViewer {
    unsafe fn action(&mut self, value: u32);
}

#[repr(C)]
pub struct Object {
    pub vptr: *const CustomLayout,
    pub ticks: f32,
    pub value: u32,
}

#[vtable_impl]
unsafe impl Custom for Object {
    unsafe fn tick(&mut self, delta: f32) {
        self.ticks += delta;
    }
    unsafe fn action(&mut self, value: u32) {
        self.value = value;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use native_vtable::VTable;
    use vtable_test_interface::engine::{Base, BaseLayout};
    unsafe extern "C" fn context(this: *const ()) -> *const core::ffi::c_void {
        this.cast()
    }
    #[test]
    fn cross_crate_prefix_and_reexport() {
        let base = BaseLayout {
            context: Some(context),
            tick: None,
        };
        let table = Object::vtable_builder().inherit(&base).build().unwrap();
        let mut obj = Object {
            vptr: &table,
            ticks: 0.0,
            value: 0,
        };
        unsafe {
            table.base.base.call_tick(&mut obj, 2.5);
            table.call_action(&mut obj, 42);
            assert!(obj.is_visible());
            assert_eq!(obj.context(), (&obj as *const Object).cast());
        }
        assert_eq!(obj.ticks, 2.5);
        assert_eq!(obj.value, 42);
    }
}
