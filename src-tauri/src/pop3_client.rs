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
            .map_err(|e| {
                format!(
                    "无法连接 {}:{} — {}",
                    account.incoming_host, account.incoming_port, e
                )
            })?;
        tcp.set_read_timeout(Some(std::time::Duration::from_secs(30)))
            .ok();
        let stream = tls
            .connect(&account.incoming_host, tcp)
            .map_err(|e| format!("TLS 握手失败: {}", e))?;
        let mut c = Pop3Client {
            reader: BufReader::new(stream),
        };
        c.read_line_ok()?; // 服务器问候
        if let Some(oauth) = &secret.oauth {
            // RFC 5034 SASL 带初始响应：AUTH XOAUTH2 <base64>
            use base64::{engine::general_purpose::STANDARD as B64, Engine};
            let b64 = B64.encode(crate::oauth::xoauth2_string(
                &account.username,
                &oauth.access_token,
            ));
            if let Err(e) = c.cmd(&format!("AUTH XOAUTH2 {}", b64)) {
                // 服务器可能用 "+ <base64>" 继续挑战携带错误详情：回空行取最终 -ERR
                if e.starts_with('+') {
                    let _ = c.write_cmd("");
                    let _ = c.read_line_ok();
                }
                return Err(format!(
                    "POP3 OAuth2 登录失败（授权可能已失效，请重新授权）: {}",
                    e
                ));
            }
        } else {
            c.cmd(&format!("USER {}", account.username))?;
            c.cmd(&format!("PASS {}", secret.password))
                .map_err(|e| format!("POP3 登录失败（请检查密码或应用专用密码）: {}", e))?;
        }
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
        self.reader
            .read_line(&mut line)
            .map_err(|e| e.to_string())?;
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
                self.reader
                    .read_exact(&mut byte)
                    .map_err(|e| e.to_string())?;
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

    /// UIDL 列表：(seq, uidl)。uidl 是跨会话稳定的邮件标识
    pub fn uidl(&mut self) -> Result<Vec<(u32, String)>, String> {
        self.cmd("UIDL")?;
        let body = self.read_multiline()?;
        let text = String::from_utf8_lossy(&body);
        let mut out = Vec::new();
        for line in text.lines() {
            let mut parts = line.split_whitespace();
            let (Some(seq), Some(uidl)) = (parts.next(), parts.next()) else {
                continue;
            };
            let seq: u32 = seq
                .parse()
                .map_err(|_| format!("UIDL 响应异常: {}", line))?;
            out.push((seq, uidl.to_string()));
        }
        Ok(out)
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

/// 一次 POP3 增量同步的结果
pub struct PopSync {
    /// 新邮件 (uidl, raw)
    pub new_mails: Vec<(String, Vec<u8>)>,
    /// 服务器现存全部 uidl（用于检测服务器侧删除）
    pub all_uidls: Vec<String>,
}

/// 按 UIDL 增量拉取：known 里没有的才下载；首次（known 为空）只取最近 initial_window 封
pub fn sync_fetch(
    account: &Account,
    secret: &AccountSecret,
    known: &std::collections::HashSet<String>,
    initial_window: u32,
) -> Result<PopSync, String> {
    let mut c = Pop3Client::connect(account, secret)?;
    let list = c.uidl()?;
    let all_uidls: Vec<String> = list.iter().map(|(_, u)| u.clone()).collect();
    let mut unknown: Vec<&(u32, String)> =
        list.iter().filter(|(_, u)| !known.contains(u)).collect();
    if known.is_empty() && unknown.len() > initial_window as usize {
        // 首次同步只取最近一窗，避免一口气下完整个邮箱
        unknown = unknown.split_off(unknown.len() - initial_window as usize);
    }
    let mut new_mails = Vec::new();
    for (seq, uidl) in unknown {
        match c.retrieve(*seq) {
            Ok(raw) => new_mails.push((uidl.clone(), raw)),
            Err(e) => eprintln!("[pop3] RETR {} 失败: {}", seq, e),
        }
    }
    c.quit();
    Ok(PopSync {
        new_mails,
        all_uidls,
    })
}

/// POP3 没有目录分页语义；这里从尚未缓存的 UIDL 里取相对最新的一批。
pub fn fetch_unknown_window(
    account: &Account,
    secret: &AccountSecret,
    known: &std::collections::HashSet<String>,
    batch: u32,
) -> Result<PopSync, String> {
    let mut c = Pop3Client::connect(account, secret)?;
    let list = c.uidl()?;
    let all_uidls: Vec<String> = list.iter().map(|(_, u)| u.clone()).collect();
    let unknown: Vec<&(u32, String)> = list.iter().filter(|(_, u)| !known.contains(u)).collect();
    let start = unknown.len().saturating_sub(batch as usize);
    let mut new_mails = Vec::new();
    for (seq, uidl) in &unknown[start..] {
        match c.retrieve(*seq) {
            Ok(raw) => new_mails.push((uidl.clone(), raw)),
            Err(e) => eprintln!("[pop3] RETR {} 失败: {}", seq, e),
        }
    }
    c.quit();
    Ok(PopSync {
        new_mails,
        all_uidls,
    })
}

/// 按 UIDL 拉取单封原文（附件下载用；seq 跨会话不稳定，必须用 uidl 定位）
pub fn fetch_raw_by_uidl(
    account: &Account,
    secret: &AccountSecret,
    uidl: &str,
) -> Result<Vec<u8>, String> {
    let mut c = Pop3Client::connect(account, secret)?;
    let seq = c
        .uidl()?
        .into_iter()
        .find(|(_, u)| u == uidl)
        .map(|(s, _)| s)
        .ok_or("邮件已不在服务器上")?;
    let raw = c.retrieve(seq)?;
    c.quit();
    Ok(raw)
}

/// 按 UIDL 物理删除
pub fn delete_by_uidl(account: &Account, secret: &AccountSecret, uidl: &str) -> Result<(), String> {
    let mut c = Pop3Client::connect(account, secret)?;
    let seq = c
        .uidl()?
        .into_iter()
        .find(|(_, u)| u == uidl)
        .map(|(s, _)| s)
        .ok_or("邮件已不在服务器上")?;
    c.delete(seq)?;
    c.quit();
    Ok(())
}
