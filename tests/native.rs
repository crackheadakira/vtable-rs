use core::sync::atomic::{AtomicUsize, Ordering};

use native_vtable::{Unresolved, native};

#[repr(C)]
struct Counter {
    value: u32,
}

unsafe extern "C" fn counter_add(this: *mut Counter, by: u32) -> u32 {
    unsafe {
        (*this).value += by;
        (*this).value
    }
}

unsafe extern "C" fn counter_get(this: *const Counter) -> u32 {
    unsafe { (*this).value }
}

unsafe extern "C" fn twice(x: i32) -> i32 {
    x * 2
}

static LOOKUPS: AtomicUsize = AtomicUsize::new(0);

fn resolve(symbol: &'static str) -> Option<usize> {
    LOOKUPS.fetch_add(1, Ordering::Relaxed);
    match symbol {
        "Counter::add" => Some(counter_add as *const () as usize),
        "Counter::get" => Some(counter_get as *const () as usize),
        "twice" => Some(twice as *const () as usize),
        _ => None,
    }
}

#[native(resolve = resolve)]
impl Counter {
    #[native(symbol = "Counter::add")]
    unsafe fn add(&mut self, by: u32) -> u32;

    #[native(symbol = "Counter::get")]
    unsafe fn get(&self) -> u32;

    #[native(symbol = "Counter::missing")]
    unsafe fn missing(&mut self);

    fn plain(&self) -> u32 {
        self.value + 1
    }
}

#[native(resolve = resolve)]
unsafe extern "C" {
    #[native(symbol = "twice")]
    fn native_twice(x: i32) -> i32;
}

#[test]
fn receivers_are_passed_as_this() {
    let mut c = Counter { value: 1 };
    unsafe {
        assert_eq!(c.add(4), Ok(5));
        assert_eq!(c.get(), Ok(5));
    }
    assert_eq!(c.plain(), 6);
}

#[test]
fn free_functions_resolve() {
    assert_eq!(unsafe { native_twice(21) }, Ok(42));
}

#[test]
fn missing_symbol_is_an_error() {
    let mut c = Counter { value: 0 };
    assert_eq!(
        unsafe { c.missing() },
        Err(Unresolved {
            symbol: "Counter::missing"
        })
    );
}

#[test]
fn address_is_cached_after_first_call() {
    let mut c = Counter { value: 0 };
    unsafe {
        let _ = c.add(1);
        let before = LOOKUPS.load(Ordering::Relaxed);
        let _ = c.add(1);
        let _ = c.add(1);
        assert_eq!(LOOKUPS.load(Ordering::Relaxed), before);
    }
    assert_eq!(c.value, 3);
}
