use super::*;
use std::{cell::RefCell, net::TcpListener, thread};

fn server(response: &'static [u8]) -> (String, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let worker = thread::spawn(move || {
        let (mut socket, _) = listener.accept().unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        // TCP can split even a small request across reads. Closing with
        // unread headers can reset the connection before the response arrives.
        let mut request = Vec::new();
        let mut chunk = [0; 1024];
        while !request.windows(4).any(|w| w == b"\r\n\r\n") {
            let n = socket.read(&mut chunk).unwrap();
            assert!(n > 0 && request.len() < 16_384);
            request.extend_from_slice(&chunk[..n]);
        }
        socket.write_all(response).unwrap();
    });
    (url, worker)
}

#[test]
fn download_reports_unknown_size_and_rejects_interruption_and_http_errors() {
    for (response, success) in [
        (
            &b"HTTP/1.1 200 OK\r\nConnection: close\r\n\r\npayload"[..],
            true,
        ),
        (
            &b"HTTP/1.1 200 OK\r\nContent-Length: 7\r\nConnection: close\r\n\r\npayload"[..],
            true,
        ),
        (
            &b"HTTP/1.1 200 OK\r\nContent-Length: 99\r\nConnection: close\r\n\r\nshort"[..],
            false,
        ),
        (
            &b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"[..],
            false,
        ),
    ] {
        let (url, worker) = server(response);
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("download");
        let progress = RefCell::new(Vec::new());
        let result = download(&url, &path, |p| progress.borrow_mut().push(p));
        worker.join().unwrap();
        assert_eq!(result.is_ok(), success, "{result:?}");
        assert_eq!(progress.borrow().first(), Some(&0));
        if success {
            assert_eq!(progress.borrow().last(), Some(&100));
            assert_eq!(fs::read(&path).unwrap(), b"payload");
        } else {
            assert!(!progress.borrow().contains(&100));
        }
    }
}

#[test]
fn hash_rejects_a_corrupted_download() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("file");
    fs::write(&path, b"payload").unwrap();
    let hash = format!("{:x}", Sha256::digest(b"payload"));
    verify_sha256(&path, &hash.to_uppercase()).unwrap();
    fs::write(&path, b"tampered").unwrap();
    assert!(verify_sha256(&path, &hash).is_err());
}

#[test]
fn malformed_manifest_is_an_error() {
    let (url, worker) = server(b"HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n{broken");
    assert!(get_json::<serde_json::Value>(&url).is_err());
    worker.join().unwrap();
}
