use crate::worker::{self, Entry};
use std::{
    cmp::Ordering,
    fs,
    path::Path,
    sync::atomic::{AtomicBool, Ordering::Relaxed},
};

const NEIGHBOR_COUNT: usize = 8;
const INDEX_LIMIT: usize = 32 * 1024 * 1024;

pub(super) fn scan(
    path: &Path,
    opened_name: &str,
    cancel: &AtomicBool,
    publish_neighbors: impl FnOnce(Vec<Entry>),
) -> Result<Vec<Entry>, String> {
    check_cancel(cancel)?;
    let mut entries = Vec::new();
    let mut bytes = 1024; // Allow for the worker reply envelope as well as entry records.
    for item in fs::read_dir(path).map_err(|error| error.to_string())? {
        check_cancel(cancel)?;
        let Ok(item) = item else {
            continue;
        };
        let Ok(metadata) = item.metadata() else {
            continue;
        };
        let Some(entry) =
            worker::local_entry(item.file_name().to_string_lossy().into_owned(), metadata)
        else {
            continue;
        };
        if entries.len() == worker::MAX_ENTRIES {
            return Err("目录超过十万条，当前版本请打开更小的子目录".into());
        }
        // Keep the directory worker's reply limit without allocating JSON.
        bytes += index_bytes(&entry);
        if bytes > INDEX_LIMIT {
            return Err("目录索引超过读取预算，请打开更小的子目录".into());
        }
        entries.push(entry);
    }
    check_cancel(cancel)?;
    let nearby = neighbors(&entries, opened_name, cancel)?;
    if !nearby.is_empty() {
        publish_neighbors(nearby);
    }
    check_cancel(cancel)?;
    entries.sort_by(|a, b| {
        b.directory
            .cmp(&a.directory)
            .then_with(|| natord::compare(&a.name, &b.name))
    });
    check_cancel(cancel)?;
    Ok(entries)
}

fn index_bytes(entry: &Entry) -> usize {
    let digits = |number: u64| number.checked_ilog10().map_or(1, |log| log as usize + 1);
    let modified = entry.modified.map_or(4, |time| {
        digits(time.unsigned_abs()) + usize::from(time.is_negative())
    });
    let name = entry
        .name
        .bytes()
        .map(|byte| match byte {
            b'\t' | b'\r' | b'\n' | 8 | 12 | b'\\' | b'"' => 2,
            0..=31 => 6,
            _ => 1,
        })
        .sum::<usize>();
    // Fixed JSON field names, syntax, boolean values and one entry separator.
    66 - usize::from(entry.directory) - usize::from(entry.archive)
        + digits(entry.size)
        + modified
        + name
}

fn check_cancel(cancel: &AtomicBool) -> Result<(), String> {
    if cancel.load(Relaxed) {
        Err("已取消".into())
    } else {
        Ok(())
    }
}

fn neighbors(
    entries: &[Entry],
    opened_name: &str,
    cancel: &AtomicBool,
) -> Result<Vec<Entry>, String> {
    let image = |entry: &Entry| !entry.directory && !entry.archive;
    let anchor = entries
        .iter()
        .position(|entry| image(entry) && entry.name == opened_name)
        .or_else(|| {
            if !cfg!(windows) {
                return None;
            }
            let opened_name = opened_name.to_lowercase();
            entries
                .iter()
                .position(|entry| image(entry) && entry.name.to_lowercase() == opened_name)
        });
    let Some(anchor) = anchor else {
        return Ok(Vec::new());
    };

    // The full sort is stable. Compare the enumeration indices on natural-sort
    // ties so whitespace-equivalent names neighbor the same files here.
    let compare = |a: &usize, b: &usize| {
        natord::compare(&entries[*a].name, &entries[*b].name).then_with(|| a.cmp(b))
    };
    let mut before = Vec::new();
    let mut after = Vec::new();
    for (index, entry) in entries.iter().enumerate() {
        check_cancel(cancel)?;
        if !image(entry) {
            continue;
        }
        match compare(&index, &anchor) {
            Ordering::Less => before.push(index),
            Ordering::Greater => after.push(index),
            Ordering::Equal => {}
        }
    }
    if before.len() > NEIGHBOR_COUNT {
        let start = before.len() - NEIGHBOR_COUNT;
        before.select_nth_unstable_by(start, compare);
        before = before.split_off(start);
    }
    if after.len() > NEIGHBOR_COUNT {
        after.select_nth_unstable_by(NEIGHBOR_COUNT, compare);
        after.truncate(NEIGHBOR_COUNT);
    }
    before.sort_unstable_by(compare);
    after.sort_unstable_by(compare);
    check_cancel(cancel)?;
    Ok(before
        .into_iter()
        .chain([anchor])
        .chain(after)
        .map(|index| entries[index].clone())
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image(name: String) -> Entry {
        Entry {
            name,
            size: 10,
            directory: false,
            archive: false,
            modified: None,
        }
    }

    #[test]
    fn neighbors_match_the_final_order_even_on_natural_sort_ties() {
        let mut entries: Vec<_> = (1..=70)
            .rev()
            .map(|i| image(format!("图 {i}.jpg")))
            .collect();
        entries.insert(10, image("图18.jpg".into()));
        entries.insert(
            3,
            Entry {
                name: "图 20.jpg".into(),
                directory: true,
                ..image(String::new())
            },
        );
        entries.insert(
            7,
            Entry {
                name: "图 21.zip".into(),
                archive: true,
                ..image(String::new())
            },
        );
        let cancel = AtomicBool::new(false);
        for opened in ["图 1.jpg", "图 18.jpg", "图 42.jpg", "图 70.jpg"] {
            let actual = neighbors(&entries, opened, &cancel).unwrap();
            let mut ordered: Vec<_> = entries
                .iter()
                .filter(|entry| !entry.directory && !entry.archive)
                .collect();
            ordered.sort_by(|a, b| natord::compare(&a.name, &b.name));
            let wanted = ordered
                .iter()
                .position(|entry| entry.name == opened)
                .unwrap();
            let expected = &ordered[wanted.saturating_sub(NEIGHBOR_COUNT)
                ..(wanted + NEIGHBOR_COUNT + 1).min(ordered.len())];
            assert_eq!(
                actual.iter().map(|entry| &entry.name).collect::<Vec<_>>(),
                expected.iter().map(|entry| &entry.name).collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn index_budget_counts_json_escaping_and_numeric_fields() {
        let sample = Entry {
            name: "图\"a\\b\n\x01.jpg".into(),
            size: u64::MAX,
            directory: true,
            archive: false,
            modified: Some(i64::MIN),
        };
        let expected = serde_json::to_vec(&sample).unwrap().len() + 1;
        assert_eq!(index_bytes(&sample), expected);
    }

    #[test]
    fn cancelling_after_neighbors_prevents_delivery_of_the_full_list() {
        let folder = tempfile::tempdir().unwrap();
        for name in ["图2.jpg", "图10.jpg", "图100.jpg", "skip.txt"] {
            fs::write(folder.path().join(name), b"image or text").unwrap();
        }
        let cancel = AtomicBool::new(false);
        let result = scan(folder.path(), "图10.jpg", &cancel, |entries| {
            assert_eq!(
                entries
                    .into_iter()
                    .map(|entry| entry.name)
                    .collect::<Vec<_>>(),
                ["图2.jpg", "图10.jpg", "图100.jpg"]
            );
            cancel.store(true, Relaxed);
        });
        assert_eq!(result.err().as_deref(), Some("已取消"));
    }
}
