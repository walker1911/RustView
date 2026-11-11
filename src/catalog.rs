use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use std::{
    path::Path,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

const MAX_QUERY: usize = 500;
const MAX_KEY_BYTES: usize = 32 * 1024;
const MAX_CATEGORY_BYTES: usize = 4096;
const MAX_ARCHIVE_RESUME: i64 = 512;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Record {
    pub rating: u8,
    pub label: u8,
    pub marked: bool,
    pub category: String,
    pub basket: bool,
    pub title: String,
    pub description: String,
    pub keywords: String,
    pub iptc_keywords: String,
}

#[derive(Clone, Debug)]
pub enum Filter {
    All,
    Marked,
    Basket,
    Rating(u8),
    Label(u8),
    Category(String),
}

#[derive(Clone, Debug)]
pub enum Change {
    Rating(u8),
    Label(u8),
    ToggleMark,
    Category(String),
    Basket(bool),
    Title(String),
    Description(String),
    Keywords(String),
    IptcKeywords(String),
    Preset(Record),
}

pub struct Catalog {
    connection: Connection,
}

fn valid_key(key: &str) -> Result<(), String> {
    if key.is_empty() || key.len() > MAX_KEY_BYTES || key.contains('\0') {
        Err("图片元数据键为空或过长".into())
    } else {
        Ok(())
    }
}

fn archive_resume_key(archive: &Path) -> Result<String, String> {
    let key = archive
        .canonicalize()
        .map_err(|error| error.to_string())?
        .into_os_string()
        .into_string()
        .map_err(|_| "压缩包路径无法保存")?;
    valid_key(&key)?;
    Ok(key)
}

fn read_record(connection: &Connection, key: &str) -> Result<Record, String> {
    connection.query_row(
        "SELECT rating,label,marked,category,basket,title,description,keywords,iptc_keywords FROM items WHERE item_key=?1",
        [key],
        |row| Ok(Record { rating: row.get(0)?, label: row.get(1)?, marked: row.get(2)?, category: row.get(3)?, basket: row.get(4)?, title: row.get(5)?, description: row.get(6)?, keywords: row.get(7)?, iptc_keywords: row.get(8)? }),
    ).optional().map(|record| record.unwrap_or_default()).map_err(|error| error.to_string())
}

fn apply_change(record: &mut Record, change: &Change) -> Result<(), String> {
    match change {
        Change::Rating(value) if *value <= 5 => record.rating = *value,
        Change::Label(value) if *value <= 5 => record.label = *value,
        Change::Rating(_) | Change::Label(_) => return Err("评级和颜色标签必须在 0–5 之间".into()),
        Change::ToggleMark => record.marked = !record.marked,
        Change::Category(value) => {
            if value.len() > MAX_CATEGORY_BYTES || value.contains('\0') {
                return Err("类别名称过长或包含空字符".into());
            }
            record.category = value.trim().to_owned();
        }
        Change::Basket(value) => record.basket = *value,
        Change::Title(value) => {
            if value.len() > 4096 || value.contains('\0') {
                return Err("标题过长或包含空字符".into());
            }
            record.title = value.clone();
        }
        Change::Description(value) => {
            if value.len() > 32 * 1024 || value.contains('\0') {
                return Err("说明过长或包含空字符".into());
            }
            record.description = value.clone();
        }
        Change::Keywords(value) | Change::IptcKeywords(value) => {
            if value.len() > 8192 || value.contains('\0') {
                return Err("关键词过长或包含空字符".into());
            }
            if matches!(change, Change::Keywords(_)) {
                record.keywords = value.clone();
            } else {
                record.iptc_keywords = value.clone();
            }
        }
        Change::Preset(value) => {
            for change in [
                Change::Rating(value.rating),
                Change::Label(value.label),
                Change::Category(value.category.clone()),
                Change::Title(value.title.clone()),
                Change::Description(value.description.clone()),
                Change::Keywords(value.keywords.clone()),
                Change::IptcKeywords(value.iptc_keywords.clone()),
            ] {
                apply_change(record, &change)?;
            }
        }
    }
    Ok(())
}

impl Catalog {
    pub fn path(&self) -> Option<&Path> {
        self.connection
            .path()
            .filter(|p| !p.is_empty())
            .map(Path::new)
    }
    /// Stores only RustView's own metadata. The supplied key must not contain a password.
    pub fn open(path: &Path) -> Result<Self, String> {
        if let Some(parent) = path.parent().filter(|path| !path.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        let mut connection = Connection::open(path).map_err(|error| error.to_string())?;
        connection
            .busy_timeout(Duration::from_millis(300))
            .map_err(|error| error.to_string())?;
        connection
            .execute_batch(
                "PRAGMA journal_mode=WAL;
             PRAGMA synchronous=NORMAL;
             PRAGMA cache_size=-2048;
             PRAGMA temp_store=FILE;
             PRAGMA max_page_count=65536;
             CREATE TABLE IF NOT EXISTS items (
               item_key TEXT PRIMARY KEY NOT NULL,
               rating INTEGER NOT NULL CHECK(rating BETWEEN 0 AND 5),
               label INTEGER NOT NULL CHECK(label BETWEEN 0 AND 5),
               marked INTEGER NOT NULL CHECK(marked IN (0,1)),
               category TEXT NOT NULL,
               basket INTEGER NOT NULL CHECK(basket IN (0,1)),
               updated INTEGER NOT NULL,
               basket_added INTEGER NOT NULL,
               title TEXT NOT NULL DEFAULT '',
               description TEXT NOT NULL DEFAULT '',
               keywords TEXT NOT NULL DEFAULT '',
               iptc_keywords TEXT NOT NULL DEFAULT ''
             );
             CREATE INDEX IF NOT EXISTS items_rating ON items(rating,updated);
             CREATE INDEX IF NOT EXISTS items_label ON items(label,updated);
             CREATE INDEX IF NOT EXISTS items_marked ON items(marked,updated);
             CREATE INDEX IF NOT EXISTS items_category ON items(category,updated);
             CREATE INDEX IF NOT EXISTS items_basket ON items(basket,basket_added);
             CREATE TABLE IF NOT EXISTS settings(name TEXT PRIMARY KEY NOT NULL,value TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS archive_resume (
               archive_key TEXT PRIMARY KEY NOT NULL,
               entry_name TEXT NOT NULL,
               viewed_at INTEGER NOT NULL
             );",
            )
            .map_err(|error| format!("无法初始化图片元数据：{error}"))?;
        let transaction = connection
            .transaction()
            .map_err(|error| error.to_string())?;
        let columns = {
            let mut statement = transaction
                .prepare("PRAGMA table_info(items)")
                .map_err(|error| error.to_string())?;
            let rows = statement
                .query_map([], |row| row.get::<_, String>(1))
                .map_err(|error| error.to_string())?;
            rows.collect::<Result<Vec<_>, _>>()
                .map_err(|error| error.to_string())?
        };
        if !columns.iter().any(|column| column == "title") {
            transaction
                .execute(
                    "ALTER TABLE items ADD COLUMN title TEXT NOT NULL DEFAULT ''",
                    [],
                )
                .map_err(|error| error.to_string())?;
        }
        if !columns.iter().any(|column| column == "description") {
            transaction
                .execute(
                    "ALTER TABLE items ADD COLUMN description TEXT NOT NULL DEFAULT ''",
                    [],
                )
                .map_err(|error| error.to_string())?;
        }
        for name in ["keywords", "iptc_keywords"] {
            if !columns.iter().any(|column| column == name) {
                transaction
                    .execute(
                        &format!("ALTER TABLE items ADD COLUMN {name} TEXT NOT NULL DEFAULT ''"),
                        [],
                    )
                    .map_err(|error| error.to_string())?;
            }
        }
        transaction
            .execute_batch("PRAGMA user_version=3")
            .map_err(|error| error.to_string())?;
        transaction.commit().map_err(|error| error.to_string())?;
        Ok(Self { connection })
    }

    pub fn get(&self, key: &str) -> Result<Record, String> {
        valid_key(key)?;
        read_record(&self.connection, key)
    }

    pub fn archive_resume(&self, archive: &Path) -> Result<Option<String>, String> {
        self.connection
            .query_row(
                "SELECT entry_name FROM archive_resume WHERE archive_key=?1",
                [archive_resume_key(archive)?],
                |row| row.get(0),
            )
            .optional()
            .map_err(|error| error.to_string())
    }

    pub fn remember_archive_resume(
        &mut self,
        archive: &Path,
        entry: &str,
        viewed_at: SystemTime,
    ) -> Result<(), String> {
        let key = archive_resume_key(archive)?;
        valid_key(entry)?;
        let micros = viewed_at
            .duration_since(UNIX_EPOCH)
            .map_err(|error| error.to_string())?
            .as_micros();
        let viewed_at = i64::try_from(micros).map_err(|error| error.to_string())?;
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| error.to_string())?;
        transaction
            .execute(
                "INSERT INTO archive_resume(archive_key,entry_name,viewed_at)
                 VALUES(?1,?2,?3)
                 ON CONFLICT(archive_key) DO UPDATE SET
                   entry_name=excluded.entry_name,viewed_at=excluded.viewed_at
                 WHERE excluded.viewed_at>=archive_resume.viewed_at",
                params![key, entry, viewed_at],
            )
            .map_err(|error| error.to_string())?;
        transaction
            .execute(
                "DELETE FROM archive_resume WHERE rowid IN (
                   SELECT rowid FROM archive_resume ORDER BY viewed_at DESC, rowid DESC LIMIT -1 OFFSET ?1
                 )",
                [MAX_ARCHIVE_RESUME],
            )
            .map_err(|error| error.to_string())?;
        transaction.commit().map_err(|error| error.to_string())
    }

    pub fn set_rating(&mut self, key: &str, value: u8) -> Result<Record, String> {
        self.update_many(&[key.to_owned()], Change::Rating(value))?;
        self.get(key)
    }

    pub fn set_label(&mut self, key: &str, value: u8) -> Result<Record, String> {
        self.update_many(&[key.to_owned()], Change::Label(value))?;
        self.get(key)
    }

    pub fn toggle_mark(&mut self, key: &str) -> Result<Record, String> {
        self.update_many(&[key.to_owned()], Change::ToggleMark)?;
        self.get(key)
    }

    pub fn set_category(&mut self, key: &str, value: &str) -> Result<Record, String> {
        self.update_many(&[key.to_owned()], Change::Category(value.to_owned()))?;
        self.get(key)
    }

    pub fn set_basket(&mut self, key: &str, value: bool) -> Result<Record, String> {
        self.update_many(&[key.to_owned()], Change::Basket(value))?;
        self.get(key)
    }

    pub fn set_title(&mut self, key: &str, value: &str) -> Result<Record, String> {
        self.update_many(&[key.to_owned()], Change::Title(value.to_owned()))?;
        self.get(key)
    }

    pub fn set_description(&mut self, key: &str, value: &str) -> Result<Record, String> {
        self.update_many(&[key.to_owned()], Change::Description(value.to_owned()))?;
        self.get(key)
    }

    /// RustView keywords and IPTC-keyword catalog fields are stored independently.
    pub fn set_keywords(&mut self, key: &str, value: &str) -> Result<Record, String> {
        self.update_many(&[key.to_owned()], Change::Keywords(value.to_owned()))?;
        self.get(key)
    }

    pub fn set_iptc_keywords(&mut self, key: &str, value: &str) -> Result<Record, String> {
        self.update_many(&[key.to_owned()], Change::IptcKeywords(value.to_owned()))?;
        self.get(key)
    }

    pub fn default_preset(&self) -> Result<Option<Record>, String> {
        let value: Option<String> = self
            .connection
            .query_row(
                "SELECT value FROM settings WHERE name='default_metadata_preset'",
                [],
                |row| row.get(0),
            )
            .optional()
            .map_err(|error| error.to_string())?;
        value
            .map(|value| {
                if value.len() > 64 * 1024 {
                    return Err("元数据预设超过64KiB上限".into());
                }
                let record: Record = serde_json::from_str(&value)
                    .map_err(|error| format!("元数据预设无效：{error}"))?;
                apply_change(&mut Record::default(), &Change::Preset(record.clone()))?;
                Ok(record)
            })
            .transpose()
    }

    pub fn set_default_preset(&mut self, record: &Record) -> Result<(), String> {
        apply_change(&mut Record::default(), &Change::Preset(record.clone()))?;
        let value = serde_json::to_string(record).map_err(|error| error.to_string())?;
        if value.len() > 64 * 1024 {
            return Err("元数据预设超过64KiB上限".into());
        }
        self.connection.execute("INSERT INTO settings(name,value) VALUES('default_metadata_preset',?1) ON CONFLICT(name) DO UPDATE SET value=excluded.value",[value]).map(|_|()).map_err(|error|error.to_string())
    }

    /// Presets preserve each item's mark and image-basket membership.
    pub fn apply_default(&mut self, keys: &[String]) -> Result<(), String> {
        let preset = self.default_preset()?.ok_or("尚未设置默认元数据预设")?;
        self.update_many(keys, Change::Preset(preset))
    }

    /// A selection is updated in one transaction, without loading the catalog into RAM.
    pub fn update_many(&mut self, keys: &[String], change: Change) -> Result<(), String> {
        if keys.len() > 100_000 {
            return Err("一次最多处理十万条图片元数据".into());
        }
        for key in keys {
            valid_key(key)?;
        }
        apply_change(&mut Record::default(), &change)?;
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
            .min(i64::MAX as u128) as i64;
        let transaction = self
            .connection
            .transaction()
            .map_err(|error| error.to_string())?;
        let mut seen = std::collections::HashSet::new();
        for key in keys {
            if !seen.insert(key) {
                continue;
            }
            let mut record = read_record(&transaction, key)?;
            apply_change(&mut record, &change)?;
            if record == Record::default() {
                transaction
                    .execute("DELETE FROM items WHERE item_key=?1", [key])
                    .map_err(|error| error.to_string())?;
                continue;
            }
            transaction.execute(
                "INSERT INTO items(item_key,rating,label,marked,category,basket,updated,basket_added,title,description,keywords,iptc_keywords)
                 VALUES(?1,?2,?3,?4,?5,?6,?7,CASE WHEN ?6 THEN ?7 ELSE 0 END,?8,?9,?10,?11)
                 ON CONFLICT(item_key) DO UPDATE SET
                   rating=excluded.rating,label=excluded.label,marked=excluded.marked,
                   category=excluded.category,basket=excluded.basket,updated=excluded.updated,
                   title=excluded.title,description=excluded.description,
                   keywords=excluded.keywords,iptc_keywords=excluded.iptc_keywords,
                   basket_added=CASE WHEN excluded.basket=0 THEN 0 WHEN items.basket=1 THEN items.basket_added ELSE excluded.updated END",
                params![key,record.rating,record.label,record.marked,record.category,record.basket,now,record.title,record.description,record.keywords,record.iptc_keywords],
            ).map_err(|error| error.to_string())?;
        }
        transaction.commit().map_err(|error| error.to_string())
    }

    /// At most 500 records are returned per call; callers page with offset.
    pub fn list(
        &self,
        filter: Filter,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<(String, Record)>, String> {
        use rusqlite::types::Value;
        let (condition, value, order) = match filter {
            Filter::All => ("?1=0", Value::Integer(0), "updated DESC,item_key"),
            Filter::Marked => ("marked=?1", Value::Integer(1), "updated DESC,item_key"),
            Filter::Basket => ("basket=?1", Value::Integer(1), "basket_added,item_key"),
            Filter::Rating(value) if value <= 5 => (
                "rating=?1",
                Value::Integer(value.into()),
                "updated DESC,item_key",
            ),
            Filter::Label(value) if value <= 5 => (
                "label=?1",
                Value::Integer(value.into()),
                "updated DESC,item_key",
            ),
            Filter::Category(value) if value.len() <= MAX_CATEGORY_BYTES => {
                ("category=?1", Value::Text(value), "updated DESC,item_key")
            }
            _ => return Err("无效的图片元数据查询".into()),
        };
        if limit == 0 {
            return Ok(Vec::new());
        }
        let offset = i64::try_from(offset).map_err(|_| "查询起始位置过大")?;
        let sql = format!("SELECT item_key,rating,label,marked,category,basket,title,description,keywords,iptc_keywords FROM items WHERE {condition} ORDER BY {order} LIMIT ?2 OFFSET ?3");
        let mut statement = self
            .connection
            .prepare(&sql)
            .map_err(|error| error.to_string())?;
        let rows = statement
            .query_map(params![value, limit.min(MAX_QUERY) as i64, offset], |row| {
                Ok((
                    row.get(0)?,
                    Record {
                        rating: row.get(1)?,
                        label: row.get(2)?,
                        marked: row.get(3)?,
                        category: row.get(4)?,
                        basket: row.get(5)?,
                        title: row.get(6)?,
                        description: row.get(7)?,
                        keywords: row.get(8)?,
                        iptc_keywords: row.get(9)?,
                    },
                ))
            })
            .map_err(|error| error.to_string())?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())
    }

    pub fn categories(&self, limit: usize) -> Result<Vec<String>, String> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT DISTINCT category FROM items WHERE category<>'' ORDER BY category LIMIT ?1",
            )
            .map_err(|error| error.to_string())?;
        let rows = statement
            .query_map([limit.min(MAX_QUERY) as i64], |row| row.get(0))
            .map_err(|error| error.to_string())?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())
    }

    /// Lightweight projection used by the directory sort worker. Text metadata
    /// is deliberately not loaded when only rating and color are needed.
    pub fn get_marks(&self, keys: &[String]) -> Result<Vec<(String, u8, u8)>, String> {
        if keys.len() > MAX_QUERY {
            return Err("一次目录库查询最多500个键".into());
        }
        if keys.is_empty() {
            return Ok(Vec::new());
        }
        for key in keys {
            valid_key(key)?;
        }
        let placeholders = (1..=keys.len())
            .map(|n| format!("?{n}"))
            .collect::<Vec<_>>()
            .join(",");
        let mut statement = self
            .connection
            .prepare_cached(&format!(
                "SELECT item_key,rating,label FROM items WHERE item_key IN ({placeholders})"
            ))
            .map_err(|e| e.to_string())?;
        let rows = statement
            .query_map(rusqlite::params_from_iter(keys.iter()), |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())
    }

    pub fn move_key(&mut self, old: &str, new: &str) -> Result<(), String> {
        self.move_keys(&[(old.to_owned(), new.to_owned())])
    }

    /// Move a complete rename plan together. Local directory/container moves also
    /// relocate all descendant files and archive-entry keys. SQL staging permits
    /// swaps without overwriting another item's metadata or loading the catalog.
    pub fn move_keys(&mut self, moves: &[(String, String)]) -> Result<(), String> {
        if moves.len() > 100_000
            || moves
                .iter()
                .map(|(a, b)| a.len().saturating_add(b.len()))
                .sum::<usize>()
                > 32 * 1024 * 1024
        {
            return Err("目录库迁移计划超过十万条或32MiB上限".into());
        }
        let mut direct = std::collections::HashMap::new();
        let mut local = std::collections::HashMap::new();
        for (old, new) in moves {
            valid_key(old)?;
            valid_key(new)?;
            if direct.insert(old.as_str(), new.as_str()).is_some() {
                return Err("目录库迁移计划包含重复源键".into());
            }
            if let (Ok((old, None)), Ok((new, None))) = (
                serde_json::from_str::<(String, Option<String>)>(old),
                serde_json::from_str::<(String, Option<String>)>(new),
            ) {
                local.insert(
                    normalized_path(&old)
                        .trim_end_matches('/')
                        .to_ascii_lowercase(),
                    new,
                );
            }
        }
        let transaction = self.connection.transaction().map_err(|e| e.to_string())?;
        transaction.execute_batch("CREATE TEMP TABLE IF NOT EXISTS rustview_key_moves(old TEXT PRIMARY KEY,new TEXT UNIQUE NOT NULL,stage TEXT UNIQUE NOT NULL); DELETE FROM rustview_key_moves;").map_err(|e|e.to_string())?;
        let token: String = transaction
            .query_row("SELECT lower(hex(randomblob(16)))", [], |r| r.get(0))
            .map_err(|e| e.to_string())?;
        let mut after = String::new();
        let mut sequence = 0_u64;
        loop {
            let rows = {
                let mut statement = transaction
                    .prepare(
                        "SELECT item_key FROM items WHERE item_key>?1 ORDER BY item_key LIMIT 500",
                    )
                    .map_err(|e| e.to_string())?;
                let rows = statement
                    .query_map([&after], |r| r.get::<_, String>(0))
                    .map_err(|e| e.to_string())?;
                rows.collect::<Result<Vec<_>, _>>()
                    .map_err(|e| e.to_string())?
            };
            if rows.is_empty() {
                break;
            }
            for old in rows {
                after = old.clone();
                let Some(new) = relocated_key(&old, &direct, &local)? else {
                    continue;
                };
                if old == new {
                    continue;
                }
                valid_key(&new)?;
                sequence += 1;
                let stage = format!("rustview-migration:{token}:{sequence}");
                transaction
                    .execute(
                        "INSERT INTO rustview_key_moves(old,new,stage) VALUES(?1,?2,?3)",
                        params![old, new, stage],
                    )
                    .map_err(|e| format!("目录库目标键冲突，原记录未改动：{e}"))?;
            }
        }
        let collision:bool=transaction.query_row("SELECT EXISTS(SELECT 1 FROM rustview_key_moves m JOIN items i ON i.item_key=m.new LEFT JOIN rustview_key_moves moving ON moving.old=i.item_key WHERE moving.old IS NULL)",[],|r|r.get(0)).map_err(|e|e.to_string())?;
        if collision {
            return Err("目录库目标已有其它记录，原记录未改动".into());
        }
        transaction.execute("UPDATE items SET item_key=(SELECT stage FROM rustview_key_moves WHERE old=items.item_key) WHERE item_key IN (SELECT old FROM rustview_key_moves)",[]).map_err(|e|e.to_string())?;
        transaction.execute("UPDATE items SET item_key=(SELECT new FROM rustview_key_moves WHERE stage=items.item_key) WHERE item_key IN (SELECT stage FROM rustview_key_moves)",[]).map_err(|e|e.to_string())?;
        transaction
            .execute("DELETE FROM rustview_key_moves", [])
            .map_err(|e| e.to_string())?;
        transaction.commit().map_err(|e| e.to_string())
    }
}

fn normalized_path(path: &str) -> String {
    let value = path.replace('\\', "/");
    if let Some(rest) = value.strip_prefix("//?/UNC/") {
        format!("//{rest}")
    } else {
        value.strip_prefix("//?/").unwrap_or(&value).to_owned()
    }
}

fn relocated_key(
    key: &str,
    direct: &std::collections::HashMap<&str, &str>,
    local: &std::collections::HashMap<String, String>,
) -> Result<Option<String>, String> {
    if let Some(new) = direct.get(key) {
        return Ok(Some((*new).to_owned()));
    }
    let Ok((source, entry)) = serde_json::from_str::<(String, Option<String>)>(key) else {
        return Ok(None);
    };
    let source_normal = normalized_path(&source);
    let folded = source_normal.to_ascii_lowercase();
    let mut prefix = folded.as_str();
    loop {
        if let Some(new) = local.get(prefix) {
            let suffix = &source_normal[prefix.len()..];
            let suffix = if new.contains('\\') {
                suffix.replace('/', "\\")
            } else {
                suffix.to_owned()
            };
            let output = format!("{}{}", new.trim_end_matches(['/', '\\']), suffix);
            return serde_json::to_string(&(output, entry))
                .map(Some)
                .map_err(|e| e.to_string());
        }
        let Some(end) = prefix.rfind('/') else {
            return Ok(None);
        };
        prefix = &prefix[..end];
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn json_key(path: &str, entry: Option<&str>) -> String {
        serde_json::to_string(&(path, entry)).unwrap()
    }

    #[test]
    fn mark_projection_is_bounded_and_migration_normalizes_extended_windows_paths() {
        let root = tempfile::tempdir().unwrap();
        let mut catalog = Catalog::open(&root.path().join("catalog.sqlite")).unwrap();
        let key = json_key(r"\\?\G:\photos\book.zip", Some("image.jpg"));
        catalog.set_rating(&key, 4).unwrap();
        catalog.set_label(&key, 3).unwrap();
        catalog
            .set_description(&key, &"x".repeat(32 * 1024))
            .unwrap();
        let projected = catalog.get_marks(&[key.clone(), "missing".into()]).unwrap();
        assert_eq!(projected, [(key.clone(), 4, 3)]);
        assert!(catalog.get_marks(&vec![key.clone(); 501]).is_err());
        assert_eq!(
            catalog.path(),
            Some(root.path().join("catalog.sqlite").as_path())
        );
        catalog
            .move_keys(&[(json_key("G:/photos", None), json_key("H:/new", None))])
            .unwrap();
        assert_eq!(
            catalog
                .get(&json_key("H:/new/book.zip", Some("image.jpg")))
                .unwrap()
                .rating,
            4
        );
        assert_eq!(catalog.get(&key).unwrap().rating, 0);
    }

    #[test]
    fn container_and_directory_moves_relocate_all_entries_without_matching_neighbors() {
        let root = tempfile::tempdir().unwrap();
        let mut catalog = Catalog::open(&root.path().join("library.sqlite")).unwrap();
        let archive = json_key(r"G:\photos\book.zip", Some("inside/01.jpg"));
        let neighbor = json_key(r"G:\photos\book.zip2", Some("inside/01.jpg"));
        let child = json_key(r"G:\photos\sub\image.png", None);
        catalog.set_rating(&archive, 5).unwrap();
        catalog.set_rating(&neighbor, 2).unwrap();
        catalog.set_title(&child, "keep").unwrap();
        catalog
            .move_key(
                &json_key(r"G:\photos\book.zip", None),
                &json_key(r"G:\photos\renamed.zip", None),
            )
            .unwrap();
        assert_eq!(catalog.get(&archive).unwrap().rating, 0);
        assert_eq!(
            catalog
                .get(&json_key(r"G:\photos\renamed.zip", Some("inside/01.jpg")))
                .unwrap()
                .rating,
            5
        );
        assert_eq!(catalog.get(&neighbor).unwrap().rating, 2);
        catalog
            .move_key(&json_key(r"G:\photos", None), &json_key(r"H:\moved", None))
            .unwrap();
        assert_eq!(
            catalog
                .get(&json_key(r"H:\moved\sub\image.png", None))
                .unwrap()
                .title,
            "keep"
        );
        assert_eq!(
            catalog
                .get(&json_key(r"H:\moved\renamed.zip", Some("inside/01.jpg")))
                .unwrap()
                .rating,
            5
        );
    }

    #[test]
    fn batch_key_swaps_are_atomic_and_collisions_keep_original_metadata() {
        let root = tempfile::tempdir().unwrap();
        let mut catalog = Catalog::open(&root.path().join("library.sqlite")).unwrap();
        let a = json_key("G:/a.jpg", None);
        let b = json_key("G:/b.jpg", None);
        let c = json_key("G:/c.jpg", None);
        catalog.set_rating(&a, 1).unwrap();
        catalog.set_rating(&b, 2).unwrap();
        catalog.set_rating(&c, 3).unwrap();
        catalog
            .move_keys(&[(a.clone(), b.clone()), (b.clone(), a.clone())])
            .unwrap();
        assert_eq!(catalog.get(&a).unwrap().rating, 2);
        assert_eq!(catalog.get(&b).unwrap().rating, 1);
        assert!(catalog
            .move_keys(&[
                (a.clone(), c.clone()),
                (b.clone(), json_key("G:/d.jpg", None))
            ])
            .is_err());
        assert_eq!(catalog.get(&a).unwrap().rating, 2);
        assert_eq!(catalog.get(&b).unwrap().rating, 1);
        assert_eq!(catalog.get(&c).unwrap().rating, 3);
    }

    #[test]
    fn directory_key_migration_pages_without_skipping_rows() {
        let root = tempfile::tempdir().unwrap();
        let mut catalog = Catalog::open(&root.path().join("library.sqlite")).unwrap();
        let keys = (0..1203)
            .map(|n| json_key(&format!("G:/old/{n}.jpg"), None))
            .collect::<Vec<_>>();
        catalog.update_many(&keys, Change::Rating(4)).unwrap();
        catalog
            .move_key(&json_key("G:/old", None), &json_key("G:/new", None))
            .unwrap();
        for n in [0, 499, 500, 1000, 1202] {
            assert_eq!(
                catalog
                    .get(&json_key(&format!("G:/new/{n}.jpg"), None))
                    .unwrap()
                    .rating,
                4
            );
        }
        assert!(catalog
            .list(Filter::All, 500, 1000)
            .unwrap()
            .iter()
            .all(|(key, _)| !key.contains("G:/old")));
    }

    #[test]
    fn persists_file_and_archive_entry_records_independently() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("catalog.sqlite3");
        let file = "C:/images/a.jpg::";
        let archive = "C:/images/book.zip::a.jpg";
        {
            let mut catalog = Catalog::open(&path).unwrap();
            catalog.set_rating(file, 5).unwrap();
            catalog.set_label(archive, 2).unwrap();
            catalog.toggle_mark(archive).unwrap();
            catalog.set_category(archive, "旅行").unwrap();
            catalog.set_basket(archive, true).unwrap();
        }
        let catalog = Catalog::open(&path).unwrap();
        assert_eq!(catalog.get(file).unwrap().rating, 5);
        assert_eq!(
            catalog.get(archive).unwrap(),
            Record {
                rating: 0,
                label: 2,
                marked: true,
                category: "旅行".into(),
                basket: true,
                ..Default::default()
            }
        );
        assert_eq!(catalog.categories(50).unwrap(), vec!["旅行"]);
    }

    #[test]
    fn archive_resume_keeps_exact_member_paths_across_catalogs_and_reopen() {
        let directory = tempfile::tempdir().unwrap();
        let database = directory.path().join("catalog.sqlite3");
        let first = directory.path().join("旅行.zip");
        let second = directory.path().join("旅行.rar");
        std::fs::write(&first, []).unwrap();
        std::fs::write(&second, []).unwrap();
        {
            let earlier = UNIX_EPOCH + Duration::from_secs(1);
            let later = UNIX_EPOCH + Duration::from_secs(2);
            let mut writer = Catalog::open(&database).unwrap();
            let mut reader = Catalog::open(&database).unwrap();
            assert_eq!(reader.archive_resume(&first).unwrap(), None);
            writer
                .remember_archive_resume(&first, "第一章/cover.png", earlier)
                .unwrap();
            writer
                .remember_archive_resume(&second, "独立/cover.png", earlier)
                .unwrap();
            writer
                .remember_archive_resume(&first, "第二章/cover.png", later)
                .unwrap();
            reader
                .remember_archive_resume(&first, "第一章/cover.png", earlier)
                .unwrap();
            assert_eq!(
                reader.archive_resume(&first).unwrap().as_deref(),
                Some("第二章/cover.png")
            );
        }
        let catalog = Catalog::open(&database).unwrap();
        assert_eq!(
            catalog
                .archive_resume(&directory.path().join(".").join("旅行.zip"))
                .unwrap()
                .as_deref(),
            Some("第二章/cover.png")
        );
        assert_eq!(
            catalog.archive_resume(&second).unwrap().as_deref(),
            Some("独立/cover.png")
        );
    }

    #[test]
    fn batch_changes_are_atomic_and_queries_are_bounded() {
        let directory = tempfile::tempdir().unwrap();
        let mut catalog = Catalog::open(&directory.path().join("catalog.sqlite3")).unwrap();
        let keys: Vec<_> = (0..700).map(|n| format!("image-{n}")).collect();
        catalog.update_many(&keys, Change::Rating(3)).unwrap();
        assert_eq!(
            catalog
                .list(Filter::Rating(3), usize::MAX, 0)
                .unwrap()
                .len(),
            500
        );
        assert_eq!(
            catalog.list(Filter::Rating(3), 500, 500).unwrap().len(),
            200
        );
        assert!(catalog
            .update_many(&["image-0".into(), "".into()], Change::Rating(5))
            .is_err());
        assert_eq!(catalog.get("image-0").unwrap().rating, 3);
        assert!(catalog.set_label("image-0", 6).is_err());
    }

    #[test]
    fn reset_removes_empty_records_and_keys_move_without_overwriting() {
        let directory = tempfile::tempdir().unwrap();
        let mut catalog = Catalog::open(&directory.path().join("catalog.sqlite3")).unwrap();
        catalog.set_basket("old", true).unwrap();
        catalog.move_key("old", "new").unwrap();
        assert!(catalog.get("new").unwrap().basket);
        assert_eq!(catalog.get("old").unwrap(), Record::default());
        catalog.set_basket("new", false).unwrap();
        assert!(catalog.list(Filter::All, 500, 0).unwrap().is_empty());
    }

    #[test]
    fn legacy_schema_migrates_and_default_metadata_preset_persists() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("old.sqlite3");
        let old = Connection::open(&path).unwrap();
        old.execute_batch("CREATE TABLE items(item_key TEXT PRIMARY KEY NOT NULL,rating INTEGER NOT NULL,label INTEGER NOT NULL,marked INTEGER NOT NULL,category TEXT NOT NULL,basket INTEGER NOT NULL,updated INTEGER NOT NULL,basket_added INTEGER NOT NULL);INSERT INTO items VALUES('old',4,1,1,'原类别',1,0,0);").unwrap();
        drop(old);
        {
            let mut catalog = Catalog::open(&path).unwrap();
            assert_eq!(catalog.get("old").unwrap().rating, 4);
            assert!(catalog.default_preset().unwrap().is_none());
            let preset = Record {
                rating: 5,
                label: 3,
                category: "新类别".into(),
                title: "标题".into(),
                description: "说明".into(),
                keywords: "旅行;海边".into(),
                iptc_keywords: "location".into(),
                ..Default::default()
            };
            catalog.set_default_preset(&preset).unwrap();
            catalog
                .apply_default(&["old".into(), "new".into()])
                .unwrap();
            let updated = catalog.get("old").unwrap();
            assert!(updated.marked && updated.basket);
            assert_eq!(updated.title, "标题");
            assert_eq!(updated.keywords, "旅行;海边");
            assert_eq!(catalog.get("new").unwrap().iptc_keywords, "location");
        }
        let catalog = Catalog::open(&path).unwrap();
        assert_eq!(
            catalog.default_preset().unwrap().unwrap().description,
            "说明"
        );
        assert_eq!(catalog.get("old").unwrap().category, "新类别");
    }
}
