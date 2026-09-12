//! Local HTTP mock: tests never connect to real exchanges.
use std::{collections::HashMap, io::{Read, Write}, net::TcpListener, thread, time::{Duration, Instant}};

pub struct Request {
    pub method: String,
    pub target: String,
    pub headers: HashMap<String, String>,
    pub body: String,
}
pub struct MockServer {
    pub url: String,
    worker: thread::JoinHandle<Vec<Request>>,
}
pub fn client() -> reqwest::Client {
    reqwest::Client::builder().no_proxy().timeout(Duration::from_secs(3)).build().unwrap()
}
impl MockServer {
    pub fn new(responses: Vec<String>) -> Self { Self::spawn(responses, false) }
    pub fn parallel_cancels(count: usize) -> Self {
        Self::spawn(vec![String::new(); count], true)
    }
    fn spawn(responses: Vec<String>, parallel: bool) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let worker = thread::spawn(move || {
            let mut requests = Vec::new();
            let mut pending = Vec::new();
            for response in responses {
                let deadline = Instant::now() + Duration::from_secs(5);
                let mut socket = loop {
                    match listener.accept() {
                        Ok((socket, _)) => break socket,
                        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock && Instant::now() < deadline => {
                            thread::sleep(Duration::from_millis(1));
                        }
                        Err(e) => panic!("mock did not receive expected request: {e}"),
                    }
                };
                socket.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
                let mut bytes = Vec::new();
                let header_end = loop {
                    if let Some(index) = bytes.windows(4).position(|w| w == b"\r\n\r\n") { break index + 4; }
                    let mut buffer = [0u8; 4096];
                    let count = socket.read(&mut buffer).unwrap();
                    assert!(count > 0, "unexpected EOF");
                    bytes.extend_from_slice(&buffer[..count]);
                };
                let header = String::from_utf8(bytes[..header_end].to_vec()).unwrap();
                let mut lines = header.lines();
                let mut first = lines.next().unwrap().split_whitespace();
                let method = first.next().unwrap().to_string();
                let target = first.next().unwrap().to_string();
                let headers: HashMap<String,String> = lines.filter_map(|line| line.split_once(':'))
                    .map(|(key,value)| (key.to_ascii_lowercase(),value.trim().to_string())).collect();
                let length = headers.get("content-length").map(|v| v.parse::<usize>().unwrap()).unwrap_or(0);
                while bytes.len() < header_end + length {
                    let mut buffer = [0u8;4096];
                    let count = socket.read(&mut buffer).unwrap();
                    assert!(count > 0, "unexpected body EOF");
                    bytes.extend_from_slice(&buffer[..count]);
                }
                requests.push(Request { method, target, headers,
                    body: String::from_utf8(bytes[header_end..header_end+length].to_vec()).unwrap() });
                if parallel {
                    let body: serde_json::Value = serde_json::from_str(&requests.last().unwrap().body).unwrap();
                    let response = serde_json::json!({"retCode":0,"result":{"orderId":body["orderId"]}}).to_string();
                    pending.push((socket, response));
                } else {
                    write!(socket, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", response.len(), response).unwrap();
                }
            }
            for (mut socket, response) in pending {
                write!(socket, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", response.len(), response).unwrap();
            }
            requests
        });
        Self { url, worker }
    }
    pub fn finish(self) -> Vec<Request> { self.worker.join().unwrap() }
}
