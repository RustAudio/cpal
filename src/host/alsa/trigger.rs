use std::sync::Arc;

use super::TRIGGER_PAYLOAD_SIZE;

#[derive(Debug)]
pub(super) struct TriggerSender(pub(super) libc::c_int);

#[derive(Debug)]
pub(super) struct TriggerReceiver(pub(super) libc::c_int);

impl TriggerSender {
    pub(super) fn wakeup(&self) {
        let buf = !0u64; // any non-zero value wakes poll()
        loop {
            let ret = unsafe {
                libc::write(
                    self.0,
                    &buf as *const u64 as *const _,
                    TRIGGER_PAYLOAD_SIZE as _,
                )
            };
            if ret == TRIGGER_PAYLOAD_SIZE {
                return;
            }
            // write() can be interrupted by a signal before writing any bytes; retry.
            assert_eq!(ret, -1, "wakeup: unexpected return value {ret}");
            let err = std::io::Error::last_os_error();
            if err.kind() != std::io::ErrorKind::Interrupted {
                panic!("wakeup: {err}");
            }
        }
    }
}

impl TriggerReceiver {
    pub(super) fn clear_pipe(&self) {
        let mut out = 0u64;
        loop {
            let ret = unsafe {
                libc::read(
                    self.0,
                    &mut out as *mut u64 as *mut _,
                    TRIGGER_PAYLOAD_SIZE as _,
                )
            };
            if ret == TRIGGER_PAYLOAD_SIZE {
                return;
            }
            // read() can be interrupted by a signal before reading any bytes; retry.
            assert_eq!(ret, -1, "clear_pipe: unexpected return value {ret}");
            let err = std::io::Error::last_os_error();
            if err.kind() != std::io::ErrorKind::Interrupted {
                panic!("clear_pipe: {err}");
            }
        }
    }
}

pub(super) fn trigger() -> (TriggerSender, Arc<TriggerReceiver>) {
    let mut fds = [0, 0];
    match unsafe { libc::pipe(fds.as_mut_ptr()) } {
        0 => (TriggerSender(fds[1]), Arc::new(TriggerReceiver(fds[0]))),
        _ => panic!("Could not create pipe"),
    }
}

impl Drop for TriggerSender {
    fn drop(&mut self) {
        unsafe {
            libc::close(self.0);
        }
    }
}

impl Drop for TriggerReceiver {
    fn drop(&mut self) {
        unsafe {
            libc::close(self.0);
        }
    }
}
