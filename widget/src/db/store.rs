use std::collections::HashMap;

use rusqlite::Connection;

pub(in crate::db) fn column_exists(conn: &Connection, table: &str, column: &str) -> bool {
    let Ok(mut stmt) = conn.prepare(&format!("PRAGMA table_info({table})")) else {
        return false;
    };
    stmt.query_map([], |row| row.get::<_, String>(1))
        .map(|rows| rows.filter_map(Result::ok).any(|name| name == column))
        .unwrap_or(false)
}

pub(in crate::db) fn table_exists(conn: &Connection, name: &str) -> bool {
    conn.query_row(
        "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1",
        [name],
        |row| row.get::<_, i64>(0),
    )
    .is_ok()
}

pub(in crate::db) fn load_online(conn: &Connection) -> Result<HashMap<String, String>, String> {
    let mut stmt = conn
        .prepare("SELECT id, lang, text FROM strings WHERE object = 'OnlineText.dat'")
        .map_err(|err| err.to_string())?;
    let rows = stmt
        .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?)))
        .map_err(|err| err.to_string())?;
    let mut texts = HashMap::new();
    for row in rows {
        let (id, lang, text) = row.map_err(|err| err.to_string())?;
        texts.insert(format!("{lang}:{id}"), text);
    }
    Ok(texts)
}

pub(in crate::db) fn online<'a>(texts: &'a HashMap<String, String>, gts: i64, lang: &str) -> &'a str {
    if gts <= 0 {
        return "";
    }
    texts.get(&format!("{lang}:{gts:08}")).map(String::as_str).unwrap_or("")
}
