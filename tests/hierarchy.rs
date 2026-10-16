use native_vtable::{VTable, vtable, vtable_impl};

#[vtable]
pub unsafe trait Root {
    unsafe fn update(&mut self, value: i32);
    unsafe fn value(&self) -> i32;
}

#[vtable]
pub unsafe trait Viewer: Root {
    unsafe fn enabled(&self) -> bool {
        true
    }
}

#[vtable]
pub unsafe trait Menu: Viewer {
    unsafe fn scale(&mut self, value: f32) -> f32;
}

#[repr(C)]
struct Object {
    vptr: *const RootLayout,
    value: i32,
}

#[vtable_impl]
unsafe impl Menu for Object {
    unsafe fn update(&mut self, value: i32) {
        self.value = value;
    }
    unsafe fn scale(&mut self, value: f32) -> f32 {
        self.value as f32 * value
    }
}

unsafe extern "C" fn native_value(this: *const ()) -> i32 {
    unsafe { (*this.cast::<Object>()).value }
}

#[test]
fn hierarchy_callbacks_and_native_forwarding() {
    let base = RootLayout {
        update: None,
        value: Some(native_value),
    };
    let table = Object::vtable_builder().inherit(&base).build().unwrap();
    assert_eq!(
        core::mem::size_of::<MenuLayout>(),
        4 * core::mem::size_of::<usize>()
    );
    assert_eq!(core::mem::offset_of!(MenuLayout, base), 0);
    let mut obj = Object {
        vptr: core::ptr::null(),
        value: 0,
    };
    unsafe {
        native_vtable::install(&mut obj.vptr, &table);
        table.base.base.call_update(&mut obj, 21);
        assert_eq!(obj.value(), 21);
        assert_eq!(table.call_scale(&mut obj, 2.0), 42.0);
        assert!(table.base.call_enabled(&obj));
    }
}

#[test]
fn missing_slot_is_reported() {
    assert_eq!(
        Object::vtable_builder().build().err().unwrap().0,
        "Root::value"
    );
}

unsafe extern "C" fn native_disabled(_: *const ()) -> bool {
    false
}

#[test]
fn native_implementation_takes_precedence_over_rust_fallback() {
    let native = ViewerLayout {
        base: RootLayout {
            update: None,
            value: Some(native_value),
        },
        enabled: Some(native_disabled),
    };
    let table = Object::vtable_builder().inherit(&native).build().unwrap();
    let obj = Object {
        vptr: &table.base.base,
        value: 0,
    };
    unsafe {
        assert!(!obj.enabled());
    }
}

#[test]
fn metadata_includes_inherited_slots() {
    use native_vtable::VTableLayout;
    assert_eq!(MenuLayout::SLOT_COUNT, 4);
    assert_eq!(MenuLayout::slot(0).unwrap().name, "Root::update");
    assert_eq!(MenuLayout::slot(3).unwrap().name, "Menu::scale");
    assert!(MenuLayout::slot(4).is_none());
}
