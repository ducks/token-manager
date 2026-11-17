use std::{
    fs,
    io::Write,
    net::TcpListener,
    os::unix::fs::PermissionsExt,
    process::{Command, Stdio},
    thread,
};
#[test]
fn keyring_login_uses_stdin_and_lookup_roundtrips_without_plaintext_config() {
    use std::io::Read;
    let tmp = tempfile::tempdir().unwrap();
    let config = tmp.path().join("config");
    fs::create_dir_all(config.join("providers")).unwrap();
    let socket = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", socket.local_addr().unwrap());
    let server = thread::spawn(move || {
        let (mut client, _) = socket.accept().unwrap();
        let mut buffer = [0; 4096];
        let n = client.read(&mut buffer).unwrap();
        assert!(String::from_utf8_lossy(&buffer[..n]).contains("fixture-keyring-secret"));
        let body = r#"{"balance":"8","total_used":"2"}"#;
        write!(
            client,
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
        .unwrap();
    });
    let provider =
        include_str!("../providers/vercel.toml").replace("https://ai-gateway.vercel.sh", &url);
    fs::write(config.join("providers/vercel.toml"), provider).unwrap();
    let fake = tmp.path().join("secret-tool");
    fs::write(&fake,"#!/bin/sh\numask 077\nprintf '%s\\n' \"$@\" > \"$FIXTURE_ARGS\"\ncase \"$1\" in\nstore) cat > \"$FIXTURE_KEY\";;\nlookup) cat \"$FIXTURE_KEY\";;\n*) exit 1;;\nesac\n").unwrap();
    fs::set_permissions(&fake, fs::Permissions::from_mode(0o700)).unwrap();
    let key = tmp.path().join("secret");
    let args = tmp.path().join("arguments");
    let command = || {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_token-manager"));
        cmd.args(["--allow-local-http", "--config-dir"])
            .arg(&config)
            .env(
                "PATH",
                format!(
                    "{}:{}",
                    tmp.path().display(),
                    std::env::var("PATH").unwrap()
                ),
            )
            .env("FIXTURE_KEY", &key)
            .env("FIXTURE_ARGS", &args);
        cmd
    };
    let mut child = command()
        .args(["add", "vercel", "--key-stdin"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"fixture-keyring-secret")
        .unwrap();
    assert!(child.wait().unwrap().success());
    assert!(
        !fs::read_to_string(config.join("accounts.json"))
            .unwrap()
            .contains("fixture-keyring-secret")
    );
    assert!(
        !fs::read_to_string(&args)
            .unwrap()
            .contains("fixture-keyring-secret")
    );
    let output = command()
        .args(["status", "--refresh", "--json"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let rows: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(rows[0]["data"]["balances"][0]["remaining"], "8");
    assert!(!String::from_utf8_lossy(&output.stdout).contains("fixture-keyring-secret"));
    server.join().unwrap();
}
