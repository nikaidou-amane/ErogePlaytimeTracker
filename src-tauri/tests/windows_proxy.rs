#![cfg(windows)]

#[allow(dead_code)]
#[path = "../src/thumbnail.rs"]
mod thumbnail;

use std::{
    io::{BufRead, BufReader, Write},
    net::{SocketAddr, TcpListener},
    path::PathBuf,
    process::Command,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tauri_plugin_updater::UpdaterExt;
use windows::Win32::System::Registry::{HKEY, HKEY_CURRENT_USER, RegOverridePredefKey};

const TEST: &str = "follows_windows_proxy_changes_for_downloads_and_updates";
const CHILD: &str = "EPT_PROXY_TEST_CHILD";

struct TestSettings {
    path: String,
    _root: windows_registry::Key,
    settings: windows_registry::Key,
}

impl TestSettings {
    fn new() -> Self {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = format!("Software\\ErogePlaytimeTrackerTests\\Proxy-{unique}");
        let root = windows_registry::CURRENT_USER.create(&path).unwrap();
        let settings = root
            .create("Software\\Microsoft\\Windows\\CurrentVersion\\Internet Settings")
            .unwrap();
        // The override is process-local. This isolated child never changes the user's proxy.
        unsafe {
            RegOverridePredefKey(HKEY_CURRENT_USER, Some(HKEY(root.as_raw())))
                .ok()
                .unwrap();
        }
        Self {
            path,
            _root: root,
            settings,
        }
    }

    fn set(&self, enabled: bool, address: SocketAddr, bypass: &str) {
        self.settings
            .set_u32("ProxyEnable", u32::from(enabled))
            .unwrap();
        self.settings
            .set_string("ProxyServer", address.to_string())
            .unwrap();
        self.settings.set_string("ProxyOverride", bypass).unwrap();
    }
}

impl Drop for TestSettings {
    fn drop(&mut self) {
        unsafe { RegOverridePredefKey(HKEY_CURRENT_USER, None).ok().unwrap() };
        windows_registry::CURRENT_USER
            .remove_tree(&self.path)
            .unwrap();
    }
}

struct Server {
    address: SocketAddr,
    requests: Arc<Mutex<Vec<String>>>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Server {
    fn new(image: &'static str, origin: Option<SocketAddr>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        listener.set_nonblocking(true).unwrap();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let observed = requests.clone();
        let stopping = stop.clone();
        let thread = thread::spawn(move || {
            while !stopping.load(Ordering::Relaxed) {
                let (stream, _) = match listener.accept() {
                    Ok(connection) => connection,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(5));
                        continue;
                    }
                    Err(error) => panic!("test server accept failed: {error}"),
                };
                // Windows may inherit the listener's nonblocking mode on accepted sockets.
                stream.set_nonblocking(false).unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut reader = BufReader::new(stream);
                let mut first_line = String::new();
                reader.read_line(&mut first_line).unwrap();
                loop {
                    let mut header = String::new();
                    reader.read_line(&mut header).unwrap();
                    if header == "\r\n" || header.is_empty() {
                        break;
                    }
                }
                let body = if first_line.contains("/latest.json") {
                    serde_json::json!({
                        "version": "99.0.0",
                        "url": format!("http://{}/artifact", origin.unwrap_or(address)),
                        "signature": "invalid-test-signature"
                    })
                    .to_string()
                } else {
                    image.to_owned()
                };
                observed.lock().unwrap().push(first_line);
                let reply = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                reader.get_mut().write_all(reply.as_bytes()).unwrap();
            }
        });
        Self {
            address,
            requests,
            stop,
            thread: Some(thread),
        }
    }

    fn count(&self, path: &str) -> usize {
        self.requests
            .lock()
            .unwrap()
            .iter()
            .filter(|request| request.contains(path))
            .count()
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        self.thread.take().unwrap().join().unwrap();
    }
}

#[test]
fn follows_windows_proxy_changes_for_downloads_and_updates() {
    if std::env::var_os(CHILD).is_none() {
        let mut child = Command::new(std::env::current_exe().unwrap());
        child.args(["--exact", TEST, "--nocapture"]).env(CHILD, "1");
        for variable in ["HTTP_PROXY", "HTTPS_PROXY", "ALL_PROXY", "NO_PROXY"] {
            child
                .env_remove(variable)
                .env_remove(variable.to_ascii_lowercase());
        }
        let output = child.output().unwrap();
        assert!(
            output.status.success(),
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }

    let settings = TestSettings::new();
    let direct = Server::new("direct", None);
    let first = Server::new("first-proxy", Some(direct.address));
    let second = Server::new("second-proxy", Some(direct.address));
    let directory: PathBuf =
        std::env::temp_dir().join(format!("ept-system-proxy-{}", std::process::id()));
    std::fs::create_dir(&directory).unwrap();
    let image_url = format!("http://{}/cover.png", direct.address);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        for (enabled, address, bypass, expected) in [
            (true, first.address, "", "first-proxy"),
            (false, first.address, "", "direct"),
            (true, second.address, "", "second-proxy"),
            (true, first.address, "127.0.0.1", "direct"),
        ] {
            settings.set(enabled, address, bypass);
            let path = crate::thumbnail::download(&image_url, &directory)
                .await
                .unwrap();
            assert_eq!(std::fs::read_to_string(path).unwrap(), expected);
        }

        let mut context = tauri::test::mock_context(tauri::test::noop_assets());
        context.config_mut().plugins.0.insert(
            "updater".into(),
            serde_json::json!({
                "pubkey": "invalid-test-public-key",
                "dangerousInsecureTransportProtocol": true,
            }),
        );
        let app = tauri::test::mock_builder()
            .plugin(tauri_plugin_updater::Builder::new().build())
            .build(context)
            .unwrap();
        let updater = app
            .updater_builder()
            .endpoints(vec![
                format!("http://{}/latest.json", direct.address)
                    .parse()
                    .unwrap(),
            ])
            .unwrap()
            .timeout(Duration::from_secs(5))
            .build()
            .unwrap();

        settings.set(true, first.address, "");
        let update = updater.check().await.unwrap().unwrap();
        assert_eq!(first.count("/latest.json"), 1);
        settings.set(true, second.address, "");
        // Only download the fixture: signature validation must reject it, and no installer runs.
        assert!(update.download(|_, _| {}, || {}).await.is_err());
        assert_eq!(second.count("/artifact"), 1);
        settings.set(false, second.address, "");
        assert!(updater.check().await.unwrap().is_some());
        assert_eq!(direct.count("/latest.json"), 1);

        // An unreachable configured proxy must fail instead of retrying directly.
        let closed = TcpListener::bind("127.0.0.1:0").unwrap();
        let closed_address = closed.local_addr().unwrap();
        drop(closed);
        settings.set(true, closed_address, "");
        let direct_images = direct.count("/cover.png");
        assert!(
            crate::thumbnail::download(&image_url, &directory)
                .await
                .is_err()
        );
        assert!(updater.check().await.is_err());
        assert_eq!(direct.count("/cover.png"), direct_images);
        assert_eq!(direct.count("/latest.json"), 1);
    });
    for entry in std::fs::read_dir(&directory).unwrap() {
        std::fs::remove_file(entry.unwrap().path()).unwrap();
    }
    std::fs::remove_dir(directory).unwrap();
}
