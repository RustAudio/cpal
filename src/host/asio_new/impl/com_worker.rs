use std::sync::mpsc::{self, SyncSender};
use std::thread;

use azo::utils::com;
use azo::*;
use windows_core::GUID;

type Request = (GUID, oneshot::Sender<Response>);
type Response = WinResult<Driver>;

#[derive(Debug, Clone)]
pub struct Link(SyncSender<Request>);

impl Link {
    pub fn new() -> Self {
        let (sender, receiver) = mpsc::sync_channel::<Request>(0);

        // This thread will live exactly as long as we need it to, no more and no less.
        // This is because `receiver.recv()` returns an error IFF all senders got dropped,
        // causing the `while` loop to end, and the thread to run out (dropping the COM init
        // guard along the way)
        thread::spawn(move || {
            // inits COM on creation,
            // and uninits it on drop
            let _guard = com::InitGuard
                ::new(COINIT_APARTMENTTHREADED)
                .expect("STA COM init on a fresh thread should be infallible");
            // except for stuff like E_OUTOFMEMORY of course, but that's pretty fatal anyway

            while let Ok((guid, ret)) = receiver.recv() {
                let result = unsafe { Driver::new_unguarded(&guid) };
                _ = ret.send(result); // if the recipient bailed for some reason, just drop and continue
            }
        });

        Self(sender)
    }

    #[expect(clippy::unwrap_in_result, reason = "infallible")]
    pub fn create_driver(&self, guid: GUID) -> Response {
        let (ret_sender, ret_receiver) = oneshot::channel();

        self.0.send((guid, ret_sender)).expect("infallible");
        ret_receiver.recv().expect("infallible")
    }
}
