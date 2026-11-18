//! One HTTPS GET, as small as the update needs: GitHub's release files, followed through its
//! redirects to its own storage, over TLS checked against the web's usual roots. Through Tor
//! while Tor is on (the app's own, through the same door ssh's connections take), and then
//! never around it; directly otherwise.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

/// How long a connection or a read may stall.
const STALL: Duration = Duration::from_secs(45);
const MOST_REDIRECTS: usize = 5;
/// The most a reply's status line and headers may take.
const MOST_HEAD: usize = 64 * 1024;

/// How to reach the server.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Route {
    Direct,
    /// Through this app's Tor (`sshee_core::tor::app_stream`).
    Tor,
}

impl Route {
    /// Tor's while it's on, as every connection the app makes.
    pub fn now() -> Route {
        if sshee_core::tor::is_on() {
            Route::Tor
        } else {
            Route::Direct
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Url {
    pub host: String,
    pub port: u16,
    /// From the first "/", with its query.
    pub path: String,
}

/// An `https://` address with a plain host name; anything else (another scheme, a user name,
/// spaces) is refused.
pub fn parse_url(url: &str) -> Result<Url, String> {
    let rest = url.strip_prefix("https://").ok_or("only https:// addresses are fetched")?;
    if rest.bytes().any(|b| b.is_ascii_whitespace() || b.is_ascii_control()) {
        return Err("an address with spaces".into());
    }
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let (authority, path) = rest.split_at(end);
    let path = path.split('#').next().unwrap_or_default();
    let path = if path.starts_with('/') { path.to_owned() } else { format!("/{path}") };
    let (host, port) = match authority.rsplit_once(':') {
        Some((host, port)) => (host, port.parse::<u16>().map_err(|_| "a port that isn't one")?),
        None => (authority, 443),
    };
    let host = host.to_ascii_lowercase();
    if host.is_empty() || !host.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-') {
        return Err("a host name that isn't one".into());
    }
    Ok(Url { host, port, path })
}

/// Where a release file may come from: GitHub, and the storage it sends downloads on to.
pub fn allowed_host(host: &str) -> bool {
    host == "github.com" || host.ends_with(".githubusercontent.com")
}

/// Where a redirect from `from` leads.
pub fn resolve(from: &Url, location: &str) -> Result<Url, String> {
    if location.starts_with("https://") {
        parse_url(location)
    } else if location.starts_with('/') && !location.starts_with("//") {
        parse_url(&format!("https://{}:{}{location}", from.host, from.port))
    } else {
        Err("a redirect that leaves https".into())
    }
}

/// What one request got.
#[derive(Debug, PartialEq, Eq)]
pub enum Reply {
    Body(Vec<u8>),
    Redirect(String),
}

/// The body at `url`, at most `most` bytes, through `route`.
pub fn get(url: &str, most: u64, route: Route) -> Result<Vec<u8>, String> {
    let mut at = parse_url(url)?;
    for _ in 0..=MOST_REDIRECTS {
        if !allowed_host(&at.host) {
            return Err(format!("{} isn't GitHub", at.host));
        }
        match fetch(&at, most, route)? {
            Reply::Body(body) => return Ok(body),
            Reply::Redirect(location) => at = resolve(&at, &location)?,
        }
    }
    Err("too many redirects".into())
}

fn tls() -> Arc<rustls::ClientConfig> {
    static CONFIG: OnceLock<Arc<rustls::ClientConfig>> = OnceLock::new();
    CONFIG
        .get_or_init(|| {
            let mut roots = rustls::RootCertStore::empty();
            roots.add_parsable_certificates(webpki_root_certs::TLS_SERVER_ROOT_CERTS.iter().cloned());
            let config = rustls::ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
                .with_safe_default_protocol_versions()
                .expect("ring offers TLS 1.2 and 1.3")
                .with_root_certificates(roots)
                .with_no_client_auth();
            Arc::new(config)
        })
        .clone()
}

fn connect(host: &str, port: u16, route: Route) -> Result<TcpStream, String> {
    match route {
        Route::Tor => sshee_core::tor::app_stream(host, port),
        Route::Direct => {
            let addrs = (host, port).to_socket_addrs().map_err(|e| format!("{host} wasn't found: {e}"))?;
            let mut last = format!("{host} has no address");
            for addr in addrs {
                match TcpStream::connect_timeout(&addr, STALL) {
                    Ok(sock) => return Ok(sock),
                    Err(e) => last = format!("{host} didn't answer: {e}"),
                }
            }
            Err(last)
        }
    }
}

fn fetch(url: &Url, most: u64, route: Route) -> Result<Reply, String> {
    let sock = connect(&url.host, url.port, route)?;
    let _ = sock.set_read_timeout(Some(STALL));
    let _ = sock.set_write_timeout(Some(STALL));
    let name = rustls::pki_types::ServerName::try_from(url.host.clone()).map_err(|e| format!("{}: {e}", url.host))?;
    let conn = rustls::ClientConnection::new(tls(), name).map_err(|e| e.to_string())?;
    let mut stream = rustls::StreamOwned::new(conn, sock);
    // A plain name for the program, as GitHub asks of every client; nothing about this computer.
    let request = format!("GET {} HTTP/1.1\r\nHost: {}\r\nUser-Agent: SSHeesh-update\r\nAccept: */*\r\nAccept-Encoding: identity\r\nConnection: close\r\n\r\n", url.path, url.host);
    stream.write_all(request.as_bytes()).and_then(|_| stream.flush()).map_err(|e| format!("{}: {e}", url.host))?;
    read_reply(&mut BufReader::new(stream), most)
}

/// One line, without its CRLF; `budget` is what the head may still take.
fn line(r: &mut impl BufRead, budget: &mut usize) -> Result<String, String> {
    let mut buf = Vec::new();
    r.by_ref().take(*budget as u64 + 1).read_until(b'\n', &mut buf).map_err(|e| e.to_string())?;
    if buf.len() > *budget || !buf.ends_with(b"\n") {
        return Err("the reply's head is cut off or too long".into());
    }
    *budget -= buf.len();
    let text = String::from_utf8(buf).map_err(|_| "the reply's head isn't text")?;
    Ok(text.trim_end_matches(['\r', '\n']).to_owned())
}

/// Reads a reply: its status, its headers, and its body or where it redirects.
pub fn read_reply(r: &mut impl BufRead, most: u64) -> Result<Reply, String> {
    let mut budget = MOST_HEAD;
    let status = line(r, &mut budget)?;
    let code = status.strip_prefix("HTTP/1.").and_then(|s| s.get(2..5)).and_then(|c| c.parse::<u16>().ok()).ok_or_else(|| "not an HTTP reply".to_owned())?;
    let (mut length, mut chunked, mut location) = (None, false, None);
    loop {
        let header = line(r, &mut budget)?;
        if header.is_empty() {
            break;
        }
        let Some((name, value)) = header.split_once(':') else { continue };
        let value = value.trim();
        match name.trim().to_ascii_lowercase().as_str() {
            "content-length" => length = Some(value.parse::<u64>().map_err(|_| "a length that isn't one")?),
            "transfer-encoding" => chunked = value.to_ascii_lowercase().contains("chunked"),
            "location" => location = Some(value.to_owned()),
            _ => {}
        }
    }
    match code {
        200 => {}
        301 | 302 | 303 | 307 | 308 => return location.map(Reply::Redirect).ok_or_else(|| "a redirect to nowhere".into()),
        404 => return Err("not published (404)".into()),
        other => return Err(format!("the server answered {other}")),
    }
    let too_big = || format!("more than {most} bytes");
    let body = if chunked {
        let mut body = Vec::new();
        loop {
            let size = line(r, &mut 1024)?;
            let size = u64::from_str_radix(size.split(';').next().unwrap_or_default().trim(), 16).map_err(|_| "a chunk size that isn't one")?;
            if size == 0 {
                // Trailers, then the empty line.
                let mut budget = MOST_HEAD;
                while !line(r, &mut budget)?.is_empty() {}
                break;
            }
            if body.len() as u64 + size > most {
                return Err(too_big());
            }
            let start = body.len();
            body.resize(start + size as usize, 0);
            r.read_exact(&mut body[start..]).map_err(|e| format!("the download stopped: {e}"))?;
            if !line(r, &mut 2)?.is_empty() {
                return Err("a chunk without its end".into());
            }
        }
        body
    } else if let Some(length) = length {
        if length > most {
            return Err(too_big());
        }
        let mut body = vec![0; length as usize];
        r.read_exact(&mut body).map_err(|e| format!("the download stopped: {e}"))?;
        body
    } else {
        let mut body = Vec::new();
        match r.take(most + 1).read_to_end(&mut body) {
            Ok(_) => {}
            // TLS closed without its goodbye: what came is what was sent, and the hash says
            // whether it's whole.
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {}
            Err(e) => return Err(format!("the download stopped: {e}")),
        }
        if body.len() as u64 > most {
            return Err(too_big());
        }
        body
    };
    Ok(Reply::Body(body))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reply(text: &str, most: u64) -> Result<Reply, String> {
        read_reply(&mut std::io::Cursor::new(text.as_bytes().to_vec()), most)
    }

    #[test]
    fn addresses_are_https_on_a_plain_host() {
        let u = parse_url("https://github.com/footprintsntrails/ssheesh/releases/download/canary/canary.json").unwrap();
        assert_eq!((u.host.as_str(), u.port, u.path.as_str()), ("github.com", 443, "/footprintsntrails/ssheesh/releases/download/canary/canary.json"));
        let u = parse_url("https://Release-Assets.githubusercontent.com:8443?x=1#frag").unwrap();
        assert_eq!((u.host.as_str(), u.port, u.path.as_str()), ("release-assets.githubusercontent.com", 8443, "/?x=1"));
        for bad in ["http://github.com/a", "https://user@github.com/a", "https://git hub.com/", "https://github.com:x/", "https:///a", "ftp://github.com"] {
            assert!(parse_url(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn redirects_stay_on_https_and_on_github() {
        let from = parse_url("https://github.com/a").unwrap();
        assert_eq!(resolve(&from, "/b?c").unwrap().path, "/b?c");
        assert_eq!(resolve(&from, "https://objects.githubusercontent.com/x").unwrap().host, "objects.githubusercontent.com");
        assert!(resolve(&from, "http://github.com/b").is_err());
        assert!(resolve(&from, "//elsewhere.example/b").is_err());
        assert!(allowed_host("github.com") && allowed_host("release-assets.githubusercontent.com"));
        assert!(!allowed_host("githubusercontent.com.example") && !allowed_host("evilgithub.com") && !allowed_host("example.com"));
    }

    #[test]
    fn a_body_reads_by_length_by_chunks_or_to_the_end() {
        assert_eq!(reply("HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\nhello and more", 100), Ok(Reply::Body(b"hello".to_vec())));
        assert_eq!(reply("HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n5;x=y\r\nhello\r\n6\r\n world\r\n0\r\nX-T: 1\r\n\r\n", 100), Ok(Reply::Body(b"hello world".to_vec())));
        assert_eq!(reply("HTTP/1.0 200 OK\r\n\r\nto the end", 100), Ok(Reply::Body(b"to the end".to_vec())));
        assert_eq!(reply("HTTP/1.1 302 Found\r\nLocation: https://objects.githubusercontent.com/a\r\n\r\n", 100), Ok(Reply::Redirect("https://objects.githubusercontent.com/a".into())));
    }

    #[test]
    fn too_much_or_too_little_is_refused() {
        assert!(reply("HTTP/1.1 200 OK\r\nContent-Length: 500\r\n\r\nhello", 100).is_err());
        assert!(reply("HTTP/1.1 200 OK\r\nContent-Length: 50\r\n\r\nhello", 100).is_err());
        assert!(reply("HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n65\r\n", 100).is_err());
        assert!(reply("HTTP/1.1 200 OK\r\n\r\n0123456789", 5).is_err());
        assert!(reply("HTTP/1.1 404 Not Found\r\n\r\n", 100).is_err());
        assert!(reply("HTTP/1.1 302 Found\r\n\r\n", 100).is_err());
        assert!(reply("SSH-2.0-OpenSSH\r\n\r\n", 100).is_err());
        let long = format!("HTTP/1.1 200 OK\r\nX: {}\r\n\r\n", "a".repeat(MOST_HEAD));
        assert!(reply(&long, 100).is_err());
    }
}
