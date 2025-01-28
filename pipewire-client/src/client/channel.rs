use ControlFlow::Break;
use crate::error::Error;
use crossbeam_channel::{unbounded, SendError, TryRecvError};
use std::collections::HashMap;
use std::fmt::Debug;
use std::ops::ControlFlow;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::runtime::Runtime;
use tokio::select;
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

pub(crate) struct Request<T> {
    pub id: Uuid,
    pub message: T,
}

impl <T> Request<T> {
    pub(super) fn new(message: T) -> Self {
        Self {
            id: Uuid::new_v4(),
            message,
        }
    }
}

impl <T> From<T> for Request<T> {
    fn from(value: T) -> Self {
        Self::new(value)
    }
}

pub(crate) struct Response<T> {
    pub id: Uuid,
    pub message: T,
}

impl <T> Response<T> {
    pub fn from<Q>(request: &Request<Q>, message: T) -> Self {
        Self {
            id: request.id.clone(),
            message,
        }
    }
}

type PendingMessages<R> = Arc<Mutex<HashMap<Uuid, Response<R>>>>;
type GlobalMessages<R> = Arc<Mutex<Vec<Response<R>>>>;

const GLOBAL_MESSAGE_ID: Uuid = Uuid::nil();

pub(crate) struct ClientChannel<Q, R> {
    sender: pipewire::channel::Sender<Request<Q>>,
    receiver: crossbeam_channel::Receiver<Response<R>>,
    pub(super) global_messages: GlobalMessages<R>,
    pub(super) pending_messages: PendingMessages<R>,
    runtime: Arc<Runtime>
}

impl <Q: Debug + Send + 'static, R: Send + 'static> ClientChannel<Q, R> {
    pub(self) fn new(
        sender: pipewire::channel::Sender<Request<Q>>,
        receiver: crossbeam_channel::Receiver<Response<R>>,
        runtime: Arc<Runtime>
    ) -> Self {
        Self {
            sender,
            receiver,
            global_messages: Arc::new(Mutex::new(Vec::new())),
            pending_messages: Arc::new(Mutex::new(HashMap::new())),
            runtime,
        }
    }

    pub fn fire(&self, request: Q) -> Result<Uuid, Error> {
        let id = Uuid::new_v4();
        let request = Request {
            id: id.clone(),
            message: request,
        };
        let response = self.sender.send(request);
        match response {
            Ok(_) => Ok(id),
            Err(value) => Err(Error {
                description: format!("Failed to send request: {:?}", value.message),
            }),
        }
    }

    pub fn send(&self, request: Q) -> Result<R, Error> {
        let id = Uuid::new_v4();
        let request = Request {
            id: id.clone(),
            message: request,
        };
        let response = self.sender.send(request);
        let response = match response {
            Ok(_) => self.receive(
                id,
                self.global_messages.clone(),
                self.pending_messages.clone(),
                self.receiver.clone(),
                CancellationToken::default()
            ),
            Err(value) => return Err(Error {
                description: format!("Failed to send request: {:?}", value.message),
            }),
        };
        match response {
            Ok(value) => Ok(value.message),
            Err(value) => Err(Error {
                description: format!(
                    "Failed to execute request ({:?})", value
                ),
            }),
        }
    }

    pub fn send_timeout(&self, request: Q, timeout: Duration) -> Result<R, Error> {
        let request_id = match self.fire(request) {
            Ok(value) => value,
            Err(value) => return Err(value)
        };
        self.internal_receive_timeout(request_id, timeout)
    }
    
    pub fn receive_timeout(&self, timeout: Duration) -> Result<R, Error> {
        self.internal_receive_timeout(GLOBAL_MESSAGE_ID, timeout)
    }

    fn internal_receive_timeout(&self, id: Uuid, timeout: Duration) -> Result<R, Error> {
        let global_messages = self.global_messages.clone();
        let pending_messages = self.pending_messages.clone();
        let receiver = self.receiver.clone();
        let handle = self.runtime.spawn(async move {
            let start_time = Instant::now();
            loop {
                let control_flow = Self::internal_receive(
                    id,
                    global_messages.clone(),
                    pending_messages.clone(),
                    receiver.clone()
                );
                match control_flow.await {
                    Break(value) => {
                        return match value {
                            Ok(value   ) => Ok(value.message),
                            Err(value) => Err(value)
                        };
                    },
                    _ => {
                        let now_time = Instant::now();
                        let delta_time = now_time - start_time;
                        if delta_time >= timeout {
                            return Err(Error {
                                description: "Timeout".to_string(),
                            });
                        }
                        continue
                    },
                }
            }
        });
        self.runtime.block_on(handle).unwrap()
    }

    fn receive(
        &self,
        id: Uuid,
        global_messages: GlobalMessages<R>,
        pending_messages: PendingMessages<R>,
        receiver: crossbeam_channel::Receiver<Response<R>>,
        cancellation_token: CancellationToken
    ) -> Result<Response<R>, Error> {
        let handle = self.runtime.spawn(async move {
            loop {
                select! {
                    _ = cancellation_token.cancelled() => (),
                    control_flow = Self::internal_receive(
                        id, 
                        global_messages.clone(), 
                        pending_messages.clone(), 
                        receiver.clone()
                    ) => {
                        match control_flow {
                            Break(value) => return value,
                            _ => continue,
                        }
                    }
                }
            }
        });
        self.runtime.block_on(handle).unwrap()
    }
    
    async fn internal_receive(
        id: Uuid,
        global_messages: GlobalMessages<R>,
        pending_messages: PendingMessages<R>,
        receiver: crossbeam_channel::Receiver<Response<R>>,
    ) -> ControlFlow<Result<Response<R>, Error>, ()>
    {
        let response = receiver.try_recv();
        match response {
            Ok(value) => {
                // When request id is equal Uuid::nil, 
                // message is sent when unrecoverable error occurred outside of request context.
                // But it's not necessary an error message, initialized message is global too.
                //
                // Those errors are sent in event handler, registry 
                // and during server thread init phase.
                // Might a good idea to find a better solution, because any request could fail
                // but not because request is malformed or request result cannot be computed, but
                // because somewhere else something bad happen.
                //
                // Maybe putting error messages into a vec which will be regularly watched by an async
                // periodic task ? But that involve to create a thread/task in tokio and that task will  
                // live during client lifetime (maybe for a long time).
                //
                // Maybe add a separate channel but same issue occur here. An async periodic task will
                // watch if any error message spawned
                // 
                // For now, solution is simple:
                //   1.1: Requested id is equal to response id
                //        in that case we break the loop because that's the requested id
                //   1.2: Requested id is not equal to response id but to global id
                //        we store that response to further process
                if value.id == id {
                    Break(Ok(value))
                }
                else if value.id == GLOBAL_MESSAGE_ID {
                    global_messages.lock().unwrap().push(value);
                    ControlFlow::Continue(())
                }
                else {
                    pending_messages.lock().unwrap().insert(value.id.clone(), value);
                    ControlFlow::Continue(())
                }
            }
            Err(value) => {
                match value {
                    TryRecvError::Empty => {
                        match pending_messages.lock().unwrap().remove(&id) {
                            Some(value) => Break(Ok(value)),
                            None => ControlFlow::Continue(()),
                        }
                    }
                    TryRecvError::Disconnected => {
                        Break(Err(Error {
                            description: "Channel disconnected".to_string(),
                        }))
                    }
                }
            }
        }
    }
}

impl <Q, R> Clone for ClientChannel<Q, R> {
    fn clone(&self) -> Self {
        Self {
            sender: self.sender.clone(),
            receiver: self.receiver.clone(),
            global_messages: self.global_messages.clone(),
            pending_messages: self.pending_messages.clone(),
            runtime: self.runtime.clone(),
        }
    }
}

pub(crate) struct ServerChannel<Q: 'static, R> {
    sender: crossbeam_channel::Sender<Response<R>>,
    receiver: Option<pipewire::channel::Receiver<Request<Q>>>
}

impl <Q, R> ServerChannel<Q, R> {
    pub(self) fn new(
        sender: crossbeam_channel::Sender<Response<R>>,
        receiver: pipewire::channel::Receiver<Request<Q>>,
    ) -> Self {
        Self {
            sender,
            receiver: Some(receiver),
        }
    }

    pub fn attach<'a, F>(&mut self, loop_: &'a pipewire::loop_::LoopRef, callback: F) -> pipewire::channel::AttachedReceiver<'a, Request<Q>>
    where
        F: Fn(Request<Q>) + 'static,
    {
        let receiver = self.receiver.take().unwrap();
        let attached_receiver = receiver.attach(loop_, callback);
        attached_receiver
    }
    
    pub fn fire(&self, response: R) -> Result<(), SendError<Response<R>>> {
        let response = Response {
            id: GLOBAL_MESSAGE_ID,
            message: response,
        };
        self.sender.send(response)
    }

    pub fn send(&self, request: &Request<Q>, response: R) -> Result<(), SendError<Response<R>>> {
        let response = Response::from(request, response);
        self.sender.send(response)
    }
}

impl <Q, R> Clone for ServerChannel<Q, R> {
    fn clone(&self) -> Self {
        Self {
            sender: self.sender.clone(),
            receiver: None // pipewire receiver cannot be cloned
        }
    }
}

pub(crate) fn channels<Q, R>(runtime: Arc<Runtime>) -> (ClientChannel<Q, R>, ServerChannel<Q, R>) 
where 
    Q: Debug + Send, 
    R: Send + 'static
{
    let (pw_sender, pw_receiver) = pipewire::channel::channel();
    let (main_sender, main_receiver) = unbounded();
    let client_channel = ClientChannel::<Q, R>::new(
        pw_sender,
        main_receiver,
        runtime
    );
    let server_channel = ServerChannel::<Q, R>::new(
        main_sender,
        pw_receiver
    );
    (client_channel, server_channel)
}