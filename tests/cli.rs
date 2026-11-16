//! CLI behaviour: `--check-config` reports readiness without opening a socket
//! and without contacting any provider.

use std::process::Command;

fn binary() -> Command {
    Command::new(env!("CARGO_BIN_EXE_rustxng"))
}

fn json_stdout(output: &std::process::Output) -> serde_json::Value {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "stdout is not JSON ({error}): {}",
            String::from_utf8_lossy(&output.stdout)
        )
    })
}

#[test]
fn check_config_reports_the_clean_profile_readiness() {
    let output = binary()
        .args(["--check-config", "--profile", "clean"])
        .output()
        .expect("run the binary");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json = json_stdout(&output);
    assert_eq!(json["profile"], "clean");
    // Valid but surprising settings are reported next to the readiness.
    assert!(
        json["warnings"].is_array(),
        "warnings are part of the report: {json}"
    );
    assert_eq!(json["engines"], 0);
    // The repository ships the two indexes next to Cargo.toml, and the test runs there.
    assert_eq!(json["local_index"], true, "{json}");
    assert_eq!(json["ready"], true, "{json}");
}

#[test]
fn check_config_reports_a_profile_without_sources() {
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join("settings.toml");
    std::fs::write(
        &path,
        "use_default_settings = false\n[search]\nlocal_index_path = \"\"\nwikipedia_index_path = \"\"\n",
    )
    .expect("write settings");
    let output = binary()
        .args([
            "--check-config",
            "--config",
            path.to_str().expect("utf-8 path"),
        ])
        .output()
        .expect("run the binary");
    assert!(output.status.success());
    let json = json_stdout(&output);
    assert_eq!(json["ready"], false, "{json}");
    assert_eq!(json["engines"], 0);
    assert_eq!(json["reasons"].as_array().map(Vec::len), Some(2), "{json}");
}

#[test]
fn check_config_fails_on_malformed_settings() {
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join("settings.toml");
    std::fs::write(&path, "server = \"not a table\"\n").expect("write settings");
    let output = binary()
        .args([
            "--check-config",
            "--config",
            path.to_str().expect("utf-8 path"),
        ])
        .output()
        .expect("run the binary");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("error:"), "{stderr}");
}

#[test]
fn check_config_cannot_be_combined_with_mcp() {
    let output = binary()
        .args(["mcp", "--check-config"])
        .output()
        .expect("run the binary");
    assert_eq!(output.status.code(), Some(2));
}

#[cfg(unix)]
#[test]
fn sigterm_stops_the_server_gracefully() {
    use std::io::Write as _;
    use std::net::TcpStream;
    use std::time::{Duration, Instant};

    let dir = tempfile::tempdir().expect("temp dir");
    let port = {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("pick a port");
        listener.local_addr().expect("local addr").port()
    };
    let settings = dir.path().join("settings.toml");
    let mut file = std::fs::File::create(&settings).expect("create settings");
    write!(
        file,
        "use_default_settings = false\n[server]\nbind_address = \"127.0.0.1\"\nport = {port}\nsecret_key = \"cli-test-secret\"\n[search]\nlocal_index_path = \"\"\n"
    )
    .expect("write settings");
    drop(file);

    let mut child = Command::new(env!("CARGO_BIN_EXE_rustxng"))
        .args(["--config", settings.to_str().expect("utf-8 path")])
        .env("RUST_LOG", "error")
        .env("RUSTXNG_IDLE_TIMEOUT", "0")
        .spawn()
        .expect("start the server");

    // Wait for a real HTTP answer: the shutdown future is polled by the same
    // server future, so answering requests means SIGTERM is already watched.
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut ready = false;
    while Instant::now() < deadline {
        if let Ok(mut stream) = TcpStream::connect(("127.0.0.1", port)) {
            use std::io::{Read as _, Write as _};
            let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
            let request = b"GET /healthz HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n";
            if stream.write_all(request).is_ok() {
                let mut answer = String::new();
                if stream.read_to_string(&mut answer).is_ok() && answer.contains(" 200 ") {
                    ready = true;
                    break;
                }
            }
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    if !ready {
        let _ = child.kill();
        panic!("the server did not answer /healthz on port {port}");
    }

    let status = Command::new("kill")
        .args(["-TERM", &child.id().to_string()])
        .status()
        .expect("send SIGTERM");
    assert!(status.success(), "kill -TERM failed");

    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        match child.try_wait().expect("wait for the server") {
            Some(status) => {
                assert_eq!(
                    status.code(),
                    Some(0),
                    "SIGTERM must stop the server cleanly"
                );
                return;
            }
            None if Instant::now() >= deadline => {
                let _ = child.kill();
                panic!("the server did not exit within 15s of SIGTERM");
            }
            None => std::thread::sleep(Duration::from_millis(50)),
        }
    }
}
