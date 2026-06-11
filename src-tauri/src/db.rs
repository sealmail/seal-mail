//! 本地邮件缓存（SQLite，mail.db）。
//! 存原始 RFC822 邮件 + 列表所需的少量列；读取时重新解析+验证，
//! 这样信任列表变化（新增/移除可信联系人）后无需迁移缓存即可生效。
//! - IMAP：按 UIDVALIDITY+UID 增量同步；近窗口同步 FLAGS 并检测服务器侧删除
//! - POP3：按 UIDL 识别邮件，uid 为本地自增；目录/已读/星标都记在本地列

use rusqlite::{params, Connection, OptionalExtension};
use std::path::Path;

/// 已读/删除状态回扫窗口（最近 N 封）
pub const FLAG_SYNC_WINDOW: u32 = 200;
/// 首次同步抓取的邮件数
pub const INITIAL_WINDOW: u32 = 50;

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS messages (
  account_id TEXT NOT NULL,
  folder     TEXT NOT NULL,
  uid        INTEGER NOT NULL,
  pop_uidl   TEXT,
  unread     INTEGER NOT NULL DEFAULT 0,
  flagged    INTEGER NOT NULL DEFAULT 0,
  timestamp  INTEGER NOT NULL DEFAULT 0,
  raw        BLOB NOT NULL,
  PRIMARY KEY (account_id, folder, uid)
);
CREATE INDEX IF NOT EXISTS idx_msg_list ON messages(account_id, folder, timestamp DESC);
CREATE INDEX IF NOT EXISTS idx_msg_uidl ON messages(account_id, pop_uidl);
CREATE TABLE IF NOT EXISTS folder_state (
  account_id  TEXT NOT NULL,
  folder      TEXT NOT NULL,
  uidvalidity INTEGER NOT NULL DEFAULT 0,
  PRIMARY KEY (account_id, folder)
);
CREATE TABLE IF NOT EXISTS pop_state (
  account_id TEXT PRIMARY KEY,
  next_uid   INTEGER NOT NULL
);
";

pub fn open(dir: &Path) -> Result<Connection, String> {
    let conn = Connection::open(dir.join("mail.db")).map_err(|e| format!("打开邮件缓存失败: {e}"))?;
    conn.execute_batch(SCHEMA).map_err(|e| format!("初始化邮件缓存失败: {e}"))?;
    Ok(conn)
}

fn err(e: rusqlite::Error) -> String {
    format!("邮件缓存读写失败: {e}")
}

pub struct CachedRow {
    pub uid: u32,
    pub unread: bool,
    pub flagged: bool,
    pub raw: Vec<u8>,
}

/// 按时间倒序分页读取
pub fn list(conn: &Connection, account: &str, folder: &str, offset: u32, limit: u32) -> Result<Vec<CachedRow>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT uid, unread, flagged, raw FROM messages
             WHERE account_id=?1 AND folder=?2
             ORDER BY timestamp DESC, uid DESC LIMIT ?3 OFFSET ?4",
        )
        .map_err(err)?;
    let rows = stmt
        .query_map(params![account, folder, limit, offset], |r| {
            Ok(CachedRow {
                uid: r.get(0)?,
                unread: r.get::<_, i64>(1)? != 0,
                flagged: r.get::<_, i64>(2)? != 0,
                raw: r.get(3)?,
            })
        })
        .map_err(err)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(err)?;
    Ok(rows)
}

pub fn count(conn: &Connection, account: &str, folder: &str) -> Result<i64, String> {
    conn.query_row(
        "SELECT COUNT(*) FROM messages WHERE account_id=?1 AND folder=?2",
        params![account, folder],
        |r| r.get(0),
    )
    .map_err(err)
}

pub fn get_raw(conn: &Connection, account: &str, folder: &str, uid: u32) -> Result<Option<CachedRow>, String> {
    conn.query_row(
        "SELECT uid, unread, flagged, raw FROM messages WHERE account_id=?1 AND folder=?2 AND uid=?3",
        params![account, folder, uid],
        |r| {
            Ok(CachedRow {
                uid: r.get(0)?,
                unread: r.get::<_, i64>(1)? != 0,
                flagged: r.get::<_, i64>(2)? != 0,
                raw: r.get(3)?,
            })
        },
    )
    .optional()
    .map_err(err)
}

#[allow(clippy::too_many_arguments)]
pub fn upsert_message(
    conn: &Connection,
    account: &str,
    folder: &str,
    uid: u32,
    pop_uidl: Option<&str>,
    unread: bool,
    flagged: bool,
    timestamp: i64,
    raw: &[u8],
) -> Result<(), String> {
    conn.execute(
        "INSERT OR REPLACE INTO messages(account_id, folder, uid, pop_uidl, unread, flagged, timestamp, raw)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
        params![account, folder, uid, pop_uidl, unread as i64, flagged as i64, timestamp, raw],
    )
    .map(|_| ())
    .map_err(err)
}

pub fn set_unread(conn: &Connection, account: &str, folder: &str, uids: &[u32], unread: bool) -> Result<(), String> {
    for uid in uids {
        conn.execute(
            "UPDATE messages SET unread=?4 WHERE account_id=?1 AND folder=?2 AND uid=?3",
            params![account, folder, uid, unread as i64],
        )
        .map_err(err)?;
    }
    Ok(())
}

pub fn set_flagged(conn: &Connection, account: &str, folder: &str, uid: u32, flagged: bool) -> Result<(), String> {
    conn.execute(
        "UPDATE messages SET flagged=?4 WHERE account_id=?1 AND folder=?2 AND uid=?3",
        params![account, folder, uid, flagged as i64],
    )
    .map(|_| ())
    .map_err(err)
}

pub fn update_flags(conn: &Connection, account: &str, folder: &str, uid: u32, unread: bool, flagged: bool) -> Result<(), String> {
    conn.execute(
        "UPDATE messages SET unread=?4, flagged=?5 WHERE account_id=?1 AND folder=?2 AND uid=?3",
        params![account, folder, uid, unread as i64, flagged as i64],
    )
    .map(|_| ())
    .map_err(err)
}

/// POP3 移动到本地虚拟目录
pub fn set_folder(conn: &Connection, account: &str, folder: &str, uid: u32, target: &str) -> Result<(), String> {
    conn.execute(
        "UPDATE messages SET folder=?4 WHERE account_id=?1 AND folder=?2 AND uid=?3",
        params![account, folder, uid, target],
    )
    .map(|_| ())
    .map_err(err)
}

pub fn delete_row(conn: &Connection, account: &str, folder: &str, uid: u32) -> Result<(), String> {
    conn.execute(
        "DELETE FROM messages WHERE account_id=?1 AND folder=?2 AND uid=?3",
        params![account, folder, uid],
    )
    .map(|_| ())
    .map_err(err)
}

pub fn clear_folder(conn: &Connection, account: &str, folder: &str) -> Result<(), String> {
    conn.execute(
        "DELETE FROM messages WHERE account_id=?1 AND folder=?2",
        params![account, folder],
    )
    .map(|_| ())
    .map_err(err)
}

pub fn max_uid(conn: &Connection, account: &str, folder: &str) -> Result<Option<u32>, String> {
    conn.query_row(
        "SELECT MAX(uid) FROM messages WHERE account_id=?1 AND folder=?2",
        params![account, folder],
        |r| r.get::<_, Option<u32>>(0),
    )
    .map_err(err)
}

/// FLAGS 回扫窗口下界：最近第 N 封的 uid
pub fn window_low(conn: &Connection, account: &str, folder: &str, window: u32) -> Result<Option<u32>, String> {
    conn.query_row(
        "SELECT MIN(uid) FROM (SELECT uid FROM messages WHERE account_id=?1 AND folder=?2
          ORDER BY uid DESC LIMIT ?3)",
        params![account, folder, window],
        |r| r.get::<_, Option<u32>>(0),
    )
    .map_err(err)
}

pub fn uids_from(conn: &Connection, account: &str, folder: &str, low: u32) -> Result<Vec<u32>, String> {
    let mut stmt = conn
        .prepare("SELECT uid FROM messages WHERE account_id=?1 AND folder=?2 AND uid>=?3")
        .map_err(err)?;
    let rows = stmt
        .query_map(params![account, folder, low], |r| r.get(0))
        .map_err(err)?
        .collect::<Result<Vec<u32>, _>>()
        .map_err(err)?;
    Ok(rows)
}

pub fn uidvalidity(conn: &Connection, account: &str, folder: &str) -> Result<Option<u32>, String> {
    conn.query_row(
        "SELECT uidvalidity FROM folder_state WHERE account_id=?1 AND folder=?2",
        params![account, folder],
        |r| r.get(0),
    )
    .optional()
    .map_err(err)
}

pub fn set_uidvalidity(conn: &Connection, account: &str, folder: &str, v: u32) -> Result<(), String> {
    conn.execute(
        "INSERT OR REPLACE INTO folder_state(account_id, folder, uidvalidity) VALUES (?1,?2,?3)",
        params![account, folder, v],
    )
    .map(|_| ())
    .map_err(err)
}

/// 该账户全部已知的 POP3 UIDL（跨本地目录）
pub fn pop_known_uidls(conn: &Connection, account: &str) -> Result<Vec<(String, String, u32)>, String> {
    let mut stmt = conn
        .prepare("SELECT pop_uidl, folder, uid FROM messages WHERE account_id=?1 AND pop_uidl IS NOT NULL")
        .map_err(err)?;
    let rows = stmt
        .query_map(params![account], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .map_err(err)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(err)?;
    Ok(rows)
}

pub fn pop_uidl_of(conn: &Connection, account: &str, folder: &str, uid: u32) -> Result<Option<String>, String> {
    conn.query_row(
        "SELECT pop_uidl FROM messages WHERE account_id=?1 AND folder=?2 AND uid=?3",
        params![account, folder, uid],
        |r| r.get(0),
    )
    .optional()
    .map_err(err)
    .map(Option::flatten)
}

/// 取下一个 POP3 本地 uid（自增）
pub fn pop_next_uid(conn: &Connection, account: &str) -> Result<u32, String> {
    let cur: u32 = conn
        .query_row(
            "SELECT next_uid FROM pop_state WHERE account_id=?1",
            params![account],
            |r| r.get(0),
        )
        .optional()
        .map_err(err)?
        .unwrap_or(1);
    conn.execute(
        "INSERT OR REPLACE INTO pop_state(account_id, next_uid) VALUES (?1,?2)",
        params![account, cur + 1],
    )
    .map_err(err)?;
    Ok(cur)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_and_paging() {
        let dir = std::env::temp_dir().join(format!("sealmail-db-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let conn = open(&dir).unwrap();
        for i in 1..=5u32 {
            upsert_message(&conn, "a", "INBOX", i, None, true, false, 1000 + i as i64, b"raw").unwrap();
        }
        assert_eq!(count(&conn, "a", "INBOX").unwrap(), 5);
        // 时间倒序：第一页是最新的
        let page = list(&conn, "a", "INBOX", 0, 2).unwrap();
        assert_eq!(page[0].uid, 5);
        assert_eq!(page[1].uid, 4);
        let page2 = list(&conn, "a", "INBOX", 2, 2).unwrap();
        assert_eq!(page2[0].uid, 3);
        assert_eq!(max_uid(&conn, "a", "INBOX").unwrap(), Some(5));
        // 窗口下界：最近 3 封的最小 uid
        assert_eq!(window_low(&conn, "a", "INBOX", 3).unwrap(), Some(3));
        // 标记与移动
        set_unread(&conn, "a", "INBOX", &[5], false).unwrap();
        assert!(!list(&conn, "a", "INBOX", 0, 1).unwrap()[0].unread);
        set_flagged(&conn, "a", "INBOX", 5, true).unwrap();
        assert!(list(&conn, "a", "INBOX", 0, 1).unwrap()[0].flagged);
        set_folder(&conn, "a", "INBOX", 5, "归档").unwrap();
        assert_eq!(count(&conn, "a", "INBOX").unwrap(), 4);
        assert_eq!(count(&conn, "a", "归档").unwrap(), 1);
        // uidvalidity
        assert_eq!(uidvalidity(&conn, "a", "INBOX").unwrap(), None);
        set_uidvalidity(&conn, "a", "INBOX", 99).unwrap();
        assert_eq!(uidvalidity(&conn, "a", "INBOX").unwrap(), Some(99));
        // POP3 自增 uid
        assert_eq!(pop_next_uid(&conn, "p").unwrap(), 1);
        assert_eq!(pop_next_uid(&conn, "p").unwrap(), 2);
        std::fs::remove_dir_all(&dir).ok();
    }
}
