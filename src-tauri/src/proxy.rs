//! Tunnels the SSH transport through an HTTP CONNECT or SOCKS5 proxy.
//!
//! OpenSSH has no built-in proxy client, and the usual helpers (`nc -X`,
//! `corkscrew`, `socat`) are not installed everywhere. Heminus therefore
//! re-executes itself as the `ProxyCommand`: the child speaks the proxy
//! handshake, then pipes bytes between its standard streams and the socket.
//! Keeping the connector in-process means proxies work identically on Linux
//! and Windows and the password never appears on a command line.

use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

use base64::Engine as _;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use heminus_domain::{HostProxy, ProxyKind};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

const CONNECT_FLAG: &str = "--heminus-proxy-connect";
const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_HTTP_RESPONSE: usize = 16 * 1024;

/// Everything the connector needs, encoded into a single shell-safe argument.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProxySpec {
    pub kind: ProxyKind,
    pub hostname: String,
    pub port: u16,
    #[serde(default)]
    pub username: Option<String>,
    /// The host whose keyring entry holds the proxy password.
    #[serde(default)]
    pub host_id: Option<Uuid>,
}

impl ProxySpec {
    pub fn from_host_proxy(proxy: &HostProxy, host_id: Uuid) -> Self {
        Self {
            kind: proxy.kind,
            hostname: proxy.hostname.trim().to_string(),
            port: proxy.port,
            username: proxy
                .username
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_owned),
            host_id: proxy.secret_stored.then_some(host_id),
        }
    }

    pub fn encode(&self) -> Result<String, String> {
        let json = serde_json::to_vec(self)
            .map_err(|error| format!("Could not prepare the proxy settings: {error}"))?;
        Ok(URL_SAFE_NO_PAD.encode(json))
    }

    fn decode(value: &str) -> Result<Self, String> {
        let json = URL_SAFE_NO_PAD
            .decode(value)
            .map_err(|error| format!("Invalid proxy settings: {error}"))?;
        serde_json::from_slice(&json).map_err(|error| format!("Invalid proxy settings: {error}"))
    }
}

pub const fn connect_flag() -> &'static str {
    CONNECT_FLAG
}

/// Runs the `ProxyCommand` side of Heminus when SSH re-executes the binary.
///
/// Returns `false` for a normal application launch so `main` can continue.
pub fn run_proxy_connect_if_requested() -> bool {
    let arguments = std::env::args().collect::<Vec<_>>();
    let Some(index) = arguments.iter().position(|value| value == CONNECT_FLAG) else {
        return false;
    };
    let outcome = (|| {
        let spec = arguments
            .get(index + 1)
            .ok_or_else(|| "The proxy settings are missing".to_string())
            .and_then(|value| ProxySpec::decode(value))?;
        let target_host = arguments
            .get(index + 2)
            .ok_or_else(|| "The proxy target host is missing".to_string())?;
        let target_port = arguments
            .get(index + 3)
            .ok_or_else(|| "The proxy target port is missing".to_string())?
            .parse::<u16>()
            .map_err(|_| "The proxy target port is invalid".to_string())?;
        let password = spec
            .host_id
            .map(|host_id| crate::credential::get_proxy(host_id).map(|value| value.to_string()))
            .transpose()?;
        let stream = connect_through(&spec, password.as_deref(), target_host, target_port)?;
        pump(stream)
    })();
    if let Err(error) = outcome {
        // SSH surfaces the ProxyCommand's stderr, so this reaches the session log.
        let _ = writeln!(std::io::stderr(), "heminus proxy: {error}");
        std::process::exit(1);
    }
    true
}

fn connect_through(
    spec: &ProxySpec,
    password: Option<&str>,
    target_host: &str,
    target_port: u16,
) -> Result<TcpStream, String> {
    connect_through_with_timeout(spec, password, target_host, target_port, CONNECT_TIMEOUT)
}

fn connect_through_with_timeout(
    spec: &ProxySpec,
    password: Option<&str>,
    target_host: &str,
    target_port: u16,
    handshake_timeout: Duration,
) -> Result<TcpStream, String> {
    let endpoint = format!("{}:{}", spec.hostname, spec.port);
    let address = endpoint
        .to_socket_addrs()
        .map_err(|error| format!("Could not resolve the proxy {endpoint}: {error}"))?
        .next()
        .ok_or_else(|| format!("The proxy {endpoint} did not resolve to an address"))?;
    let mut stream = TcpStream::connect_timeout(&address, CONNECT_TIMEOUT)
        .map_err(|error| format!("Could not reach the proxy {endpoint}: {error}"))?;
    stream
        .set_nodelay(true)
        .map_err(|error| format!("Could not configure the proxy socket: {error}"))?;
    // `connect_timeout` only bounds the TCP handshake. Without these, a proxy
    // that accepts the connection and then says nothing leaves the read below
    // blocked forever, and the terminal sits on "connecting" with no way out.
    let deadline = Some(handshake_timeout);
    stream
        .set_read_timeout(deadline)
        .and_then(|()| stream.set_write_timeout(deadline))
        .map_err(|error| format!("Could not configure the proxy timeouts: {error}"))?;
    match spec.kind {
        ProxyKind::Http => http_connect(
            &mut stream,
            spec.username.as_deref(),
            password,
            target_host,
            target_port,
        )?,
        ProxyKind::Socks5 => socks5_connect(
            &mut stream,
            spec.username.as_deref(),
            password,
            target_host,
            target_port,
        )?,
    }
    // The tunnel itself is long-lived and mostly idle, so it must not inherit
    // the handshake deadline.
    stream
        .set_read_timeout(None)
        .and_then(|()| stream.set_write_timeout(None))
        .map_err(|error| format!("Could not restore the proxy socket: {error}"))?;
    Ok(stream)
}

fn http_connect(
    stream: &mut TcpStream,
    username: Option<&str>,
    password: Option<&str>,
    target_host: &str,
    target_port: u16,
) -> Result<(), String> {
    let authority = format_authority(target_host, target_port);
    let mut request = format!("CONNECT {authority} HTTP/1.1\r\nHost: {authority}\r\n");
    if let Some(username) = username {
        let credentials = STANDARD.encode(format!("{username}:{}", password.unwrap_or_default()));
        request.push_str(&format!("Proxy-Authorization: Basic {credentials}\r\n"));
    }
    request.push_str("Proxy-Connection: Keep-Alive\r\n\r\n");
    stream
        .write_all(request.as_bytes())
        .and_then(|()| stream.flush())
        .map_err(|error| format!("Could not send the proxy CONNECT request: {error}"))?;

    let mut response = Vec::new();
    let mut byte = [0_u8; 1];
    loop {
        let read = stream
            .read(&mut byte)
            .map_err(|error| format!("Could not read the proxy response: {error}"))?;
        if read == 0 {
            return Err("The proxy closed the connection before replying".into());
        }
        response.push(byte[0]);
        if response.ends_with(b"\r\n\r\n") {
            break;
        }
        if response.len() > MAX_HTTP_RESPONSE {
            return Err("The proxy sent an unexpectedly large response".into());
        }
    }
    let status = String::from_utf8_lossy(&response);
    let first_line = status.lines().next().unwrap_or_default().trim();
    let code = first_line
        .split_whitespace()
        .nth(1)
        .and_then(|value| value.parse::<u16>().ok())
        .ok_or_else(|| format!("The proxy sent an unreadable reply: {first_line}"))?;
    match code {
        200..=299 => Ok(()),
        407 => Err("The proxy rejected the credentials (407 Proxy Authentication Required)".into()),
        403 => Err(format!("The proxy refused the connection to {authority}")),
        _ => Err(format!("The proxy replied: {first_line}")),
    }
}

fn socks5_connect(
    stream: &mut TcpStream,
    username: Option<&str>,
    password: Option<&str>,
    target_host: &str,
    target_port: u16,
) -> Result<(), String> {
    let methods: &[u8] = if username.is_some() {
        &[0x00, 0x02]
    } else {
        &[0x00]
    };
    let mut greeting = vec![0x05, methods.len() as u8];
    greeting.extend_from_slice(methods);
    write_all(stream, &greeting, "the SOCKS5 greeting")?;

    let mut choice = [0_u8; 2];
    read_exact(stream, &mut choice, "the SOCKS5 greeting reply")?;
    if choice[0] != 0x05 {
        return Err("The proxy did not answer with SOCKS5".into());
    }
    match choice[1] {
        0x00 => {}
        0x02 => {
            let username = username
                .ok_or_else(|| "The SOCKS5 proxy requires a username and password".to_string())?;
            let password = password.unwrap_or_default();
            if username.len() > 255 || password.len() > 255 {
                return Err("SOCKS5 usernames and passwords are limited to 255 bytes".into());
            }
            let mut authentication = vec![0x01, username.len() as u8];
            authentication.extend_from_slice(username.as_bytes());
            authentication.push(password.len() as u8);
            authentication.extend_from_slice(password.as_bytes());
            write_all(stream, &authentication, "the SOCKS5 credentials")?;
            let mut reply = [0_u8; 2];
            read_exact(stream, &mut reply, "the SOCKS5 credential reply")?;
            if reply[1] != 0x00 {
                return Err("The SOCKS5 proxy rejected the username and password".into());
            }
        }
        0xFF => {
            return Err(
                "The SOCKS5 proxy rejected every authentication method Heminus offers".into(),
            );
        }
        method => {
            return Err(format!(
                "The SOCKS5 proxy asked for unsupported method {method}"
            ));
        }
    }

    let host = target_host.as_bytes();
    if host.len() > 255 {
        return Err("The destination hostname is too long for SOCKS5".into());
    }
    let mut request = vec![0x05, 0x01, 0x00];
    match target_host.parse::<std::net::IpAddr>() {
        Ok(std::net::IpAddr::V4(address)) => {
            request.push(0x01);
            request.extend_from_slice(&address.octets());
        }
        Ok(std::net::IpAddr::V6(address)) => {
            request.push(0x04);
            request.extend_from_slice(&address.octets());
        }
        Err(_) => {
            request.push(0x03);
            request.push(host.len() as u8);
            request.extend_from_slice(host);
        }
    }
    request.extend_from_slice(&target_port.to_be_bytes());
    write_all(stream, &request, "the SOCKS5 connect request")?;

    let mut reply = [0_u8; 4];
    read_exact(stream, &mut reply, "the SOCKS5 connect reply")?;
    if reply[1] != 0x00 {
        return Err(format!(
            "The SOCKS5 proxy refused the connection: {}",
            socks5_error(reply[1])
        ));
    }
    // Drain the bound address so the tunnel starts on a clean byte boundary.
    let bound_length = match reply[3] {
        0x01 => 4,
        0x04 => 16,
        0x03 => {
            let mut length = [0_u8; 1];
            read_exact(stream, &mut length, "the SOCKS5 bound address")?;
            usize::from(length[0])
        }
        kind => {
            return Err(format!(
                "The SOCKS5 proxy replied with unknown address type {kind}"
            ));
        }
    };
    let mut bound = vec![0_u8; bound_length + 2];
    read_exact(stream, &mut bound, "the SOCKS5 bound address")?;
    Ok(())
}

const fn socks5_error(code: u8) -> &'static str {
    match code {
        0x01 => "general SOCKS server failure",
        0x02 => "connection not allowed by ruleset",
        0x03 => "network unreachable",
        0x04 => "host unreachable",
        0x05 => "connection refused",
        0x06 => "TTL expired",
        0x07 => "command not supported",
        0x08 => "address type not supported",
        _ => "unknown failure",
    }
}

fn format_authority(host: &str, port: u16) -> String {
    if host.contains(':') && !host.starts_with('[') {
        format!("[{host}]:{port}")
    } else {
        format!("{host}:{port}")
    }
}

fn write_all(stream: &mut TcpStream, bytes: &[u8], what: &str) -> Result<(), String> {
    stream
        .write_all(bytes)
        .and_then(|()| stream.flush())
        .map_err(|error| format!("Could not send {what}: {error}"))
}

fn read_exact(stream: &mut TcpStream, buffer: &mut [u8], what: &str) -> Result<(), String> {
    stream
        .read_exact(buffer)
        .map_err(|error| format!("Could not read {what}: {error}"))
}

/// Copies bytes both ways until either side closes, the way `nc` would.
///
/// Each chunk is flushed immediately: SSH is a request/response protocol, and
/// the buffering `io::copy` would apply to standard output holds the banner
/// back until the transfer ends, which is never.
fn pump(stream: TcpStream) -> Result<(), String> {
    let mut upstream = stream
        .try_clone()
        .map_err(|error| format!("Could not split the proxy socket: {error}"))?;
    let writer = std::thread::Builder::new()
        .name("heminus-proxy-upstream".into())
        .spawn(move || {
            let mut input = std::io::stdin().lock();
            let _ = relay(&mut input, &mut upstream);
            let _ = upstream.shutdown(std::net::Shutdown::Write);
        })
        .map_err(|error| format!("Could not start the proxy tunnel: {error}"))?;

    let mut downstream = stream;
    let mut output = std::io::stdout().lock();
    let relayed = relay(&mut downstream, &mut output);
    let _ = output.flush();
    let _ = downstream.shutdown(std::net::Shutdown::Both);
    let _ = writer.join();
    relayed.map_err(|error| format!("The proxy tunnel stopped: {error}"))
}

fn relay(reader: &mut impl Read, writer: &mut impl Write) -> std::io::Result<()> {
    let mut buffer = vec![0_u8; 32 * 1024];
    loop {
        let read = match reader.read(&mut buffer) {
            Ok(0) => return Ok(()),
            Ok(read) => read,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        };
        writer.write_all(&buffer[..read])?;
        writer.flush()?;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::BufRead;
    use std::net::TcpListener;

    fn spec(kind: ProxyKind, username: Option<&str>) -> ProxySpec {
        ProxySpec {
            kind,
            hostname: "127.0.0.1".into(),
            port: 0,
            username: username.map(str::to_owned),
            host_id: None,
        }
    }

    #[test]
    fn proxy_specs_survive_a_shell_safe_round_trip() {
        let mut spec = spec(ProxyKind::Socks5, Some("agent"));
        spec.host_id = Some(Uuid::new_v4());
        let encoded = spec.encode().unwrap();
        assert!(
            encoded
                .chars()
                .all(|value| value.is_ascii_alphanumeric() || value == '-' || value == '_')
        );
        let decoded = ProxySpec::decode(&encoded).unwrap();
        assert_eq!(decoded.hostname, spec.hostname);
        assert_eq!(decoded.username.as_deref(), Some("agent"));
        assert_eq!(decoded.host_id, spec.host_id);
    }

    #[test]
    fn http_connect_sends_basic_credentials_and_accepts_a_tunnel() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let (connection, _) = listener.accept().unwrap();
            let mut reader = std::io::BufReader::new(connection.try_clone().unwrap());
            let mut request = String::new();
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                if line == "\r\n" || line.is_empty() {
                    break;
                }
                request.push_str(&line);
            }
            let mut connection = connection;
            connection
                .write_all(b"HTTP/1.1 200 Connection established\r\n\r\n")
                .unwrap();
            request
        });

        let mut spec = spec(ProxyKind::Http, Some("agent"));
        spec.port = port;
        let stream = connect_through(&spec, Some("secret"), "10.0.0.5", 22).unwrap();
        drop(stream);
        let request = server.join().unwrap();
        assert!(request.starts_with("CONNECT 10.0.0.5:22 HTTP/1.1\r\n"));
        let credentials = STANDARD.encode("agent:secret");
        assert!(request.contains(&format!("Proxy-Authorization: Basic {credentials}")));
    }

    #[test]
    fn http_connect_reports_a_rejected_password() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            let (mut connection, _) = listener.accept().unwrap();
            let mut buffer = [0_u8; 1024];
            let _ = connection.read(&mut buffer);
            let _ = connection.write_all(
                b"HTTP/1.1 407 Proxy Authentication Required\r\nContent-Length: 0\r\n\r\n",
            );
        });

        let mut spec = spec(ProxyKind::Http, Some("agent"));
        spec.port = port;
        let error = connect_through(&spec, Some("wrong"), "10.0.0.5", 22).unwrap_err();
        assert!(error.contains("407"), "{error}");
    }

    #[test]
    fn socks5_connect_authenticates_then_requests_the_destination() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let (mut connection, _) = listener.accept().unwrap();
            let mut greeting = [0_u8; 4];
            connection.read_exact(&mut greeting[..2]).unwrap();
            let count = usize::from(greeting[1]);
            let mut methods = vec![0_u8; count];
            connection.read_exact(&mut methods).unwrap();
            assert!(methods.contains(&0x02));
            connection.write_all(&[0x05, 0x02]).unwrap();

            let mut header = [0_u8; 2];
            connection.read_exact(&mut header).unwrap();
            let mut username = vec![0_u8; usize::from(header[1])];
            connection.read_exact(&mut username).unwrap();
            let mut password_length = [0_u8; 1];
            connection.read_exact(&mut password_length).unwrap();
            let mut password = vec![0_u8; usize::from(password_length[0])];
            connection.read_exact(&mut password).unwrap();
            connection.write_all(&[0x01, 0x00]).unwrap();

            let mut request = [0_u8; 4];
            connection.read_exact(&mut request).unwrap();
            let mut host_length = [0_u8; 1];
            connection.read_exact(&mut host_length).unwrap();
            let mut host = vec![0_u8; usize::from(host_length[0])];
            connection.read_exact(&mut host).unwrap();
            let mut destination_port = [0_u8; 2];
            connection.read_exact(&mut destination_port).unwrap();
            connection
                .write_all(&[0x05, 0x00, 0x00, 0x01, 0, 0, 0, 0, 0, 0])
                .unwrap();
            (
                String::from_utf8(username).unwrap(),
                String::from_utf8(password).unwrap(),
                String::from_utf8(host).unwrap(),
                u16::from_be_bytes(destination_port),
                request[3],
            )
        });

        let mut spec = spec(ProxyKind::Socks5, Some("agent"));
        spec.port = port;
        let stream = connect_through(&spec, Some("secret"), "private.internal", 2222).unwrap();
        drop(stream);
        let (username, password, host, destination_port, address_type) = server.join().unwrap();
        assert_eq!(username, "agent");
        assert_eq!(password, "secret");
        assert_eq!(host, "private.internal");
        assert_eq!(destination_port, 2222);
        assert_eq!(address_type, 0x03);
    }

    #[test]
    fn the_tunnel_forwards_each_chunk_without_waiting_for_the_stream_to_end() {
        struct Blocking<'a> {
            chunks: &'a [&'a [u8]],
            index: usize,
        }
        impl Read for Blocking<'_> {
            fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
                let Some(chunk) = self.chunks.get(self.index) else {
                    // A live SSH session never reaches EOF while it is in use.
                    return Err(std::io::Error::new(std::io::ErrorKind::WouldBlock, "open"));
                };
                self.index += 1;
                buffer[..chunk.len()].copy_from_slice(chunk);
                Ok(chunk.len())
            }
        }

        #[derive(Default)]
        struct Recorder {
            flushed: Vec<Vec<u8>>,
            pending: Vec<u8>,
        }
        impl Write for Recorder {
            fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
                self.pending.extend_from_slice(buffer);
                Ok(buffer.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                if !self.pending.is_empty() {
                    self.flushed.push(std::mem::take(&mut self.pending));
                }
                Ok(())
            }
        }

        let mut reader = Blocking {
            chunks: &[b"SSH-2.0-OpenSSH_9.6\r\n", &[0x00, 0x01, 0x02]],
            index: 0,
        };
        let mut recorder = Recorder::default();
        let error = relay(&mut reader, &mut recorder).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::WouldBlock);
        assert_eq!(
            recorder.flushed,
            vec![b"SSH-2.0-OpenSSH_9.6\r\n".to_vec(), vec![0x00, 0x01, 0x02]],
            "each chunk must reach SSH before the next read blocks"
        );
    }

    #[test]
    fn a_silent_proxy_fails_the_handshake_instead_of_hanging() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        // Accept the connection, then never reply — the failure mode that used
        // to wedge the ProxyCommand, and with it the terminal, indefinitely.
        let server = std::thread::spawn(move || {
            let (connection, _) = listener.accept().unwrap();
            std::thread::sleep(Duration::from_millis(600));
            drop(connection);
        });

        let mut spec = spec(ProxyKind::Http, None);
        spec.port = port;
        let started = std::time::Instant::now();
        let error =
            connect_through_with_timeout(&spec, None, "10.0.0.5", 22, Duration::from_millis(150))
                .unwrap_err();

        assert!(
            started.elapsed() < Duration::from_secs(5),
            "handshake blocked"
        );
        assert!(
            error.contains("Could not read the proxy response"),
            "{error}"
        );
        server.join().unwrap();
    }

    #[test]
    fn socks5_connect_surfaces_a_refused_destination() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            let (mut connection, _) = listener.accept().unwrap();
            let mut greeting = [0_u8; 2];
            connection.read_exact(&mut greeting).unwrap();
            let mut methods = vec![0_u8; usize::from(greeting[1])];
            connection.read_exact(&mut methods).unwrap();
            connection.write_all(&[0x05, 0x00]).unwrap();
            let mut request = [0_u8; 4];
            connection.read_exact(&mut request).unwrap();
            let mut address = [0_u8; 6];
            connection.read_exact(&mut address).unwrap();
            connection
                .write_all(&[0x05, 0x05, 0x00, 0x01, 0, 0, 0, 0, 0, 0])
                .unwrap();
        });

        let mut spec = spec(ProxyKind::Socks5, None);
        spec.port = port;
        let error = connect_through(&spec, None, "10.0.0.5", 22).unwrap_err();
        assert!(error.contains("connection refused"), "{error}");
    }
}
