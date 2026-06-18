use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

fn require_env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("{name} must be set for mail server smoke tests"))
}

fn assert_tcp_ready(name: &str, host: &str, port: u16) {
    let addr = (host, port)
        .to_socket_addrs()
        .expect("mail server address should resolve")
        .next()
        .expect("mail server address should have at least one socket address");
    TcpStream::connect_timeout(&addr, Duration::from_secs(5))
        .unwrap_or_else(|e| panic!("{name} should accept TCP connections on {host}:{port}: {e}"));
}

#[test]
#[ignore = "requires a Docker-backed local mail server configured by CI"]
fn configured_mail_server_ports_are_reachable() {
    let host = require_env("SEALMAIL_TEST_MAIL_HOST");
    let smtp: u16 = require_env("SEALMAIL_TEST_SMTP_PORT")
        .parse()
        .expect("SMTP port should be numeric");
    let imap: u16 = require_env("SEALMAIL_TEST_IMAP_PORT")
        .parse()
        .expect("IMAP port should be numeric");
    let pop3: u16 = require_env("SEALMAIL_TEST_POP3_PORT")
        .parse()
        .expect("POP3 port should be numeric");

    assert_tcp_ready("SMTP", &host, smtp);
    assert_tcp_ready("IMAP", &host, imap);
    assert_tcp_ready("POP3", &host, pop3);
}
