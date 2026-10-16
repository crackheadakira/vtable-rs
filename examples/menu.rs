use native_vtable::{VTable, vtable, vtable_impl};

#[vtable]
pub unsafe trait ScreenVtable {
    unsafe fn update(&mut self, progress: f32);
    unsafe fn is_active(&self) -> bool;
}

#[vtable]
pub unsafe trait ViewerVtable: ScreenVtable {
    unsafe fn setup(&mut self);
}

#[repr(C)]
struct BaseScreen {
    vptr: *const ScreenVtableLayout,
    active: bool,
}

#[repr(C)]
struct ModMenu {
    base: BaseScreen,
    timer: f32,
    original: *const ScreenVtableLayout,
}

#[vtable_impl]
unsafe impl ViewerVtable for ModMenu {
    unsafe fn update(&mut self, progress: f32) {
        self.timer += progress;

        unsafe {
            (*self.original).call_update(self, progress);
        }
    }

    unsafe fn setup(&mut self) {
        self.timer = 0.0;
    }
}

unsafe extern "C" fn native_update(_: *mut (), _: f32) {}
unsafe extern "C" fn native_active(this: *const ()) -> bool {
    unsafe { (*this.cast::<BaseScreen>()).active }
}

fn main() -> Result<(), native_vtable::MissingSlot> {
    let original = ScreenVtableLayout {
        update: Some(native_update),
        is_active: Some(native_active),
    };

    let replacement = ModMenu::vtable_builder().inherit(&original).build()?;

    let mut menu = ModMenu {
        base: BaseScreen {
            vptr: &original,
            active: true,
        },
        timer: 0.0,
        original: &original,
    };

    unsafe {
        native_vtable::install(&mut menu.base.vptr, &replacement);
        replacement.base.call_update(&mut menu, 2.0);

        assert!(menu.is_active());
        assert_eq!(menu.timer, 2.0);

        replacement.call_setup(&mut menu);
    }
    assert_eq!(menu.timer, 0.0);
    Ok(())
}
