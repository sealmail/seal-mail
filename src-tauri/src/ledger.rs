//! Ledger 硬件密钥支持（USB-HID，纯 Rust 协议层 + hidapi I/O）。
//!
//! 参考 auto-desktop 的实现：WKWebView 没有 WebHID，所以从 Rust 直连设备。
//! SealMail 只用到 Ethereum app 的两条 APDU：
//!   - GET ADDRESS：绑定身份时取地址
//!   - SIGN PERSONAL MESSAGE（EIP-191 personal_sign）：对邮件 canon 字符串签名
//! 私钥永不出设备：发送 payload，用户在设备上确认，设备只返回签名。
//! 确定性层（framing / path / APDU / 解析）有单元测试；hidapi I/O 需要实机。

use hidapi::{HidApi, HidDevice};

const LEDGER_VID: u16 = 0x2c97;
/// Ethereum app 的 APDU HID interface 的 usage page。
const APDU_USAGE_PAGE: u16 = 0xffa0;

const CLA: u8 = 0xe0;
const INS_GET_ADDRESS: u8 = 0x02;
const INS_SIGN_PERSONAL: u8 = 0x08;

const CHANNEL: u16 = 0x0101;
const TAG: u8 = 0x05;
const PACKET: usize = 64;
const MAX_APDU_DATA: usize = 255;

const HARDENED: u32 = 0x8000_0000;

/// 解析 `m/44'/60'/0'/0/0`（或省略 `m/`）为 BIP-32 路径分量。
fn parse_bip32_path(path: &str) -> Result<Vec<u32>, String> {
    let trimmed = path.trim();
    let body = trimmed
        .strip_prefix("m/")
        .or_else(|| trimmed.strip_prefix("M/"))
        .unwrap_or(trimmed);
    let mut comps = Vec::new();
    for part in body.split('/') {
        if part.is_empty() {
            continue;
        }
        let (num, hardened) = match part
            .strip_suffix('\'')
            .or_else(|| part.strip_suffix('h'))
            .or_else(|| part.strip_suffix('H'))
        {
            Some(n) => (n, true),
            None => (part, false),
        };
        let n: u32 = num.parse().map_err(|_| format!("路径分量无效 '{part}'"))?;
        if n >= HARDENED {
            return Err(format!("路径索引越界: {part}"));
        }
        comps.push(if hardened { n | HARDENED } else { n });
    }
    if comps.is_empty() || comps.len() > 10 {
        return Err(format!("派生路径必须为 1–10 个分量: {path}"));
    }
    Ok(comps)
}

/// 路径的 APDU 编码：`count(1) ‖ component(4 BE)…`
fn path_apdu_bytes(path: &str) -> Result<Vec<u8>, String> {
    let comps = parse_bip32_path(path)?;
    let mut out = Vec::with_capacity(1 + comps.len() * 4);
    out.push(comps.len() as u8);
    for c in comps {
        out.extend_from_slice(&c.to_be_bytes());
    }
    Ok(out)
}

/// Ledger Live 第 `index` 个账户的标准路径。
pub fn ledger_live_path(index: u32) -> String {
    format!("m/44'/60'/{index}'/0/0")
}

fn apdu(ins: u8, p1: u8, p2: u8, data: &[u8]) -> Result<Vec<u8>, String> {
    if data.len() > MAX_APDU_DATA {
        return Err("APDU 数据超过 255 字节".to_string());
    }
    let mut a = Vec::with_capacity(5 + data.len());
    a.extend_from_slice(&[CLA, ins, p1, p2, data.len() as u8]);
    a.extend_from_slice(data);
    Ok(a)
}

/// 把 APDU 切成 64 字节 HID 报文（首包带 2 字节总长）。
fn frame(apdu: &[u8]) -> Vec<[u8; PACKET]> {
    let mut packets = Vec::new();
    let mut seq: u16 = 0;
    let mut offset = 0;
    loop {
        let mut pkt = [0u8; PACKET];
        pkt[0..2].copy_from_slice(&CHANNEL.to_be_bytes());
        pkt[2] = TAG;
        pkt[3..5].copy_from_slice(&seq.to_be_bytes());
        let hdr = if seq == 0 {
            pkt[5..7].copy_from_slice(&(apdu.len() as u16).to_be_bytes());
            7
        } else {
            5
        };
        let take = (PACKET - hdr).min(apdu.len() - offset);
        pkt[hdr..hdr + take].copy_from_slice(&apdu[offset..offset + take]);
        packets.push(pkt);
        offset += take;
        seq += 1;
        if offset >= apdu.len() {
            break;
        }
    }
    packets
}

/// 重组 HID 报文为响应（校验 channel/tag/序号）。返回 数据 ‖ SW1 ‖ SW2。
fn deframe(packets: &[Vec<u8>]) -> Result<Vec<u8>, String> {
    let mut data = Vec::new();
    let mut total: Option<usize> = None;
    for (i, pkt) in packets.iter().enumerate() {
        if pkt.len() < 5 {
            return Err("HID 报文过短".to_string());
        }
        if u16::from_be_bytes([pkt[0], pkt[1]]) != CHANNEL {
            return Err("HID 报文 channel 不符".to_string());
        }
        if pkt[2] != TAG {
            return Err("HID 报文 tag 不符".to_string());
        }
        if u16::from_be_bytes([pkt[3], pkt[4]]) != i as u16 {
            return Err("HID 报文乱序".to_string());
        }
        let hdr = if i == 0 {
            if pkt.len() < 7 {
                return Err("HID 首包过短".to_string());
            }
            total = Some(u16::from_be_bytes([pkt[5], pkt[6]]) as usize);
            7
        } else {
            5
        };
        let want = total.ok_or("缺少长度前缀")?;
        let remaining = want - data.len();
        let take = remaining.min(pkt.len() - hdr);
        data.extend_from_slice(&pkt[hdr..hdr + take]);
        if data.len() >= want {
            break;
        }
    }
    match total {
        Some(t) if data.len() == t => Ok(data),
        _ => Err("HID 响应不完整".to_string()),
    }
}

/// 校验 2 字节状态字（0x9000 成功），返回数据部分。
fn check_sw(resp: &[u8]) -> Result<&[u8], String> {
    if resp.len() < 2 {
        return Err("Ledger 响应过短".to_string());
    }
    let sw = u16::from_be_bytes([resp[resp.len() - 2], resp[resp.len() - 1]]);
    if sw == 0x9000 {
        Ok(&resp[..resp.len() - 2])
    } else {
        Err(map_sw(sw))
    }
}

fn map_sw(sw: u16) -> String {
    match sw {
        0x6985 | 0x5501 => "已在 Ledger 设备上拒绝签名。".to_string(),
        0x6a80 | 0x6a87 => {
            "Ledger 拒绝了数据——请在 Ethereum app 设置里开启 blind signing。".to_string()
        }
        0x6700 | 0x6d00 | 0x6e00 | 0x6e01 | 0x6511 => {
            "请在 Ledger 上打开 Ethereum app 后重试。".to_string()
        }
        0x6982 | 0x5515 => "请先解锁 Ledger 并打开 Ethereum app。".to_string(),
        other => format!("Ledger 错误 0x{other:04x}"),
    }
}

/// 解析 GET ADDRESS 响应：`pubkeyLen(1) ‖ pubkey ‖ addrLen(1) ‖ addrAscii`。
fn parse_address_response(data: &[u8]) -> Result<String, String> {
    let pk_len = *data.first().ok_or("地址响应为空")? as usize;
    let addr_len_idx = 1 + pk_len;
    let addr_len = *data.get(addr_len_idx).ok_or("地址响应被截断")? as usize;
    let start = addr_len_idx + 1;
    let ascii = data.get(start..start + addr_len).ok_or("地址响应被截断")?;
    let s = std::str::from_utf8(ascii).map_err(|_| "Ledger 返回非 UTF-8 地址")?;
    let s = s.strip_prefix("0x").unwrap_or(s);
    if s.len() != 40 || !s.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(format!("Ledger 返回的地址格式异常: {s}"));
    }
    Ok(format!("0x{}", s.to_lowercase()))
}

/// 解析签名响应 `v(1) ‖ r(32) ‖ s(32)` → 65 字节 `r ‖ s ‖ v(27/28)`。
fn parse_signature_rsv(data: &[u8]) -> Result<[u8; 65], String> {
    if data.len() < 65 {
        return Err("Ledger 签名响应过短".to_string());
    }
    let v = if data[0] < 27 { data[0] + 27 } else { data[0] };
    let mut out = [0u8; 65];
    out[..32].copy_from_slice(&data[1..33]);
    out[32..64].copy_from_slice(&data[33..65]);
    out[64] = v;
    Ok(out)
}

// ───────────────────────── 设备 I/O（需要实机） ─────────────────────────

fn open_ledger(api: &HidApi) -> Result<HidDevice, String> {
    let mut fallback: Option<&hidapi::DeviceInfo> = None;
    let mut apdu_iface: Option<&hidapi::DeviceInfo> = None;
    for info in api.device_list() {
        if info.vendor_id() != LEDGER_VID {
            continue;
        }
        if info.usage_page() == APDU_USAGE_PAGE {
            apdu_iface = Some(info);
            break;
        }
        fallback.get_or_insert(info);
    }
    let info = apdu_iface
        .or(fallback)
        .ok_or("未找到 Ledger。请插入设备、解锁并打开 Ethereum app。")?;
    info.open_device(api)
        .map_err(|e| format!("无法打开 Ledger 设备: {e}"))
}

fn exchange(device: &HidDevice, apdu_bytes: &[u8]) -> Result<Vec<u8>, String> {
    for pkt in frame(apdu_bytes) {
        let mut buf = Vec::with_capacity(1 + PACKET);
        buf.push(0x00); // HID report id
        buf.extend_from_slice(&pkt);
        device
            .write(&buf)
            .map_err(|e| format!("写入 Ledger 失败: {e}"))?;
    }
    let mut packets: Vec<Vec<u8>> = Vec::new();
    loop {
        let mut rbuf = [0u8; PACKET];
        let n = device
            .read_timeout(&mut rbuf, 60_000)
            .map_err(|e| format!("读取 Ledger 失败: {e}"))?;
        if n == 0 {
            return Err("Ledger 超时——请在设备上确认或取消。".to_string());
        }
        packets.push(rbuf[..n].to_vec());
        if let Ok(done) = deframe(&packets) {
            return check_sw(&done).map(|d| d.to_vec());
        }
    }
}

/// 多 APDU 签名命令：首块带路径（P1=0x00），后续只带 payload（P1=0x80）。
fn exchange_signing(
    device: &HidDevice,
    ins: u8,
    path_bytes: &[u8],
    tail: &[u8],
) -> Result<Vec<u8>, String> {
    let first_tail = (MAX_APDU_DATA - path_bytes.len()).min(tail.len());
    let mut chunks: Vec<Vec<u8>> = Vec::new();
    let mut data0 = path_bytes.to_vec();
    data0.extend_from_slice(&tail[..first_tail]);
    chunks.push(data0);
    let mut off = first_tail;
    while off < tail.len() {
        let n = MAX_APDU_DATA.min(tail.len() - off);
        chunks.push(tail[off..off + n].to_vec());
        off += n;
    }
    let mut last = Vec::new();
    for (i, chunk) in chunks.iter().enumerate() {
        let p1 = if i == 0 { 0x00 } else { 0x80 };
        last = exchange(device, &apdu(ins, p1, 0x00, chunk)?)?;
    }
    Ok(last)
}

/// 一次设备会话取多个 Ledger Live 账户地址（绑定身份时选择用）。
pub fn get_addresses(indices: &[u32]) -> Result<Vec<(u32, String, String)>, String> {
    let api = HidApi::new().map_err(|e| format!("HID 初始化失败: {e}"))?;
    let device = open_ledger(&api)?;
    let mut out = Vec::with_capacity(indices.len());
    for &i in indices {
        let path = ledger_live_path(i);
        let resp = exchange(
            &device,
            &apdu(INS_GET_ADDRESS, 0x00, 0x00, &path_apdu_bytes(&path)?)?,
        )?;
        out.push((i, path.clone(), parse_address_response(&resp)?));
    }
    Ok(out)
}

/// 在设备上对 EIP-191 personal message 签名。返回 65 字节 `r‖s‖v`。
pub fn sign_personal_message(path: &str, message: &[u8]) -> Result<[u8; 65], String> {
    let api = HidApi::new().map_err(|e| format!("HID 初始化失败: {e}"))?;
    let device = open_ledger(&api)?;
    let mut tail = (message.len() as u32).to_be_bytes().to_vec();
    tail.extend_from_slice(message);
    let resp = exchange_signing(&device, INS_SIGN_PERSONAL, &path_apdu_bytes(path)?, &tail)?;
    parse_signature_rsv(&resp)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_paths() {
        let comps = parse_bip32_path("m/44'/60'/0'/0/0").unwrap();
        assert_eq!(comps, vec![44 | HARDENED, 60 | HARDENED, HARDENED, 0, 0]);
        let bytes = path_apdu_bytes("m/44'/60'/0'/0/0").unwrap();
        assert_eq!(
            hex::encode(&bytes),
            "05".to_string() + "8000002c" + "8000003c" + "80000000" + "00000000" + "00000000"
        );
        assert_eq!(ledger_live_path(3), "m/44'/60'/3'/0/0");
        assert!(parse_bip32_path("m/").is_err());
        assert!(parse_bip32_path("m/44'/xyz/0").is_err());
    }

    #[test]
    fn apdu_and_framing_roundtrip() {
        let a = apdu(INS_GET_ADDRESS, 0x00, 0x00, &[0x01, 0x02, 0x03]).unwrap();
        assert_eq!(a[..5], [0xe0, 0x02, 0x00, 0x00, 0x03]);
        assert!(apdu(INS_SIGN_PERSONAL, 0, 0, &vec![0u8; 256]).is_err());

        // 跨包 APDU：frame → deframe 还原
        let big = apdu(INS_SIGN_PERSONAL, 0, 0, &vec![0xabu8; 200]).unwrap();
        let pkts: Vec<Vec<u8>> = frame(&big).iter().map(|p| p.to_vec()).collect();
        assert!(pkts.len() > 1);
        // 模拟响应方向复用同一 framing
        assert_eq!(deframe(&pkts).unwrap(), big);
    }

    #[test]
    fn parses_address_and_signature() {
        // pubkey(2) + "AB..40 hex" 地址
        let addr_ascii = b"00112233445566778899aabbccddeeff00112233";
        let mut resp = vec![2u8, 0xaa, 0xbb, addr_ascii.len() as u8];
        resp.extend_from_slice(addr_ascii);
        assert_eq!(
            parse_address_response(&resp).unwrap(),
            "0x00112233445566778899aabbccddeeff00112233"
        );

        let mut sig = vec![0u8; 65];
        sig[0] = 0; // v=0 → 27
        sig[1] = 0x11;
        sig[33] = 0x22;
        let rsv = parse_signature_rsv(&sig).unwrap();
        assert_eq!(rsv[0], 0x11);
        assert_eq!(rsv[32], 0x22);
        assert_eq!(rsv[64], 27);
    }

    #[test]
    fn sw_mapping() {
        assert!(map_sw(0x6985).contains("拒绝"));
        assert!(map_sw(0x6d00).contains("Ethereum app"));
        assert!(check_sw(&[0x01, 0x90, 0x00]).is_ok());
        assert!(check_sw(&[0x69, 0x85]).is_err());
    }
}
