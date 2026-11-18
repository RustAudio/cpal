//! Handles default device monitoring for OSS.
//!
//! The default device is stored in `hw.snd.default_unit` and is an intiger dsp number
//! This needs to exist because `/dev/dsp` is effectively a symlink for the real `dspX` where
//! X is `hw.snd.default_unit`, it does not dynamically auto-reroute in the background on device
//! change. cpal promises dynamic default role devices, thus this adheres to that.
//!
//! Polling would be crude, wastefull, and delayed. As linking can be cumbersome, I instead monitor
//! devd events manually in a single blocking worker thread, shared globally between streams. In the event
//! listening for devd events fails (ran inside a jail, devd not running, socket err, etc), we fall back
//! to polling, meaning default device monitoring is (practically) infallible, with only a slight delay (at
//! worst).
//!
//! The worker thread exists in the background only when there is atleast one default in/out stream active.
//! It does not handle deduplication, as I was originally going to have it do, it is simpler to have each
//! worker thread simply change a running current vs last seen. Duplication can definitely happen, with the
//! monitor, FreeBSD's sysctls count 0 -> 0 a change, and with the polling fallback, it duplicates every 500ms
//!
//! FreeBSD has no concept of default input/output seperate by role, there is simpy a 'default device'.
//!
//! This took lots of work to get working perfectly ;)
use libc::{
    AF_UNIX, SOCK_SEQPACKET, c_int, connect, recv, sa_family_t, sockaddr_un, socket, socklen_t,
    sysctlbyname,
};
use std::{
    ffi::CString,
    mem,
    os::fd::{AsRawFd, FromRawFd, OwnedFd},
    ptr,
    sync::atomic::{AtomicI32, Ordering},
};

// It's possible that DEVD isn't running, or the process is ran from inside a Jail
//
// It's important this follows `assert!(path_bytes.len() < addr.sun_path.len());`,
// which I assumed would always be 108, but in some cases it can be lower (104)
const DEVD_PIPE: &str = "/var/run/devd.seqpacket.pipe";
const DEFAULT_UNIT_SYSCTL: &str = "hw.snd.default_unit";

// `snd_unit` is declared as a standard 32-bit C integer (int) in the kernel
// it is set to -1 before and audio hardware has attached, and if there is no
// default loaded
pub static DEFAULT_UNIT: AtomicI32 = AtomicI32::new(-1);

fn monitor() -> Result<(), ()> {
    unsafe {
        let raw_sock_fd = socket(AF_UNIX, SOCK_SEQPACKET, 0);
        if raw_sock_fd < 0 {
            return Err(());
        }
        // the socket is now valid:
        // Wrap the socket in an `OwnedFd` to avoid leaking the fd on every early return (easier and better practice than manual closing)
        let sockfd = OwnedFd::from_raw_fd(raw_sock_fd);

        let mut addr: sockaddr_un = mem::zeroed();
        addr.sun_family = AF_UNIX as sa_family_t;

        let path_bytes = DEVD_PIPE.as_bytes();
        ptr::copy_nonoverlapping(
            path_bytes.as_ptr(),
            addr.sun_path.as_mut_ptr() as *mut u8,
            path_bytes.len(),
        );

        // The offset of sun_path is 2 bytes (sun_len + sun_family)
        let addr_len = (2 + path_bytes.len()) as socklen_t;
        if connect(
            sockfd.as_raw_fd(),
            &addr as *const sockaddr_un as *const _,
            addr_len,
        ) < 0
        {
            return Err(());
        }

        let mut buf = [0u8; 4096];
        loop {
            let n = recv(sockfd.as_raw_fd(), buf.as_mut_ptr() as *mut _, buf.len(), 0);
            if n <= 0 {
                let err = std::io::Error::last_os_error();
                if err.kind() == std::io::ErrorKind::Interrupted {
                    continue; // try again
                }
                return Err(());
            }

            let msg = String::from_utf8_lossy(&buf[..n as usize]);

            if msg.contains("system=SND")
                && msg.contains("subsystem=CONN")
                && (msg.contains("type=IN") || msg.contains("type=OUT"))
            {
                // Having a look through the FreeBSD source (C), I managed to find that
                // type=IN is set if the device has >0 recording channels and type=OUT
                // if >0 playback channels, whether it is possible that a device is
                // set as default but has 0play and 0rec channels, I don't know, but
                // that is an exotic and bizarre situation anyway

                // Due to the way FreeBSD sysctl works, changing `hw.snd.default_unit`
                // from 0 -> 0 still counts as a change, to avoid needlessly restarting
                // streams (which would be incorrect anyway), we check a running current
                // vs last just to be safe.

                // There is the option to either manually recheck sysctl `hw.snd.default_unit`,
                // or parse the rest of the message, I've decided to parse the rest of the message
                // because I don't see any reason that A) it could fail, and B) checking sysctl is
                // slower and more wasteful.

                // Parsing cdev is impossible to fail:
                // ```c
                // snprintf(buf, sizeof(buf), "cdev=dsp%d", snd_unit);
                // if (d->reccount > 0)
                //     devctl_notify("SND", "CONN", "IN", buf);
                // if (d->playcount > 0)
                //     devctl_notify("SND", "CONN", "OUT", buf);
                // ```
                // From https://lists.freebsd.org/archives/dev-commits-src-branches/2026-March/028282.html, thanks Christos :)
                let Some(snd_unit) = msg
                    .split_whitespace()
                    .find_map(|token| token.strip_prefix("cdev=dsp"))
                    .and_then(|n| n.parse::<i32>().ok())
                else {
                    continue; // silently swallow any errors (see above)
                };

                DEFAULT_UNIT.store(snd_unit, Ordering::Relaxed);
            }
        }
    }
}

// practically infallible, unless sysctl has a meltdown,
// in which any errors are swallowed
fn polling_fallback() {
    let name = CString::new(DEFAULT_UNIT_SYSCTL).unwrap();

    loop {
        // `sysctl hw.snd.default_unit`: gives the human-readable full OID line
        // `sysctl -n hw.snd.default_unti`: gives just the raw intiger (way better for parsing)
        //
        // Instead this manually calls libc's `sysctlbyname` which avoids string parsing, and is
        // the better way to do it
        let mut len = std::mem::size_of::<c_int>() as libc::size_t; // can be modified on error, so we reset it every pass
        let mut unit: c_int = 0;
        let r = unsafe {
            sysctlbyname(
                name.as_ptr(),
                &mut unit as *mut c_int as *mut libc::c_void, // casting...
                &mut len,
                std::ptr::null_mut(),
                0,
            )
        };
        if r == 0 {
            DEFAULT_UNIT.store(unit, Ordering::Relaxed);
        }

        // A reasonable balance between responsiveness and busy-work
        std::thread::sleep(std::time::Duration::from_millis(500));
    }
}
