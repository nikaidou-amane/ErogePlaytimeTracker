use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::mpsc;

/// Proves that a client built with `reqwest::Proxy::all` routes through the
/// configured proxy instead of connecting to the target host directly. Without
/// this, an unusable proxy could be saved and still appear to work.
#[test]
fn proxied_client_sends_the_request_to_the_proxy() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let (sender, receiver) = mpsc::channel();
    let handle = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut buffer = [0u8; 4096];
        let read = stream.read(&mut buffer).unwrap_or(0);
        let _ = sender.send(String::from_utf8_lossy(&buffer[..read]).to_string());
        let _ = stream.write_all(b"HTTP/1.1 502 Bad Gateway\r\nContent-Length: 0\r\n\r\n");
    });

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let request = runtime.block_on(async {
        // The client must be built inside the runtime it will use.
        let client = reqwest::Client::builder()
            .proxy(reqwest::Proxy::all(format!("http://127.0.0.1:{port}")).unwrap())
            .build()
            .unwrap();
        let _ = client
            .get("http://example.test/thumbnail.png")
            .timeout(std::time::Duration::from_secs(10))
            .send()
            .await;
        receiver.recv().unwrap()
    });

    handle.join().unwrap();
    assert!(
        request.starts_with("GET http://example.test/thumbnail.png"),
        "the proxy received an absolute-form request: {request}"
    );
}
