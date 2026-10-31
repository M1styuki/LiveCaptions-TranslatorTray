use chrono::{DateTime, Local, LocalResult, NaiveDate, NaiveDateTime, TimeZone, Utc};
pub struct Converted {
    pub unix_seconds: i64,
    pub assumed_local: bool,
    pub ambiguous_local: bool,
    pub already_numeric: bool,
}
pub fn legacy(text: &str) -> Option<Converted> {
    if let Ok(value) = text.trim().parse::<i64>()
        && DateTime::from_timestamp(value, 0).is_some()
    {
        return Some(Converted {
            unix_seconds: value,
            assumed_local: false,
            ambiguous_local: false,
            already_numeric: true,
        });
    }
    let offset = DateTime::parse_from_rfc3339(text)
        .or_else(|_| DateTime::parse_from_rfc2822(text))
        .or_else(|_| DateTime::parse_from_str(text, "%Y-%m-%d %H:%M:%S%.f %:z"));
    if let Ok(value) = offset {
        return Some(Converted {
            unix_seconds: value.timestamp(),
            assumed_local: false,
            ambiguous_local: false,
            already_numeric: false,
        });
    }
    let parsed = [
        "%Y-%m-%d %H:%M:%S%.f",
        "%Y-%m-%dT%H:%M:%S%.f",
        "%Y/%m/%d %H:%M:%S%.f",
        "%m/%d/%Y %H:%M:%S%.f",
    ]
    .into_iter()
    .find_map(|format| NaiveDateTime::parse_from_str(text, format).ok())
    .or_else(|| {
        NaiveDate::parse_from_str(text, "%Y-%m-%d")
            .ok()
            .and_then(|date| date.and_hms_opt(0, 0, 0))
    })?;
    let (unix_seconds, ambiguous_local) = match Local.from_local_datetime(&parsed) {
        LocalResult::Single(value) => (value.timestamp(), false),
        LocalResult::Ambiguous(first, second) => (first.timestamp().min(second.timestamp()), true),
        LocalResult::None => return None,
    };
    Some(Converted {
        unix_seconds,
        assumed_local: true,
        ambiguous_local,
        already_numeric: false,
    })
}
pub fn display(timestamp: &str, seconds: bool) -> String {
    let Some(value) = timestamp
        .trim()
        .parse::<i64>()
        .ok()
        .and_then(|value| DateTime::<Utc>::from_timestamp(value, 0))
    else {
        return timestamp.into();
    };
    value
        .with_timezone(&Local)
        .format(if seconds {
            "%Y-%m-%d %H:%M:%S"
        } else {
            "%Y-%m-%d %H:%M"
        })
        .to_string()
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn explicit_timezone_dates_normalize_but_unparseable_values_are_retained_for_display() {
        let value = legacy("2026-10-26T12:34:56+08:00").unwrap();
        assert_eq!(value.unix_seconds, 1792989296);
        assert!(!value.assumed_local);
        assert!(!value.already_numeric);
        assert!(legacy("2026-10-26 12:34:56").unwrap().assumed_local);
        assert!(legacy("1792989296").unwrap().already_numeric);
        assert!(legacy("not-a-date").is_none());
        assert_eq!(display("not-a-date", true), "not-a-date");
        assert_eq!(display("", true), "");
    }
}
