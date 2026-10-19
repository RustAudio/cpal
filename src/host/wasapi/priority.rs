//! Worker-local ownership of a successful scheduling registration.

pub(super) struct Guard<H, F: FnMut(H)> {
    handle: Option<H>,
    release: F,
}

impl<H, F: FnMut(H)> Guard<H, F> {
    pub(super) fn acquire<E>(
        promote: impl FnOnce() -> Result<H, E>,
        release: F,
    ) -> Result<Self, E> {
        Ok(Self {
            handle: Some(promote()?),
            release,
        })
    }
}

impl<H, F: FnMut(H)> Drop for Guard<H, F> {
    fn drop(&mut self) {
        if let Some(handle) = self.handle.take() {
            (self.release)(handle);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Guard;
    use std::{cell::Cell, rc::Rc, thread};

    #[test]
    fn successful_registration_spans_the_body_and_releases_on_the_same_thread() {
        let worker = thread::spawn(|| {
            let active = Rc::new(Cell::new(false));
            let releases = Cell::new(0);
            let owner = thread::current().id();
            {
                // Rc deliberately makes this handle non-Send, like the Windows handle.
                let _guard = Guard::acquire(
                    || -> Result<_, ()> {
                        active.set(true);
                        Ok(Rc::clone(&active))
                    },
                    |handle: Rc<Cell<bool>>| {
                        assert_eq!(thread::current().id(), owner);
                        assert!(handle.replace(false));
                        releases.set(releases.get() + 1);
                    },
                )
                .unwrap();
                assert!(active.get(), "promotion precedes the first body call");
                for _ in 0..32 {
                    assert!(active.get());
                    assert_eq!(releases.get(), 0);
                }
            }
            assert!(!active.get());
            assert_eq!(releases.get(), 1);
        });
        worker.join().unwrap();
    }

    #[test]
    fn failed_registration_never_releases_a_handle() {
        let releases = Cell::new(0);
        let result = Guard::acquire(
            || Err::<usize, _>("injected failure"),
            |_| releases.set(releases.get() + 1),
        );
        assert!(result.is_err());
        drop(result);
        assert_eq!(releases.get(), 0);
    }

    #[test]
    fn early_error_and_unwind_release_once() {
        for panic in [false, true] {
            let releases = Cell::new(0);
            let owner = thread::current().id();
            let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let _guard = Guard::acquire(
                    || Ok::<_, ()>(owner),
                    |registered_on| {
                        assert_eq!(registered_on, thread::current().id());
                        releases.set(releases.get() + 1);
                    },
                )
                .unwrap();
                if panic {
                    panic!("injected worker failure");
                }
                Err::<(), _>("injected device error")
            }));
            assert_eq!(outcome.is_err(), panic);
            assert_eq!(releases.get(), 1);
        }
    }
}
