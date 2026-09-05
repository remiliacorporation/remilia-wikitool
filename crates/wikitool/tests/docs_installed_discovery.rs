use std::io::{Read, Write};
use std::net::TcpListener;
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

fn server(responses: Vec<Value>) -> (String, thread::JoinHandle<Vec<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let url = format!("http://{}/api.php", listener.local_addr().unwrap());
    let task = thread::spawn(move || {
        let mut requests = Vec::new();
        for response in responses {
            let deadline = Instant::now() + Duration::from_secs(10);
            let mut stream = loop {
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(
                            Instant::now() < deadline,
                            "expected HTTP request did not arrive"
                        );
                        thread::sleep(Duration::from_millis(10));
                    }
                    Err(error) => panic!("accept fixture request: {error}"),
                }
            };
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut request = Vec::new();
            while !request.ends_with(b"\r\n\r\n") {
                let mut byte = [0];
                stream.read_exact(&mut byte).unwrap();
                request.push(byte[0]);
                assert!(request.len() <= 16 * 1024);
            }
            requests.push(String::from_utf8(request).unwrap());
            let body = response.to_string();
            write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
        }
        requests
    });
    (url, task)
}

fn import(url: &str) -> std::process::Output {
    let project = tempfile::tempdir().unwrap();
    std::fs::create_dir(project.path().join(".wikitool")).unwrap();
    Command::new(env!("CARGO_BIN_EXE_wikitool"))
        .arg("--project-root")
        .arg(project.path())
        .args(["docs", "import", "--installed", "--no-subpages"])
        .env("WIKITOOL_INSTALLED_EXTENSIONS_API_URL", url)
        .env("WIKITOOL_DOCS_API_URL", url)
        .env("WIKITOOL_DOCS_RETRIES", "0")
        .env("WIKITOOL_DOCS_TIMEOUT_MS", "3000")
        .output()
        .unwrap()
}

#[test]
fn installed_docs_import_fetches_extensions_without_skin_namespace_guessing() {
    let (url, task) = server(vec![
        json!({"query":{"extensions":[
            {"name":"MinervaNeue","type":"skin"},
            {"name":"Example","type":"parserhook"}
        ]}}),
        json!({"query":{"pages":[{
            "title":"Extension:Example",
            "revisions":[{"timestamp":"2026-09-05T00:00:00Z","slots":{"main":{"content":"== Usage ==\nExample extension documentation."}}}]
        }]}}),
    ]);
    let output = import(&url);
    let requests = task.join().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("imported_extensions: 1"));
    assert!(requests[0].contains("siprop=extensions"));
    assert!(requests[1].contains("Extension%3AExample"));
    assert!(requests.iter().all(|request| !request.contains("Minerva")));
}

#[test]
fn installed_docs_import_refuses_api_error_before_document_requests() {
    let (url, task) = server(vec![json!({"error":{"code":"maxlag"}})]);
    let output = import(&url);
    assert_eq!(task.join().unwrap().len(), 1);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("API reported an error"));
}
