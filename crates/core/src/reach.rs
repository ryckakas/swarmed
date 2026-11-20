//! Where a running stack is reached, and whether it answers yet.
//!
//! A `start` command is "running" from the moment it is started, long before a dev server is
//! up: it may still be downloading what it needs. What tells the two apart is whether anything
//! answers at the stack's address: the project's `open_url`, or, for a project that names
//! none, the addresses on this machine that the stack printed.

use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr, TcpStream};
use std::time::Duration;

/// How long one address is given to answer. A port on this machine answers or refuses at once.
const ANSWER_WITHIN: Duration = Duration::from_millis(200);

/// How many printed addresses are kept: a stack of several services prints several.
const MAX_FOUND: usize = 5;

/// An address on this machine: how it is opened, and the port that tells whether it answers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Local {
    pub(crate) url: String,
    port: u16,
}

impl Local {
    /// Whether something listens there, on either of this machine's own addresses.
    pub(crate) fn answers(&self) -> bool {
        let own: [SocketAddr; 2] = [
            (Ipv4Addr::LOCALHOST, self.port).into(),
            (Ipv6Addr::LOCALHOST, self.port).into(),
        ];
        own.iter()
            .any(|address| TcpStream::connect_timeout(address, ANSWER_WITHIN).is_ok())
    }
}

/// Reads `url` as an address on this machine, or `None` if it is anywhere else or no web
/// address at all. An address that means "every interface" is opened as `localhost`.
pub(crate) fn local(url: &str) -> Option<Local> {
    let (scheme, rest) = url.split_once("://")?;
    let default_port = match scheme.to_ascii_lowercase().as_str() {
        "http" => 80,
        "https" => 443,
        _ => return None,
    };
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let (authority, path) = rest.split_at(end);
    if authority.contains('@') {
        return None;
    }
    let (host, port) = match authority.rsplit_once(':') {
        Some((host, port)) if !port.contains(']') => (host, port.parse().ok()?),
        _ => (authority, default_port),
    };
    let host = host.to_ascii_lowercase();
    let everywhere = matches!(host.as_str(), "0.0.0.0" | "[::]");
    let here = matches!(host.as_str(), "localhost" | "127.0.0.1" | "[::1]")
        || host.ends_with(".localhost");
    if !(everywhere || here) {
        return None;
    }
    let host = if everywhere { "localhost" } else { &host };
    let scheme = scheme.to_ascii_lowercase();
    let port_part = if port == default_port {
        String::new()
    } else {
        format!(":{port}")
    };
    Some(Local {
        url: format!("{scheme}://{host}{port_part}{path}"),
        port,
    })
}

/// The addresses on this machine that `log` names, in the order they first appear.
pub(crate) fn found_in(log: &str) -> Vec<Local> {
    let plain = without_colours(log);
    let mut found: Vec<Local> = Vec::new();
    for (start, _) in plain.match_indices("http") {
        let text = &plain[start..];
        let end = text
            .find(|c: char| c.is_whitespace() || "\"'<>()[]{},|`".contains(c))
            .unwrap_or(text.len());
        let Some(local) = local(text[..end].trim_end_matches(['.', ':', ';'])) else {
            continue;
        };
        if found.len() < MAX_FOUND && !found.iter().any(|known| known.port == local.port) {
            found.push(local);
        }
    }
    found
}

/// `text` without the escape sequences terminals colour it with. Dev servers print a port in
/// bold in the middle of an address, which would otherwise cut it in two.
fn without_colours(text: &str) -> String {
    let mut plain = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c != '\u{1b}' {
            plain.push(c);
            continue;
        }
        if chars.next() == Some('[') {
            // Parameters, then one final letter.
            chars.by_ref().find(|c| ('@'..='~').contains(c));
        }
    }
    plain
}

#[cfg(test)]
mod tests {
    use std::net::TcpListener;

    use super::*;

    fn urls(log: &str) -> Vec<String> {
        found_in(log).into_iter().map(|local| local.url).collect()
    }

    #[test]
    fn only_addresses_on_this_machine_are_read() {
        assert_eq!(
            local("http://LOCALHOST:3000/app").map(|l| l.url).as_deref(),
            Some("http://localhost:3000/app")
        );
        assert_eq!(
            local("http://0.0.0.0:8080").map(|l| l.url).as_deref(),
            Some("http://localhost:8080")
        );
        assert_eq!(
            local("https://one.localhost").map(|l| (l.url, l.port)),
            Some(("https://one.localhost".to_string(), 443))
        );
        assert_eq!(local("http://[::1]:5173/").map(|l| l.port), Some(5173));
        for elsewhere in [
            "https://example.com",
            "http://localhost.example.com",
            "http://user@localhost:1",
            "ftp://localhost",
            "localhost:3000",
            "http://localhost:notaport",
        ] {
            assert_eq!(local(elsewhere), None, "{elsewhere}");
        }
    }

    #[test]
    fn addresses_are_found_in_what_a_dev_server_prints() {
        let log = "installing…\n  \u{1b}[32m➜\u{1b}[39m  Local:   \u{1b}[36mhttp://localhost:\u{1b}[1m5173\u{1b}[22m/\u{1b}[39m\n\
                   see https://vite.dev/config for more\n\
                   mail at http://127.0.0.1:8025.\n\
                   again (http://localhost:5173/)\n";
        assert_eq!(
            urls(log),
            ["http://localhost:5173/", "http://127.0.0.1:8025"]
        );
        assert_eq!(urls("nothing here"), Vec::<String>::new());
    }

    #[test]
    fn an_address_answers_only_while_something_listens_there() {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("binds");
        let port = listener.local_addr().expect("an address").port();
        let here = local(&format!("http://localhost:{port}")).expect("local");
        assert!(here.answers());
        // Port 1 is the long-retired tcpmux: nothing listens there, and no test can be handed
        // it, as one could a port that was only just let go.
        assert!(!local("http://localhost:1").expect("local").answers());
    }
}
