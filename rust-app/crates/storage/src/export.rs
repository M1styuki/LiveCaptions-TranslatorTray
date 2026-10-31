use crate::{
    atomic_file,
    history::{Database, Error},
    timestamp,
};
use std::{
    io::{self, Write},
    path::Path,
};
pub const HEADERS: [&str; 5] = [
    "Timestamp",
    "SourceText",
    "TranslatedText",
    "TargetLanguage",
    "ApiUsed",
];
fn csv_error(error: csv::Error) -> io::Error {
    match error.into_kind() {
        csv::ErrorKind::Io(error) => error,
        _ => io::Error::other("CSV 文件生成失败。"),
    }
}
pub fn all_history(database: &Database, target: &Path) -> io::Result<u64> {
    let mut count = 0;
    atomic_file::write(target, |file| {
        file.write_all(&[0xef, 0xbb, 0xbf])?;
        let mut writer = csv::WriterBuilder::new()
            .terminator(csv::Terminator::CRLF)
            .from_writer(file);
        writer.write_record(HEADERS).map_err(csv_error)?;
        database
            .all(|row| {
                let timestamp = timestamp::display(&row.timestamp, true);
                writer
                    .write_record([
                        timestamp.as_str(),
                        &row.source,
                        &row.translation,
                        &row.target_language,
                        &row.api_used,
                    ])
                    .map_err(csv_error)
                    .map_err(Error::Io)?;
                count += 1;
                Ok(())
            })
            .map_err(io::Error::other)?;
        writer.flush()?;
        Ok(())
    })?;
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::params;
    use std::fs;
    #[test]
    fn export_streams_all_rows_with_bom_five_columns_crlf_and_unicode_quoting() {
        let evidence = std::env::var_os("LIVE_CAPTIONS_CSV_EVIDENCE_DIR");
        let directory = evidence
            .clone()
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| {
                std::env::temp_dir().join(format!(
                    "LiveCaptionsTray-csv-test-{}-{}",
                    std::process::id(),
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_nanos()
                ))
            });
        fs::create_dir_all(&directory).unwrap();
        let database_path = directory.join("fixture.db");
        assert!(
            !database_path.exists(),
            "fixture evidence directory must be fresh"
        );
        let database = Database::open(&database_path, "csv-fixture".into()).unwrap();
        for index in 0..35 {
            let source = if index == 0 {
                "中文 😀, \"quoted\"\nline two\r\nline three".into()
            } else if index == 1 {
                String::new()
            } else {
                format!("Public fixture {index}")
            };
            let translated = if index == 2 {
                String::new()
            } else {
                format!("译文，{index} 😀")
            };
            let timestamp = if index == 3 {
                "unparseable-old-date".into()
            } else {
                (1792989296 + index / 5).to_string()
            };
            database.connection.execute("INSERT INTO TranslationHistory(Timestamp,SourceText,TranslatedText,TargetLanguage,ApiUsed) VALUES(?1,?2,?3,?4,?5)",params![timestamp,source,translated,"zh-CN","PublicFixture"]).unwrap();
        }
        database
            .connection
            .execute("INSERT INTO TranslationHistory DEFAULT VALUES", [])
            .unwrap();
        assert_eq!(database.page("", 1, 30).unwrap().total, 36);
        assert_eq!(database.page("fixture 10", 1, 30).unwrap().total, 1);
        let target = directory.join("history.csv");
        assert_eq!(all_history(&database, &target).unwrap(), 36);
        let bytes = fs::read(&target).unwrap();
        assert!(bytes.starts_with(
            b"\xef\xbb\xbfTimestamp,SourceText,TranslatedText,TargetLanguage,ApiUsed\r\n"
        ));
        let mut reader = csv::ReaderBuilder::new().from_reader(&bytes[3..]);
        assert_eq!(
            reader.headers().unwrap().iter().collect::<Vec<_>>(),
            HEADERS
        );
        let records = reader.records().collect::<Result<Vec<_>, _>>().unwrap();
        assert_eq!(records.len(), 36);
        assert!(
            records
                .iter()
                .any(|record| record.get(1) == Some("中文 😀, \"quoted\"\nline two\r\nline three"))
        );
        drop(database);
        if evidence.is_none() {
            fs::remove_file(target).unwrap();
            fs::remove_file(database_path).unwrap();
            fs::remove_dir(directory).unwrap();
        }
    }
}
