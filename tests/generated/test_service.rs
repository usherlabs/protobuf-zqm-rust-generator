use std::sync::{Arc, Mutex};
use std::thread::sleep;
use futures::future::BoxFuture;
use futures::{FutureExt, TryFutureExt};
use prost::Message;
use tokio::task;
use zmq::SocketType;
fn create_socket(path: &str, socket_type: SocketType) -> zmq::Socket {
    let context = zmq::Context::new();
    let socket = context.socket(socket_type).unwrap();
    let protocol = "ipc://";
    create_path_if_not_exists(path);
    let endpoint = format!("{}{}", protocol, path);
    socket.bind(&endpoint).unwrap();
    socket
}
fn create_path_if_not_exists(path_str: &str) {
    let path = std::path::Path::new(path_str);
    let path1 = path.parent().unwrap();
    if !path1.exists() {
        std::fs::create_dir_all(path1).unwrap();
    }
}
/// ====== SUBSCRIPTION DEFINITIONS ======
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct EmptyInput {}
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct SubscriptionItem {
    #[prost(string, tag = "1")]
    pub data: ::prost::alloc::string::String,
}
/// ====== REQUEST DEFINITIONS ======
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct MyRequestInput {
    #[prost(int32, tag = "1")]
    pub time_to_sleep: i32,
}
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct MyRequestResult {
    #[prost(bool, tag = "1")]
    pub all_ok: bool,
    #[prost(string, tag = "2")]
    pub message: ::prost::alloc::string::String,
}
pub struct MyServerServiceServer {
    pub_socket: zmq::Socket,
    rep_socket: Arc<Mutex<zmq::Socket>>,
    reply_handlers: Arc<dyn MyServerServiceHandlers + Send + Sync>,
}
impl MyServerServiceServer {
    pub fn new(
        pubsub_path: String,
        reply_path: String,
        reply_handlers: Arc<dyn MyServerServiceHandlers + Send + Sync>,
    ) -> Self {
        let pub_socket = create_socket(&pubsub_path, zmq::PUB);
        let rep_socket = create_socket(&reply_path, zmq::ROUTER);
        Self {
            pub_socket,
            rep_socket: Arc::new(Mutex::new(rep_socket)),
            reply_handlers,
        }
    }
    /// Starts listening for requests
    pub fn start_listening(&self) {
        loop {
            let rep_socket = self.rep_socket.lock().unwrap();
            let poll_result = rep_socket.poll(zmq::POLLIN, 0);
            drop(rep_socket);
            if poll_result.is_err() {
                continue;
            }
            if (poll_result.unwrap()) == 0 {
                sleep(std::time::Duration::from_millis(50));
                continue;
            }
            let message = match self.rep_socket.lock().unwrap().recv_multipart(0) {
                Ok(msg) => msg,
                Err(_) => {
                    continue;
                }
            };
            if message.len() < 4 {
                continue;
            }
            let identity = message[0].clone();
            let request_id = message[2].clone();
            let method_name_raw = message[3].clone();
            let input = message[4].clone();
            let method_name = String::from_utf8_lossy(&method_name_raw).to_string();
            let handlers = self.reply_handlers.clone();
            let rep_socket = self.rep_socket.clone();
            task::spawn(async move {
                let mut response = Vec::new();
                response.push(identity);
                response.push(request_id);
                if handlers.has_handler(&method_name) {
                    let result = handlers.call_handler(&method_name, &input).await;
                    match result {
                        Ok(validation_result) => {
                            response.push(validation_result);
                        }
                        Err(e) => {
                            response.push(e.encode_to_vec());
                        }
                    }
                } else {
                    let not_found_error_msg = "Method not found";
                    response.push(not_found_error_msg.as_bytes().to_vec());
                }
                rep_socket.lock().unwrap().send_multipart(response, 0).unwrap();
            });
        }
    }
    fn publish_message<T: prost::Message>(
        &mut self,
        name: &str,
        data: T,
    ) -> zmq::Result<()> {
        let message = data.encode_to_vec();
        let messages = vec![name.as_bytes(), & message];
        self.pub_socket.send_multipart(messages, 0)
    }
    pub fn publish_to_items(&mut self, data: SubscriptionItem) -> zmq::Result<()> {
        self.publish_message("SubscribeToItems", data)
    }
}
pub trait MyServerServiceHandlers {
    fn has_handler(&self, method_name: &str) -> bool {
        match method_name {
            "MyRequestMethod" => true,
            _ => false,
        }
    }
    fn call_handler(
        &self,
        method_name: &str,
        encoded_input: &[u8],
    ) -> BoxFuture<Result<Vec<u8>, ()>> {
        match method_name {
            "MyRequestMethod" => {
                let input = MyRequestInput::decode(encoded_input).unwrap();
                self.my_request_method(input)
                    .map_ok(|result| { result.encode_to_vec() })
                    .boxed()
            }
            _ => async { Err(()) }.boxed(),
        }
    }
    fn my_request_method(
        &self,
        _input: MyRequestInput,
    ) -> BoxFuture<Result<MyRequestResult, ()>> {
        unimplemented!("Validate")
    }
}
