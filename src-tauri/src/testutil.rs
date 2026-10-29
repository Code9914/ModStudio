//! Outils de test : un petit faux serveur HTTP qui renvoie des réponses
//! préparées dans l'ordre et garde une trace des requêtes reçues.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};

use crate::api::ApiConfig;

#[derive(Debug, Clone)]
pub struct SeenRequest {
    pub line: String,
    pub headers: Vec<(String, String)>,
    pub body: String,
}

impl SeenRequest {
    pub fn header(&self, name: &str) -> Option<String> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.clone())
    }
}

pub struct FakeServer {
    pub base: String,
    seen: Arc<Mutex<Vec<SeenRequest>>>,
}

impl FakeServer {
    pub fn request(&self, i: usize) -> SeenRequest {
        self.seen.lock().unwrap()[i].clone()
    }

    pub fn count(&self) -> usize {
        self.seen.lock().unwrap().len()
    }
}

pub fn json_response(status: u16, body: &str) -> String {
    let reason = match status {
        200 => "OK",
        201 => "Created",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        _ => "Status",
    };
    format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
}

pub fn fake_server(responses: Vec<String>) -> FakeServer {
    fake_server_with(|_| responses)
}

/// Comme `fake_server`, mais les réponses peuvent contenir l'adresse du
/// serveur lui-même (liens de téléchargement).
pub fn fake_server_with(build: impl FnOnce(&str) -> Vec<String>) -> FakeServer {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let responses = build(&format!("http://{addr}/v1"));
    let seen = Arc::new(Mutex::new(Vec::new()));
    let seen_thread = seen.clone();
    std::thread::spawn(move || {
        for response in responses {
            let Ok((mut stream, _)) = listener.accept() else { return };
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            let mut headers = Vec::new();
            loop {
                let mut h = String::new();
                reader.read_line(&mut h).unwrap();
                if h == "\r\n" || h.is_empty() {
                    break;
                }
                if let Some((k, v)) = h.trim_end().split_once(':') {
                    headers.push((k.trim().to_string(), v.trim().to_string()));
                }
            }
            let len = headers
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case("content-length"))
                .and_then(|(_, v)| v.parse::<usize>().ok())
                .unwrap_or(0);
            let mut body = vec![0; len];
            reader.read_exact(&mut body).unwrap();
            seen_thread.lock().unwrap().push(SeenRequest {
                line: line.trim().to_string(),
                headers,
                body: String::from_utf8_lossy(&body).into_owned(),
            });
            let bytes: Vec<u8> = response.chars().map(|c| if (c as u32) < 256 { c as u8 } else { b'?' }).collect();
            stream.write_all(&bytes).unwrap();
        }
    });
    FakeServer {
        base: format!("http://{addr}/v1"),
        seen,
    }
}

pub trait ApiConfigExt {
    fn test(base: &str) -> ApiConfig;
}

impl ApiConfigExt for ApiConfig {
    fn test(base: &str) -> ApiConfig {
        ApiConfig {
            api_key: "secretkey1234".into(),
            api_path: base.to_string(),
        }
    }
}
