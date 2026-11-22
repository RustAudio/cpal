use std::panic::{RefUnwindSafe, UnwindSafe};

/// Owns a callback whose invocation and destruction must not unwind into JavaScript.
pub(crate) struct AbortOnPanic<T>(Option<T>);

impl<T> AbortOnPanic<T> {
    pub(crate) fn new(value: T) -> Self {
        Self(Some(value))
    }

    pub(crate) fn with_mut<R>(&mut self, f: impl FnOnce(&mut T) -> R) -> R {
        // Unlike `C-unwind`, this ABI aborts if a panic crosses the boundary.
        extern "C" fn invoke<T, R>(value: &mut Option<T>, f: impl FnOnce(&mut T) -> R) -> R {
            f(value.as_mut().expect("callback is present until drop"))
        }
        invoke(&mut self.0, f)
    }
}

impl<T> Drop for AbortOnPanic<T> {
    fn drop(&mut self) {
        // Captured values may have panicking destructors too.
        extern "C" fn drop_inner<T>(value: &mut Option<T>) {
            drop(value.take());
        }
        drop_inner(&mut self.0);
    }
}

// The inner value is never exposed except through `with_mut`. A panic while
// accessing or dropping it aborts, so no caller can observe it after an unwind.
impl<T> UnwindSafe for AbortOnPanic<T> {}
impl<T> RefUnwindSafe for AbortOnPanic<T> {}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use std::{cell::Cell, panic::AssertUnwindSafe, process::Command, rc::Rc};

    #[test]
    fn invokes_mutable_callback_and_drops_once() {
        struct CountDrop(Rc<Cell<usize>>);
        impl Drop for CountDrop {
            fn drop(&mut self) {
                self.0.set(self.0.get() + 1);
            }
        }
        let drops = Rc::new(Cell::new(0));
        let captured = CountDrop(drops.clone());
        let mut count = 0;
        let mut callback = AbortOnPanic::new(move || {
            let _ = &captured;
            count += 1;
            count
        });
        fn assert_unwind_safe<T: UnwindSafe + RefUnwindSafe>(_: &T) {}
        assert_unwind_safe(&callback);
        assert_eq!(callback.with_mut(|f| f()), 1);
        assert_eq!(callback.with_mut(|f| f()), 2);
        drop(callback);
        assert_eq!(drops.get(), 1);
    }

    fn assert_aborts(test_name: &str, mode: &str, marker: &str) {
        // Test names omit the crate name, both standalone and in integration tests.
        let module = module_path!().split_once("::").unwrap().1;
        let test_name = format!("{module}::{test_name}");
        let output = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", &test_name, "--nocapture"])
            .env("CPAL_ABORTING_TEST", mode)
            .output()
            .unwrap();
        assert!(!output.status.success(), "panic escaped the abort boundary");
        assert!(String::from_utf8_lossy(&output.stderr).contains(marker));
    }

    #[test]
    fn callback_panic_aborts() {
        if std::env::var("CPAL_ABORTING_TEST").as_deref() == Ok("call") {
            let mut callback = AbortOnPanic::new(|| panic!("callback panic marker"));
            let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
                callback.with_mut(|f| f());
            }));
            // If a regression lets the panic escape, this child exits successfully
            // and the parent test fails instead of accepting an ordinary test panic.
            assert!(result.is_err());
            return;
        }
        assert_aborts("callback_panic_aborts", "call", "callback panic marker");
    }

    #[test]
    fn destructor_panic_aborts() {
        if std::env::var("CPAL_ABORTING_TEST").as_deref() == Ok("drop") {
            struct PanicOnDrop;
            impl Drop for PanicOnDrop {
                fn drop(&mut self) {
                    panic!("destructor panic marker");
                }
            }
            let result = std::panic::catch_unwind(|| drop(AbortOnPanic::new(PanicOnDrop)));
            assert!(result.is_err());
            return;
        }
        assert_aborts("destructor_panic_aborts", "drop", "destructor panic marker");
    }
}
