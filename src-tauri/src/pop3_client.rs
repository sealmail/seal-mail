//! 极简 POP3 over TLS 客户端（USER/PASS/STAT/UIDL/RETR/QUIT）。
//! POP3 没有服务器目录，目录功能由本地虚拟目录实现（见 store.rs::local_assign）。

use crate::models::*;
use native_tls::{TlsConnector, TlsStream};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;

pub struct Pop3Client {
    reader: BufReader<TlsStream<TcpStream>>,
}

impl Pop3Client {
    pub fn connect(account: &Account, secret: &AccountSecret) -> Result<Self, String> {
        let tls = TlsConnector::new().map_err(|e| e.to_string())?;
        let tcp = TcpStream::connect((account.incoming_host.as_str(), account.incoming_port))
            .map_err(|e| format!("无法连接 {}:{} — {}", account.incoming_host, account.incoming_port, e))?;
        tcp.set_read_timeout(Some(std::time::Duration::from_secs(30))).ok();
        let stream = tls
            .connect(&account.incoming_host, tcp)
            .map_err(|e| format!("TLS 握手失败: {}", e))?;
        let mut c = Pop3Client { reader: BufReader::new(stream) };
        c.read_line_ok()?; // 服务器问候
        c.cmd(&format!("USER {}", account.username))?;
        c.cmd(&format!("PASS {}", secret.password))
            .map_err(|e| format!("POP3 登录失败（请检查密码或应用专用密码）: {}", e))?;
        Ok(c)
    }

    fn write_cmd(&mut self, cmd: &str) -> Result<(), String> {
        self.reader
            .get_mut()
            .write_all(format!("{}\r\n", cmd).as_bytes())
            .map_err(|e| e.to_string())
    }

    fn read_line_ok(&mut self) -> Result<String, String> {
        let mut line = String::new();
        self.reader.read_line(&mut line).map_err(|e| e.to_string())?;
        if line.starts_with("+OK") {
            Ok(line.trim().to_string())
        } else {
            Err(line.trim().to_string())
        }
    }

    fn cmd(&mut self, cmd: &str) -> Result<String, String> {
        self.write_cmd(cmd)?;
        self.read_line_ok()
    }

    /// 多行响应：读到单独一行 "." 为止，并做 dot-unstuffing
    fn read_multiline(&mut self) -> Result<Vec<u8>, String> {
        let mut out = Vec::new();
        loop {
            let mut line = Vec::new();
            let mut byte = [0u8; 1];
            loop {
                self.reader.read_exact(&mut byte).map_err(|e| e.to_string())?;
                line.push(byte[0]);
                if byte[0] == b'\n' {
                    break;
                }
            }
            if line == b".\r\n" || line == b".\n" {
                break;
            }
            if line.starts_with(b"..") {
                out.extend_from_slice(&line[1..]);
            } else {
                out.extend_from_slice(&line);
            }
        }
        Ok(out)
    }

    pub fn message_count(&mut self) -> Result<u32, String> {
        let resp = self.cmd("STAT")?; // "+OK n size"
        resp.split_whitespace()
            .nth(1)
            .and_then(|s| s.parse().ok())
            .ok_or_else(|| format!("STAT 响应异常: {}", resp))
    }

    pub fn retrieve(&mut self, n: u32) -> Result<Vec<u8>, String> {
        self.cmd(&format!("RETR {}", n))?;
        self.read_multiline()
    }

    pub fn delete(&mut self, n: u32) -> Result<(), String> {
        self.cmd(&format!("DELE {}", n)).map(|_| ())
    }

    pub fn quit(&mut self) {
        let _ = self.write_cmd("QUIT");
        let mut line = String::new();
        let _ = self.reader.read_line(&mut line);
    }
}

pub struct RawPopMail {
    pub seq: u32,
    pub raw: Vec<u8>,
}

pub fn fetch_window(
    account: &Account,
    secret: &AccountSecret,
    limit: u32,
) -> Result<Vec<RawPopMail>, String> {
    let mut c = Pop3Client::connect(account, secret)?;
    let count = c.message_count()?;
    let start = count.saturating_sub(limit.saturating_sub(1)).max(1);
    let mut out = Vec::new();
    if count > 0 {
        for n in (start..=count).rev() {
            match c.retrieve(n) {
                Ok(raw) => out.push(RawPopMail { seq: n, raw }),
                Err(_) => continue,
            }
        }
    }
    c.quit();
    Ok(out)
}

pub fn delete_message(account: &Account, secret: &AccountSecret, seq: u32) -> Result<(), String> {
    let mut c = Pop3Client::connect(account, secret)?;
    c.delete(seq)?;
    c.quit();
    Ok(())
}
