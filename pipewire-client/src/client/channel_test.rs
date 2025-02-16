use crate::client::channel::{channels, Request};
use rstest::rstest;
use std::thread;
use std::time::Duration;
use pipewire_test_utils::environment::TEST_ENVIRONMENT;

#[derive(Debug, Clone, Copy)]
enum MessageRequest {
    Quit,
    Test1,
    Test2
}

#[derive(Debug, PartialEq)]
enum MessageResponse {
    Test1,
    Test2,
    GlobalMessage
}

#[rstest]
fn request_context() {
    let (client_channel, mut server_channel) = channels(TEST_ENVIRONMENT.lock().unwrap().runtime.clone());
    let handle_main = thread::spawn(move || {
        let sender = server_channel.clone();
        let main_loop = pipewire::main_loop::MainLoop::new(None).unwrap();
        let attached_main_loop = main_loop.clone();
        let _attached_channel = server_channel.attach(
            main_loop.loop_(),
            move |message| {
                let request = message.message;
                match request {
                    MessageRequest::Test1 => {
                        sender
                            .send(&message, MessageResponse::Test1)
                            .unwrap()
                    }
                    MessageRequest::Test2 => {
                        sender
                            .send(&message, MessageResponse::Test2)
                            .unwrap()
                    }
                    _ => attached_main_loop.quit()
                }
            }
        );
        main_loop.run();
    });
    let request_1 = MessageRequest::Test1;
    let request_2 = MessageRequest::Test2;
    let response = client_channel.send(request_1).unwrap();
    assert_eq!(MessageResponse::Test1, response);
    let response = client_channel.send(request_2).unwrap();
    assert_eq!(MessageResponse::Test2, response);
    client_channel.fire(MessageRequest::Quit).unwrap();
    assert_eq!(0, client_channel.global_messages.lock().unwrap().len());
    assert_eq!(0, client_channel.pending_messages.lock().unwrap().len());
    handle_main.join().unwrap();
}

#[rstest]
fn global_message() {
    let (client_channel, server_channel) = channels::<MessageRequest, MessageResponse>(TEST_ENVIRONMENT.lock().unwrap().runtime.clone());
    server_channel.fire(MessageResponse::GlobalMessage).unwrap();
    client_channel
        .send_timeout(
            MessageRequest::Test2,
            Duration::from_millis(200),
        )
        .unwrap_err(); 
    assert_eq!(1, client_channel.global_messages.lock().unwrap().len());
    assert_eq!(0, client_channel.pending_messages.lock().unwrap().len());
}

#[rstest]
fn pending_message() {
    let (client_channel, server_channel) = channels::<MessageRequest, MessageResponse>(TEST_ENVIRONMENT.lock().unwrap().runtime.clone());
    let request = Request::new(MessageRequest::Test1);
    server_channel.send(&request ,MessageResponse::Test1).unwrap();
    client_channel
        .send_timeout(
            MessageRequest::Test2,
            Duration::from_millis(200),
        )
        .unwrap_err();
    assert_eq!(0, client_channel.global_messages.lock().unwrap().len());
    assert_eq!(1, client_channel.pending_messages.lock().unwrap().len());
}
