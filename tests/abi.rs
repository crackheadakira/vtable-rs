use native_vtable::{VTable, vtable, vtable_impl};

#[vtable]
pub unsafe trait Native {
    #[vtable(raw_this)]
    unsafe fn destroy(this: *mut Self, flags: u32);
    #[vtable(offset = 0x18)]
    unsafe fn choose(&self, other: *const Self) -> *const Self;
}

#[repr(C)]
struct Instance {
    table: *const NativeLayout,
    flags: u32,
}

#[vtable_impl]
unsafe impl Native for Instance {
    unsafe fn destroy(this: *mut Self, flags: u32) {
        unsafe {
            (*this).flags = flags;
        }
    }
    unsafe fn choose(&self, other: *const Self) -> *const Self {
        other
    }
}

#[test]
fn raw_this_and_self_pointer_arguments() {
    let table = Instance::vtable_builder().build().unwrap();
    assert_eq!(NativeLayout::OFFSET_CHOOSE, 0x18);
    assert_eq!(core::mem::offset_of!(NativeLayout, choose), 0x18);
    assert_eq!(core::mem::size_of::<NativeLayout>(), 0x20);
    let mut obj = Instance {
        table: &table,
        flags: 0,
    };
    unsafe {
        table.call_destroy(&mut obj, 42);
        assert_eq!(table.call_choose(&obj, &obj), &obj as *const _);
    }
    assert_eq!(obj.flags, 42);
}

#[repr(C)]
struct Forward {
    table: *const NativeLayout,
    flags: u32,
}
#[vtable_impl]
unsafe impl Native for Forward {}

#[test]
fn empty_impl_forwards_without_installing_forwarder_callbacks() {
    let original = Instance::vtable_builder().build().unwrap();
    // These two objects deliberately share the same tested C layout.
    let table = Forward::vtable_builder()
        .inherit(&original)
        .build()
        .unwrap();
    assert_eq!(
        table.destroy.unwrap() as usize,
        original.destroy.unwrap() as usize
    );
    let mut obj = Forward {
        table: &table,
        flags: 0,
    };
    unsafe {
        Forward::destroy(&mut obj, 7);
        assert_eq!(obj.choose(&obj), &obj as *const _);
    }
    assert_eq!(obj.flags, 7);
}

#[test]
fn opaque_slots_are_copied_verbatim() {
    let mut native = Instance::vtable_builder().build().unwrap();
    native.__gap_1 = [123, 456];
    let copy = Forward::vtable_builder().inherit(&native).build().unwrap();
    assert_eq!(copy.__gap_1, [123, 456]);
}
