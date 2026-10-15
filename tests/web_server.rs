//! WebUI server tests: auth rejection, protocol version, headless editing.
//!
//! These drive the real server over a WebSocket (tungstenite client) against
//! a server bound to 127.0.0.1:0. No browser needed.

use erax::cli::{Invocation, InvocationMode, SedOptions};
use erax::web::protocol::*;
use erax::web::server::{bind_server, serve};
use std::net::TcpStream;
use std::time::Duration;
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{connect, Message, WebSocket};

type ClientWs = WebSocket<MaybeTlsStream<TcpStream>>;

/// Set a read timeout on the underlying TCP stream.
fn set_timeout(ws: &mut ClientWs, timeout: Option<Duration>) {
    if let MaybeTlsStream::Plain(tcp) = ws.get_ref() {
        tcp.set_read_timeout(timeout).expect("set_read_timeout failed");
    } else {
        panic!("expected plain TCP stream");
    }
}

fn test_invocation() -> Invocation {
    Invocation {
        mode: InvocationMode::WebServer,
        files: vec![],
        theme: None,
        wrap: None,
        sed: SedOptions::default(),
        web_bind: "127.0.0.1:0".to_string(),
        web_allow_remote: false,
    }
}

/// Start a server in the background; return (addr, token).
fn start_server() -> (std::net::SocketAddr, String) {
    let server = bind_server("127.0.0.1:0", false).expect("bind failed");
    let addr = server.addr;
    let token = server.token.clone();
    let inv = test_invocation();
    std::thread::spawn(move || serve(server, &inv));
    // Give the accept loop a moment to start.
    std::thread::sleep(Duration::from_millis(100));
    (addr, token)
}

fn ws_url(addr: &std::net::SocketAddr) -> String {
    format!("ws://{addr}/ws")
}

fn read_server_frame(ws: &mut ClientWs) -> ServerFrame {
    // Generous: the server initializes a full editor session (debug builds
    // under parallel-test load take seconds to reach the first Screen).
    set_timeout(&mut *ws, Some(Duration::from_secs(30)));
    loop {
        match ws.read().expect("ws read failed") {
            Message::Text(text) => {
                return decode(&text).expect("server frame decode failed");
            }
            Message::Close(_) => panic!("server closed the connection unexpectedly"),
            _ => continue,
        }
    }
}

/// Connect and wait for `Hello`, returning `None` instead of panicking when
/// the handshake does not complete in time.
///
/// Under full-suite parallel load the server's accept-loop thread can be
/// starved for tens of seconds: the listener is bound (so `connect`
/// succeeds into the backlog) but no thread accepts, and the client's 30 s
/// read then fires. That is scheduling luck, not a protocol failure — the
/// caller retries until an overall deadline.
fn connect_hello(addr: &std::net::SocketAddr) -> Option<(ClientWs, ServerFrame)> {
    let (mut ws, _) = connect(ws_url(addr)).ok()?;
    set_timeout(&mut ws, Some(Duration::from_secs(30)));
    loop {
        match ws.read() {
            Ok(Message::Text(text)) => {
                let frame: ServerFrame = decode(&text).ok()?;
                if matches!(frame, ServerFrame::Hello { .. }) {
                    return Some((ws, frame));
                }
            }
            Ok(_) => continue,
            Err(_) => return None,
        }
    }
}

#[test]
fn hello_carries_protocol_version() {
    let (addr, _token) = start_server();
    let (mut ws, _) = connect(ws_url(&addr)).expect("connect failed");
    match read_server_frame(&mut ws) {
        ServerFrame::Hello { version, .. } => assert_eq!(version, PROTOCOL_VERSION),
        other => panic!("expected Hello, got {other:?}"),
    }
    // Don't auth; just close.
}

#[test]
fn auth_rejects_wrong_token() {
    let (addr, _token) = start_server();
    let (mut ws, _) = connect(ws_url(&addr)).expect("connect failed");
    let _hello = read_server_frame(&mut ws);
    ws.send(Message::Text(
        encode(&ClientFrame::Auth {
            token: "wrong-token".to_string(),
        })
        .into(),
    ))
    .unwrap();
    match read_server_frame(&mut ws) {
        ServerFrame::AuthError { reason } => {
            assert!(reason.contains("invalid token"), "reason: {reason}");
        }
        other => panic!("expected AuthError, got {other:?}"),
    }
    // The server closes the connection after AuthError.
    set_timeout(&mut ws, Some(Duration::from_secs(5)));
    match ws.read() {
        Ok(Message::Close(_)) | Err(_) => {}
        Ok(other) => panic!("expected close after AuthError, got {other:?}"),
    }
}

#[test]
fn auth_rejects_non_auth_first_frame() {
    let (addr, token) = start_server();
    let (mut ws, _) = connect(ws_url(&addr)).expect("connect failed");
    let _hello = read_server_frame(&mut ws);
    // Send input before auth: must be rejected.
    ws.send(Message::Text(
        encode(&ClientFrame::Input {
            event: InputEvent::Key(erax::core::input::KeyPress::plain(
                erax::core::input::Key::Char('x'),
            )),
        })
        .into(),
    ))
    .unwrap();
    match read_server_frame(&mut ws) {
        ServerFrame::AuthError { .. } => {}
        other => panic!("expected AuthError, got {other:?}"),
    }
    let _ = token;
}

#[test]
fn headless_editing_session() {
    let (addr, token) = start_server();
    // Retry the connect+Hello handshake until an overall deadline: under
    // parallel-test load the accept loop can be starved past one 30 s
    // read timeout. Once Hello arrives the server thread is provably
    // alive, so the later phases keep their single 30 s timeouts.
    let overall = std::time::Instant::now() + Duration::from_secs(150);
    let (mut ws, _hello) = loop {
        if let Some(pair) = connect_hello(&addr) {
            break pair;
        }
        assert!(
            std::time::Instant::now() < overall,
            "server never completed the websocket handshake (accept loop starved?)"
        );
        std::thread::sleep(Duration::from_secs(1));
    };

    // Auth with the real token.
    ws.send(Message::Text(
        encode(&ClientFrame::Auth { token }).into(),
    ))
    .unwrap();
    match read_server_frame(&mut ws) {
        ServerFrame::AuthOk => {}
        other => panic!("expected AuthOk, got {other:?}"),
    }

    // Initial full screen.
    let first = read_server_frame(&mut ws);
    let mut saw_screen = false;
    if let ServerFrame::Screen { seq, .. } = first {
        assert_eq!(seq, 1);
        saw_screen = true;
    }
    assert!(saw_screen, "expected initial Screen frame");

    // Type "hi" — two key events. The backend is in its default input
    // model; typing should insert text and produce new Screen frames.
    for c in ['h', 'i'] {
        ws.send(Message::Text(
            encode(&ClientFrame::Input {
                event: InputEvent::Key(erax::core::input::KeyPress::plain(
                    erax::core::input::Key::Char(c),
                )),
            })
            .into(),
        ))
        .unwrap();
    }

    // Collect Screen frames for a bit; at least one must contain our text.
    let mut found_h = false;
    let mut found_i = false;
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while std::time::Instant::now() < deadline && !(found_h && found_i) {
        set_timeout(&mut ws, Some(Duration::from_millis(500)));
        match ws.read() {
            Ok(Message::Text(text)) => {
                if let Ok(ServerFrame::Screen { cells, .. }) = decode::<ServerFrame>(&text) {
                    for cu in cells {
                        if cu.cell.text() == "h" {
                            found_h = true;
                        }
                        if cu.cell.text() == "i" {
                            found_i = true;
                        }
                    }
                }
            }
            Ok(_) => {}
            Err(_) => break,
        }
    }
    assert!(
        found_h && found_i,
        "typed 'hi' did not appear in any Screen frame"
    );
}

#[test]
fn non_loopback_bind_refused_without_flag() {
    let err = bind_server("0.0.0.0:0", false).expect_err("non-loopback bind must be refused");
    let msg = err.to_string();
    assert!(
        msg.contains("--web-allow-remote"),
        "error must name the flag: {msg}"
    );
}

#[test]
fn non_loopback_bind_allowed_with_flag() {
    // With the acknowledging flag, the bind proceeds (then we drop it).
    let server = bind_server("0.0.0.0:0", true).expect("bind with flag failed");
    assert!(!server.token.is_empty());
}

// ---------------------------------------------------------------------------
// Hostile clients, auth edge cases, server robustness.
// ---------------------------------------------------------------------------

use std::io::{Read, Write};

/// Auth with an empty token: rejected exactly like any wrong token.
#[test]
fn auth_rejects_empty_token() {
    let (addr, _token) = start_server();
    let (mut ws, _) = connect(ws_url(&addr)).expect("connect failed");
    let _hello = read_server_frame(&mut ws);
    ws.send(Message::Text(
        encode(&ClientFrame::Auth {
            token: String::new(),
        })
        .into(),
    ))
    .unwrap();
    match read_server_frame(&mut ws) {
        ServerFrame::AuthError { reason } => {
            assert!(reason.contains("invalid token"), "reason: {reason}");
        }
        other => panic!("expected AuthError, got {other:?}"),
    }
}

/// A frame bigger than the 1 MiB cap: the server must drop the connection,
/// not buffer and parse it.
#[test]
fn oversized_frame_rejected() {
    let (addr, _token) = start_server();
    let (mut ws, _) = connect(ws_url(&addr)).expect("connect failed");
    let _hello = read_server_frame(&mut ws);
    let big = "x".repeat(2 << 20);
    ws.send(Message::Text(big.into())).expect("send failed");
    // The server must terminate the connection promptly (not hang, not parse).
    set_timeout(&mut ws, Some(Duration::from_secs(5)));
    let mut saw_end = false;
    for _ in 0..10 {
        match ws.read() {
            Ok(Message::Close(_)) | Err(_) => {
                saw_end = true;
                break;
            }
            Ok(_) => {}
        }
    }
    assert!(
        saw_end,
        "server did not terminate the oversized-frame connection"
    );
}

/// Malformed JSON as the first frame: typed protocol error, connection dropped.
#[test]
fn malformed_json_frame_closes_connection() {
    let (addr, _token) = start_server();
    let (mut ws, _) = connect(ws_url(&addr)).expect("connect failed");
    let _hello = read_server_frame(&mut ws);
    ws.send(Message::Text("{not json".into())).unwrap();
    set_timeout(&mut ws, Some(Duration::from_secs(5)));
    let mut saw_end = false;
    for _ in 0..10 {
        match ws.read() {
            Ok(Message::Close(_)) | Err(_) => {
                saw_end = true;
                break;
            }
            Ok(_) => {}
        }
    }
    assert!(
        saw_end,
        "server did not drop the malformed-frame connection"
    );
}

/// HTTP header flood: the server caps headers and drops the connection
/// instead of reading forever.
#[test]
fn http_header_flood_capped() {
    let (addr, _token) = start_server();
    let mut stream = TcpStream::connect(addr).expect("connect failed");
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    stream.write_all(b"GET / HTTP/1.1\r\nHost: x\r\n").unwrap();
    // 200 header lines: over the 128-line cap. The server is expected to
    // trip the cap and RST the connection mid-write — a write error here
    // IS the desired server behavior, not a test failure.
    for i in 0..200 {
        if write!(stream, "X-Flood-{i}: {}\r\n", "y".repeat(64)).is_err() {
            break;
        }
    }
    let _ = stream.write_all(b"\r\n");
    // The server must close the connection (the cap trips long before the
    // 15s stream read timeout).
    let mut buf = [0u8; 1];
    let start = std::time::Instant::now();
    let closed = loop {
        match stream.read(&mut buf) {
            Ok(0) => break true,
            Ok(_) => {}
            Err(e)
                if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut =>
            {
                break false
            }
            Err(_) => break true,
        }
    };
    assert!(closed, "server did not cap the header flood");
    assert!(
        start.elapsed() < Duration::from_secs(10),
        "header cap was not prompt"
    );
}

/// Connection cap: 40 idle connections, only 32 may be held; the overflow
/// is dropped immediately without spawning handler threads.
#[test]
fn concurrent_connection_cap() {
    let (addr, _token) = start_server();
    let mut streams: Vec<TcpStream> = (0..40)
        .map(|_| {
            let s = TcpStream::connect(addr).expect("connect failed");
            s.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
            s
        })
        .collect();
    // Let the accept loop drain the backlog.
    std::thread::sleep(Duration::from_secs(2));
    let mut eofs = 0;
    let mut timeouts = 0;
    let mut buf = [0u8; 1];
    for s in &mut streams {
        // Short client-side timeout: held connections are still parked in
        // the server's 15s peek, so a read on them must time out here
        // (WouldBlock/TimedOut) rather than EOF; cap-dropped ones EOF
        // promptly because the server closed them.
        s.set_read_timeout(Some(Duration::from_millis(300))).unwrap();
        match s.read(&mut buf) {
            // Server closed the socket: dropped by the cap.
            Ok(0) => eofs += 1,
            // Still open (or unexpected data): held by the server.
            Ok(_) => timeouts += 1,
            Err(e)
                if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut =>
            {
                timeouts += 1
            }
            // Any other error: treat as closed.
            Err(_) => eofs += 1,
        }
    }
    assert_eq!(eofs + timeouts, 40);
    // 8 overflow connections should have been dropped (40 - 32 cap).
    // Bounds are tolerant of accept-loop scheduling slop.
    assert!(
        eofs >= 6,
        "cap did not drop overflow connections (eofs={eofs})"
    );
    assert!(
        timeouts >= 30,
        "too many connections dropped (timeouts={timeouts})"
    );
}

/// Raw HTTP GET against the server; returns (status line, body).
fn http_get(addr: &std::net::SocketAddr, path: &str) -> (String, Vec<u8>) {
    let mut stream = TcpStream::connect(addr).expect("connect failed");
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    write!(
        stream,
        "GET {path} HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n"
    )
    .unwrap();
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).expect("read failed");
    let header_end = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .expect("no header end");
    let headers = String::from_utf8_lossy(&raw[..header_end]).into_owned();
    let status = headers.lines().next().unwrap_or("").to_string();
    (status, raw[header_end + 4..].to_vec())
}

/// The web client assets are embedded in the server binary: they are
/// served correctly with no web-assets/ directory anywhere near the
/// server (the installed-layout case the old filesystem search broke).
#[test]
fn http_serves_embedded_assets() {
    let (addr, _token) = start_server();
    let (status, body) = http_get(&addr, "/");
    assert!(status.contains("200"), "status: {status}");
    assert!(
        body.windows(7).any(|w| w == b"<title>"),
        "index.html body wrong"
    );
    let (status, body) = http_get(&addr, "/erax-web-shell.js");
    assert!(status.contains("200"), "status: {status}");
    assert!(!body.is_empty(), "js glue empty");
    let (status, body) = http_get(&addr, "/erax-web-shell_bg.wasm");
    assert!(status.contains("200"), "status: {status}");
    assert_eq!(&body[..4], b"\0asm", "not a wasm module");
    assert!(
        body.len() > 2_000_000,
        "wasm suspiciously small: {}",
        body.len()
    );
    let (status, _) = http_get(&addr, "/nope");
    assert!(status.contains("404"), "status: {status}");
}

/// A client that dies mid-session must not wedge the server: a subsequent
/// client still gets a working session.
#[test]
fn aborted_connection_then_server_still_serves() {
    let (addr, token) = start_server();
    // Client A: auth, then abort without a Close handshake.
    {
        let (mut ws, _) = connect(ws_url(&addr)).expect("connect failed");
        let _hello = read_server_frame(&mut ws);
        ws.send(Message::Text(
            encode(&ClientFrame::Auth {
                token: token.clone(),
            })
            .into(),
        ))
        .unwrap();
        match read_server_frame(&mut ws) {
            ServerFrame::AuthOk => {}
            other => panic!("expected AuthOk, got {other:?}"),
        }
        // Violent abort: shut the TCP stream down under the websocket.
        if let MaybeTlsStream::Plain(tcp) = ws.get_ref() {
            tcp.shutdown(std::net::Shutdown::Both)
                .expect("shutdown failed");
        }
        // `ws` is dropped here with no Close frame.
    }
    // Client B: a full session must still work.
    let (mut ws, _) = connect(ws_url(&addr)).expect("connect failed");
    let _hello = read_server_frame(&mut ws);
    ws.send(Message::Text(encode(&ClientFrame::Auth { token }).into()))
        .unwrap();
    match read_server_frame(&mut ws) {
        ServerFrame::AuthOk => {}
        other => panic!("expected AuthOk, got {other:?}"),
    }
}

/// The font-stack manifest lists the configured stack in order; every entry
/// resolves to real font bytes. The web client needs this (it has no system
/// fonts) to shape with the same fallback the native shell gets.
#[test]
fn http_serves_font_stack() {
    let (addr, _token) = start_server();
    let (status, body) = http_get(&addr, "/font-stack");
    assert!(status.contains("200"), "status: {status}");
    let manifest: serde_json::Value =
        serde_json::from_slice(&body).expect("manifest must be JSON");
    let entries = manifest.as_array().expect("manifest must be an array");
    assert!(!entries.is_empty(), "stack must not be empty on a dev machine");
    for entry in entries {
        let family = entry["family"].as_str().expect("family string");
        let url = entry["url"].as_str().expect("url string");
        assert!(!family.is_empty(), "family must not be empty");
        assert!(url.starts_with("/font/"), "url shape: {url}");
        let (status, font_body) = http_get(&addr, url);
        assert!(status.contains("200"), "{url} status: {status}");
        assert!(!font_body.is_empty(), "{url} (family {family}) empty");
    }
    // Out of range and non-numeric indices are 404, not panics.
    let (status, _) = http_get(&addr, "/font/999");
    assert!(status.contains("404"), "status: {status}");
    let (status, _) = http_get(&addr, "/font/abc");
    assert!(status.contains("404"), "status: {status}");
    // The legacy single-font endpoint still serves the primary.
    let (status, body) = http_get(&addr, "/font");
    assert!(status.contains("200"), "status: {status}");
    assert!(!body.is_empty(), "/font empty");
}

/// Send one keypress to the server as a client Input frame.
fn send_key(ws: &mut ClientWs, kp: erax::core::input::KeyPress) {
    ws.send(
        Message::Text(
            encode(&ClientFrame::Input {
                event: InputEvent::Key(kp),
            })
            .into(),
        ),
    )
    .unwrap();
}

/// A request line split across TCP packets must still route correctly:
/// the server waits (bounded) for the line terminator before deciding
/// between the WebSocket upgrade and static-asset paths.
#[test]
fn split_request_line_still_upgrades() {
    use std::io::{BufRead, Write};
    let (addr, _token) = start_server();
    let mut stream = TcpStream::connect(addr).expect("connect failed");
    stream
        .set_read_timeout(Some(Duration::from_secs(20)))
        .unwrap();
    // First packet: a partial request line, no terminator yet.
    stream.write_all(b"GE").unwrap();
    stream.flush().unwrap();
    std::thread::sleep(Duration::from_millis(300));
    // Second packet: the rest of the line plus a valid upgrade handshake.
    // A server that evaluated the peeked prefix would 404/200 here.
    stream
        .write_all(
            b"T /ws HTTP/1.1\r\nHost: x\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\n\r\n",
        )
        .unwrap();
    stream.flush().unwrap();
    let mut reader = std::io::BufReader::new(&stream);
    let mut status = String::new();
    reader
        .read_line(&mut status)
        .expect("read status line failed");
    assert!(
        status.starts_with("HTTP/1.1 101"),
        "split request line must still upgrade, got {status:?}"
    );
}

/// Terminal child output must reach the web client on its own: the
/// websocket loop polls the terminal host the way the TUI loop does, so
/// a frame carrying the output arrives with no client event after it.
#[test]
fn terminal_output_reaches_client_unprompted() {
    let (addr, token) = start_server();
    let overall = std::time::Instant::now() + Duration::from_secs(150);
    let (mut ws, _hello) = loop {
        if let Some(pair) = connect_hello(&addr) {
            break pair;
        }
        assert!(
            std::time::Instant::now() < overall,
            "server never completed the websocket handshake (accept loop starved?)"
        );
        std::thread::sleep(Duration::from_secs(1));
    };
    ws.send(Message::Text(encode(&ClientFrame::Auth { token }).into()))
        .unwrap();
    match read_server_frame(&mut ws) {
        ServerFrame::AuthOk => {}
        other => panic!("expected AuthOk, got {other:?}"),
    }
    match read_server_frame(&mut ws) {
        ServerFrame::Screen { .. } => {}
        other => panic!("expected initial Screen, got {other:?}"),
    }

    // Spawn the built-in terminal: C-x t 2 in the server's keytrie
    // model (split + terminal, new window active).
    let ctrl = erax::core::input::KeyPress::ctrl;
    let plain = erax::core::input::KeyPress::plain;
    let ch = erax::core::input::Key::Char;
    for kp in [ctrl(ch('x')), plain(ch('t')), plain(ch('2'))] {
        send_key(&mut ws, kp);
        std::thread::sleep(Duration::from_millis(150));
    }
    // Drain to the post-spawn screen.
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    loop {
        if let ServerFrame::Screen { .. } = read_server_frame(&mut ws) {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "no screen after spawn-terminal"
        );
    }

    // Run a command whose output is a value that never appears in the
    // typed input itself (the pty echoes input, so a literal marker
    // would be visible before the shell runs).
    for c in "echo $((12345*6789))".chars() {
        send_key(
            &mut ws,
            erax::core::input::KeyPress::plain(erax::core::input::Key::Char(c)),
        );
    }
    send_key(
        &mut ws,
        erax::core::input::KeyPress::plain(erax::core::input::Key::Enter),
    );

    // From here on, nothing is sent. The shell's answer (83810205) must
    // arrive in a Screen frame on the server's own 100ms terminal poll.
    let mut grid: std::collections::HashMap<(u16, u16), String> =
        std::collections::HashMap::new();
    let deadline = std::time::Instant::now() + Duration::from_secs(25);
    let mut saw_output = false;
    while std::time::Instant::now() < deadline && !saw_output {
        set_timeout(&mut ws, Some(Duration::from_millis(500)));
        let frame = match ws.read() {
            Ok(Message::Text(t)) => decode::<ServerFrame>(&t).expect("decode failed"),
            _ => continue,
        };
        if let ServerFrame::Screen { cells, .. } = frame {
            for cu in &cells {
                grid.insert((cu.x, cu.y), cu.cell.text.as_str().to_string());
            }
            let mut rows: std::collections::BTreeMap<u16, Vec<(u16, &String)>> =
                std::collections::BTreeMap::new();
            for ((x, y), text) in &grid {
                rows.entry(*y).or_default().push((*x, text));
            }
            for (_y, mut row) in rows {
                row.sort_by_key(|(x, _)| *x);
                let text: String = row.into_iter().map(|(_, s)| s.as_str()).collect();
                if text.contains("83810205") && !text.contains("echo") {
                    saw_output = true;
                    break;
                }
            }
        }
    }
    assert!(
        saw_output,
        "terminal output never reached the client without a client event"
    );
}

/// An overlong header line is rejected by the per-line read bound itself:
/// the server closes the connection instead of buffering the line
/// without limit or stalling past the read deadline.
#[test]
fn overlong_header_line_is_rejected() {
    use std::io::{Read, Write};
    let (addr, _token) = start_server();
    let mut stream = TcpStream::connect(addr).expect("connect failed");
    stream
        .set_read_timeout(Some(Duration::from_secs(20)))
        .unwrap();
    stream.write_all(b"GET / HTTP/1.1\r\n").unwrap();
    stream.write_all(b"X-Pad: ").unwrap();
    stream.write_all(&vec![b'A'; 16_384]).unwrap();
    stream.write_all(b"\r\n\r\n").unwrap();
    stream.flush().unwrap();
    // The server must close (or error) promptly: EOF, not a hang and
    // not a 200 served after buffering 16KB into the line.
    let mut buf = [0u8; 1];
    match stream.read(&mut buf) {
        Ok(0) => {}
        Err(e)
            if e.kind() == std::io::ErrorKind::UnexpectedEof
                || e.kind() == std::io::ErrorKind::ConnectionReset =>
        {
            // also acceptable
        }
        other => panic!("expected prompt close on overlong header, got {other:?}"),
    }
}
