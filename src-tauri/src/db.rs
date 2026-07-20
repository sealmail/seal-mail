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
pub const INITIAL_WINDOW: u32 = 200;
/// 用户继续向下翻页时，每次按需回填的更早邮件数
pub const OLDER_WINDOW: u32 = 200;

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
  meta_json  TEXT,
  PRIMARY KEY (account_id, folder, uid)
);
CREATE INDEX IF NOT EXISTS idx_msg_list ON messages(account_id, folder, timestamp DESC);
CREATE INDEX IF NOT EXISTS idx_msg_uidl ON messages(account_id, pop_uidl);
CREATE TABLE IF NOT EXISTS folder_state (
  account_id    TEXT NOT NULL,
  folder        TEXT NOT NULL,
  uidvalidity   INTEGER NOT NULL DEFAULT 0,
  server_exists INTEGER NOT NULL DEFAULT 0,
  PRIMARY KEY (account_id, folder)
);
CREATE TABLE IF NOT EXISTS pop_state (
  account_id TEXT PRIMARY KEY,
  next_uid   INTEGER NOT NULL
);
";

pub fn open(dir: &Path) -> Result<Connection, String> {
    let conn =
        Connection::open(dir.join("mail.db")).map_err(|e| format!("打开邮件缓存失败: {e}"))?;
    // GUI 进程（后台补全/监听）与 CLI 子进程会并发读写同一个库，
    // 撞锁时等待重试而不是直接报 database is locked
    let _ = conn.busy_timeout(std::time::Duration::from_secs(5));
    // GUI、CLI 子进程、watcher、meta 回填并发读写同一库；WAL 允许多读一写，避免回滚日志模式写事务独占把 list 卡到 busy_timeout
    conn.pragma_update(None, "journal_mode", "WAL")
        .map_err(|e| format!("启用 WAL 失败: {e}"))?;
    conn.execute_batch(SCHEMA)
        .map_err(|e| format!("初始化邮件缓存失败: {e}"))?;
    // 旧库补充 meta_json 列（列已存在时 ALTER 会报错，忽略即可——这是安全的幂等迁移）
    let _ = conn.execute("ALTER TABLE messages ADD COLUMN meta_json TEXT", []);
    // 旧库补充 server_exists 列（服务器侧邮件总数，用于全局同步进度）
    let _ = conn.execute(
        "ALTER TABLE folder_state ADD COLUMN server_exists INTEGER NOT NULL DEFAULT 0",
        [],
    );
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
pub fn list(
    conn: &Connection,
    account: &str,
    folder: &str,
    offset: u32,
    limit: u32,
) -> Result<Vec<CachedRow>, String> {
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

pub fn count_unread(conn: &Connection, account: &str, folder: &str) -> Result<i64, String> {
    conn.query_row(
        "SELECT COUNT(*) FROM messages WHERE account_id=?1 AND folder=?2 AND unread=1",
        params![account, folder],
        |r| r.get(0),
    )
    .map_err(err)
}

/// 读取整个目录的“元信息缓存”（newest-first，不含 raw），用于本地会话分组。
/// 命中缓存的行不必读取/解析完整邮件，会话分组因此大幅加速。
pub fn list_folder_meta(
    conn: &Connection,
    account: &str,
    folder: &str,
) -> Result<Vec<MetaRow>, String> {
    // 与 list_meta(limit=0) 相同：目录内本地全量 meta
    list_meta(conn, account, folder, 0, 0)
}

pub fn get_raw(
    conn: &Connection,
    account: &str,
    folder: &str,
    uid: u32,
) -> Result<Option<CachedRow>, String> {
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

/// 列表展示用的轻量行：只读已缓存的元信息 JSON，不读体积大的 raw（含附件）。
/// 这是列表/启动秒开的关键——避免把整个目录的完整邮件都读出来解析。
pub struct MetaRow {
    pub uid: u32,
    pub unread: bool,
    pub flagged: bool,
    pub timestamp: i64,
    pub meta_json: Option<String>,
}

fn map_meta_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<MetaRow> {
    Ok(MetaRow {
        uid: r.get(0)?,
        unread: r.get::<_, i64>(1)? != 0,
        flagged: r.get::<_, i64>(2)? != 0,
        timestamp: r.get(3)?,
        meta_json: r.get(4)?,
    })
}

/// 按时间倒序读取“元信息缓存”（不含 raw）。
/// `limit == 0` 表示不限制条数（本地全量列表）；SQLite 的 LIMIT -1 等价于无上限。
pub fn list_meta(
    conn: &Connection,
    account: &str,
    folder: &str,
    offset: u32,
    limit: u32,
) -> Result<Vec<MetaRow>, String> {
    let sql_limit = if limit == 0 { -1i64 } else { limit as i64 };
    let mut stmt = conn
        .prepare(
            "SELECT uid, unread, flagged, timestamp, meta_json FROM messages
             WHERE account_id=?1 AND folder=?2
             ORDER BY timestamp DESC, uid DESC LIMIT ?3 OFFSET ?4",
        )
        .map_err(err)?;
    let rows = stmt
        .query_map(params![account, folder, sql_limit, offset], map_meta_row)
        .map_err(err)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(err)?;
    Ok(rows)
}

/// 写回某封邮件解析后的元信息缓存（首次解析后调用，下次列表即可秒开）。
pub fn set_meta_json(
    conn: &Connection,
    account: &str,
    folder: &str,
    uid: u32,
    meta_json: &str,
) -> Result<(), String> {
    conn.execute(
        "UPDATE messages SET meta_json=?4 WHERE account_id=?1 AND folder=?2 AND uid=?3",
        params![account, folder, uid, meta_json],
    )
    .map(|_| ())
    .map_err(err)
}

/// 清掉单封 meta，交给后台 backfill 重新解析（乱码缓存等）。
pub fn clear_meta_json(
    conn: &Connection,
    account: &str,
    folder: &str,
    uid: u32,
) -> Result<(), String> {
    conn.execute(
        "UPDATE messages SET meta_json=NULL WHERE account_id=?1 AND folder=?2 AND uid=?3",
        params![account, folder, uid],
    )
    .map(|_| ())
    .map_err(err)
}

/// 清空全部元信息缓存。可信联系人变化后，trust 标记需要按新规则重算时调用。
pub fn clear_all_meta_json(conn: &Connection) -> Result<(), String> {
    conn.execute("UPDATE messages SET meta_json=NULL", [])
        .map(|_| ())
        .map_err(err)
}

/// 还有 meta 缓存缺口的目录（供启动后的后台补全遍历）。
pub fn missing_meta_targets(conn: &Connection) -> Result<Vec<(String, String)>, String> {
    let mut stmt = conn
        .prepare("SELECT DISTINCT account_id, folder FROM messages WHERE meta_json IS NULL")
        .map_err(err)?;
    let rows = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .map_err(err)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(err)?;
    Ok(rows)
}

/// 按 uid 游标批量取出还没有 meta 缓存的行（含 raw，供后台解析回填）。
pub fn rows_missing_meta(
    conn: &Connection,
    account: &str,
    folder: &str,
    after_uid: u32,
    limit: u32,
) -> Result<Vec<CachedRow>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT uid, unread, flagged, raw FROM messages
             WHERE account_id=?1 AND folder=?2 AND meta_json IS NULL AND uid>?3
             ORDER BY uid ASC LIMIT ?4",
        )
        .map_err(err)?;
    let rows = stmt
        .query_map(params![account, folder, after_uid, limit], |r| {
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

pub fn set_unread(
    conn: &Connection,
    account: &str,
    folder: &str,
    uids: &[u32],
    unread: bool,
) -> Result<(), String> {
    for uid in uids {
        conn.execute(
            "UPDATE messages SET unread=?4 WHERE account_id=?1 AND folder=?2 AND uid=?3",
            params![account, folder, uid, unread as i64],
        )
        .map_err(err)?;
    }
    Ok(())
}

pub fn set_flagged(
    conn: &Connection,
    account: &str,
    folder: &str,
    uid: u32,
    flagged: bool,
) -> Result<(), String> {
    conn.execute(
        "UPDATE messages SET flagged=?4 WHERE account_id=?1 AND folder=?2 AND uid=?3",
        params![account, folder, uid, flagged as i64],
    )
    .map(|_| ())
    .map_err(err)
}

pub fn update_flags(
    conn: &Connection,
    account: &str,
    folder: &str,
    uid: u32,
    unread: bool,
    flagged: bool,
) -> Result<(), String> {
    conn.execute(
        "UPDATE messages SET unread=?4, flagged=?5 WHERE account_id=?1 AND folder=?2 AND uid=?3",
        params![account, folder, uid, unread as i64, flagged as i64],
    )
    .map(|_| ())
    .map_err(err)
}

/// POP3 移动到本地虚拟目录。folder 变了，元信息缓存里的 folder 也会过期，一并清空待重算。
pub fn set_folder(
    conn: &Connection,
    account: &str,
    folder: &str,
    uid: u32,
    target: &str,
) -> Result<(), String> {
    conn.execute(
        "UPDATE messages SET folder=?4, meta_json=NULL WHERE account_id=?1 AND folder=?2 AND uid=?3",
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

pub fn min_uid(conn: &Connection, account: &str, folder: &str) -> Result<Option<u32>, String> {
    conn.query_row(
        "SELECT MIN(uid) FROM messages WHERE account_id=?1 AND folder=?2",
        params![account, folder],
        |r| r.get::<_, Option<u32>>(0),
    )
    .map_err(err)
}

/// FLAGS 回扫窗口下界：最近第 N 封的 uid
pub fn window_low(
    conn: &Connection,
    account: &str,
    folder: &str,
    window: u32,
) -> Result<Option<u32>, String> {
    conn.query_row(
        "SELECT MIN(uid) FROM (SELECT uid FROM messages WHERE account_id=?1 AND folder=?2
          ORDER BY uid DESC LIMIT ?3)",
        params![account, folder, window],
        |r| r.get::<_, Option<u32>>(0),
    )
    .map_err(err)
}

pub fn uids_from(
    conn: &Connection,
    account: &str,
    folder: &str,
    low: u32,
) -> Result<Vec<u32>, String> {
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

/// 全目录范围找出原始邮件里含指定子串的候选行（新→旧），ASCII 大小写不敏感
/// （通知目标里的 Message-ID 已规范化为小写，原始头可能是大写）。
/// 用于按 Message-ID 定位被过滤规则移走的邮件；命中行还需上层解析头部确认，
/// 因为回复邮件的 References 里也会出现同一个 Message-ID。
pub fn find_candidates_containing(
    conn: &Connection,
    account: &str,
    needle: &str,
    limit: u32,
) -> Result<Vec<(String, u32, Vec<u8>)>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT folder, uid, raw FROM messages
             WHERE account_id=?1 AND instr(lower(CAST(raw AS TEXT)), lower(?2)) > 0
             ORDER BY timestamp DESC, uid DESC LIMIT ?3",
        )
        .map_err(err)?;
    let rows = stmt
        .query_map(params![account, needle, limit], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })
        .map_err(err)?
        .collect::<Result<Vec<_>, _>>()
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

pub fn set_uidvalidity(
    conn: &Connection,
    account: &str,
    folder: &str,
    v: u32,
) -> Result<(), String> {
    // 用 UPSERT 而非 INSERT OR REPLACE，避免覆盖同行的 server_exists
    conn.execute(
        "INSERT INTO folder_state(account_id, folder, uidvalidity) VALUES (?1,?2,?3)
         ON CONFLICT(account_id, folder) DO UPDATE SET uidvalidity=excluded.uidvalidity",
        params![account, folder, v],
    )
    .map(|_| ())
    .map_err(err)
}

/// 记录该目录在服务器上的邮件总数（IMAP SELECT 的 EXISTS / POP3 UIDL 条数）
pub fn set_server_exists(
    conn: &Connection,
    account: &str,
    folder: &str,
    n: i64,
) -> Result<(), String> {
    conn.execute(
        "INSERT INTO folder_state(account_id, folder, server_exists) VALUES (?1,?2,?3)
         ON CONFLICT(account_id, folder) DO UPDATE SET server_exists=excluded.server_exists",
        params![account, folder, n],
    )
    .map(|_| ())
    .map_err(err)
}

pub struct SyncOverviewRow {
    pub account_id: String,
    pub folder: String,
    pub server_total: i64,
    pub cached: i64,
}

/// 全局同步进度：每个已记录服务器总数的目录，服务器总数 vs 本地已缓存条数
pub fn sync_overview(conn: &Connection) -> Result<Vec<SyncOverviewRow>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT fs.account_id, fs.folder, fs.server_exists,
                    (SELECT COUNT(*) FROM messages m
                      WHERE m.account_id = fs.account_id AND m.folder = fs.folder)
               FROM folder_state fs
              WHERE fs.server_exists > 0",
        )
        .map_err(err)?;
    let rows = stmt
        .query_map([], |r| {
            Ok(SyncOverviewRow {
                account_id: r.get(0)?,
                folder: r.get(1)?,
                server_total: r.get(2)?,
                cached: r.get(3)?,
            })
        })
        .map_err(err)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(err)?;
    Ok(rows)
}

/// 该账户全部已知的 POP3 UIDL（跨本地目录）
pub fn pop_known_uidls(
    conn: &Connection,
    account: &str,
) -> Result<Vec<(String, String, u32)>, String> {
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

pub fn pop_uidl_of(
    conn: &Connection,
    account: &str,
    folder: &str,
    uid: u32,
) -> Result<Option<String>, String> {
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
    fn opens_with_wal_journal_mode() {
        let dir = std::env::temp_dir().join(format!("sealmail-db-wal-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let conn = open(&dir).unwrap();
        let mode: String = conn
            .pragma_query_value(None, "journal_mode", |row| row.get(0))
            .unwrap();
        assert_eq!(mode.to_lowercase(), "wal");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn roundtrip_and_paging() {
        let dir = std::env::temp_dir().join(format!("sealmail-db-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let conn = open(&dir).unwrap();
        for i in 1..=5u32 {
            upsert_message(
                &conn,
                "a",
                "INBOX",
                i,
                None,
                true,
                false,
                1000 + i as i64,
                b"raw",
            )
            .unwrap();
        }
        assert_eq!(count(&conn, "a", "INBOX").unwrap(), 5);
        assert_eq!(count_unread(&conn, "a", "INBOX").unwrap(), 5);
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
        assert_eq!(count_unread(&conn, "a", "INBOX").unwrap(), 4);
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

    #[test]
    fn server_exists_and_sync_overview() {
        let dir = std::env::temp_dir().join(format!("sealmail-sync-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let conn = open(&dir).unwrap();
        // 没记录过服务器总数时不产生进度行
        assert!(sync_overview(&conn).unwrap().is_empty());

        // 账户 a：服务器 10 封，本地缓存 3 封
        set_server_exists(&conn, "a", "INBOX", 10).unwrap();
        for i in 1..=3u32 {
            upsert_message(&conn, "a", "INBOX", i, None, true, false, i as i64, b"raw").unwrap();
        }
        // 账户 b：服务器 5 封，本地 0 封
        set_server_exists(&conn, "b", "INBOX", 5).unwrap();

        let rows = sync_overview(&conn).unwrap();
        assert_eq!(rows.len(), 2);
        let a = rows.iter().find(|r| r.account_id == "a").unwrap();
        assert_eq!((a.server_total, a.cached), (10, 3));
        let b = rows.iter().find(|r| r.account_id == "b").unwrap();
        assert_eq!((b.server_total, b.cached), (5, 0));

        // set_uidvalidity 不得覆盖同行的 server_exists（此前是 INSERT OR REPLACE 会清零）
        set_uidvalidity(&conn, "a", "INBOX", 42).unwrap();
        let rows = sync_overview(&conn).unwrap();
        let a = rows.iter().find(|r| r.account_id == "a").unwrap();
        assert_eq!(a.server_total, 10);
        assert_eq!(uidvalidity(&conn, "a", "INBOX").unwrap(), Some(42));

        // 再次同步更新服务器总数
        set_server_exists(&conn, "a", "INBOX", 12).unwrap();
        let rows = sync_overview(&conn).unwrap();
        let a = rows.iter().find(|r| r.account_id == "a").unwrap();
        assert_eq!(a.server_total, 12);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn meta_json_cache_lifecycle() {
        let dir = std::env::temp_dir().join(format!("sealmail-meta-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let conn = open(&dir).unwrap();
        upsert_message(&conn, "a", "INBOX", 1, None, true, false, 1000, b"raw-bytes").unwrap();

        // 新邮件还没有元信息缓存
        let rows = list_meta(&conn, "a", "INBOX", 0, 10).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].uid, 1);
        assert!(rows[0].meta_json.is_none(), "刚插入应无元信息缓存");

        // 写回后能读到
        set_meta_json(&conn, "a", "INBOX", 1, "{\"subject\":\"hi\"}").unwrap();
        let rows = list_meta(&conn, "a", "INBOX", 0, 10).unwrap();
        assert_eq!(rows[0].meta_json.as_deref(), Some("{\"subject\":\"hi\"}"));

        // 重新 upsert（邮件更新）会重置元信息缓存为 NULL，迫使下次重新解析
        upsert_message(&conn, "a", "INBOX", 1, None, false, true, 1000, b"raw-bytes-2").unwrap();
        let rows = list_meta(&conn, "a", "INBOX", 0, 10).unwrap();
        assert!(rows[0].meta_json.is_none(), "重新 upsert 后元信息缓存应失效");

        // 清空全部（可信联系人变化时）
        set_meta_json(&conn, "a", "INBOX", 1, "{\"subject\":\"x\"}").unwrap();
        clear_all_meta_json(&conn).unwrap();
        assert!(list_meta(&conn, "a", "INBOX", 0, 10).unwrap()[0]
            .meta_json
            .is_none());

        // POP3 移动目录会清掉该行的元信息缓存（folder 已变，缓存里的 folder 会过期）
        set_meta_json(&conn, "a", "INBOX", 1, "{\"folder\":\"INBOX\"}").unwrap();
        set_folder(&conn, "a", "INBOX", 1, "归档").unwrap();
        let rows = list_meta(&conn, "a", "归档", 0, 10).unwrap();
        assert_eq!(rows.len(), 1);
        assert!(rows[0].meta_json.is_none(), "移动目录后元信息缓存应失效");

        std::fs::remove_dir_all(&dir).ok();
    }
}
