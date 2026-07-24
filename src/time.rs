use anyhow::{anyhow, Result};
use chrono::{
    DateTime, Datelike, Duration, Local, LocalResult, NaiveDate, NaiveDateTime, NaiveTime, TimeZone,
    Utc,
};

/// Current time as UTC milliseconds.
pub fn now_ms() -> i64 {
    Utc::now().timestamp_millis()
}

/// Format a UTC-ms timestamp for display in the user's local timezone.
/// `None` renders as "-".
pub fn format_local(ms: Option<i64>) -> String {
    match ms {
        None => "-".to_string(),
        Some(ms) => match Utc.timestamp_millis_opt(ms).single() {
            Some(dt) => dt.with_timezone(&Local).format("%Y-%m-%d %H:%M").to_string(),
            None => ms.to_string(),
        },
    }
}

fn local_to_utc_ms(nd: NaiveDateTime) -> Result<i64> {
    let local_dt = match Local.from_local_datetime(&nd) {
        LocalResult::Single(dt) => dt,
        LocalResult::Ambiguous(a, _b) => a,
        LocalResult::None => return Err(anyhow!("invalid local time: {}", nd)),
    };
    Ok(local_dt.with_timezone(&Utc).timestamp_millis())
}

/// Parse a human-friendly time string into UTC milliseconds.
/// Supports:
///   "now"
///   "+2h" / "+30m" / "+1d" / "+1w"            (relative to now)
///   "today" / "tomorrow" [ "HH:MM" ]
///   "HH:MM"                                  (today)
///   "2026-07-24"                             (date)
///   "2026-07-24 14:30" / "2026-07-24T14:30"  (datetime)
pub fn parse_time(s: &str) -> Result<i64> {
    let s = s.trim();
    let now = Local::now();

    if s == "now" {
        return Ok(now.with_timezone(&Utc).timestamp_millis());
    }

    if let Some(rest) = s.strip_prefix('+') {
        let (num, unit) = split_number_unit(rest)?;
        let dur = match unit {
            'h' => Duration::hours(num),
            'm' => Duration::minutes(num),
            'd' => Duration::days(num),
            'w' => Duration::weeks(num),
            _ => return Err(anyhow!("unsupported relative unit '{}' (use h/m/d/w)", unit)),
        };
        return Ok((now + dur).with_timezone(&Utc).timestamp_millis());
    }

    if let Some(stripped) = s.strip_prefix("today") {
        let time = parse_optional_time(stripped.trim(), NaiveTime::from_hms_opt(0, 0, 0).unwrap())?;
        return local_to_utc_ms(now.date_naive().and_time(time));
    }
    if let Some(stripped) = s.strip_prefix("tomorrow") {
        let time = parse_optional_time(stripped.trim(), NaiveTime::from_hms_opt(0, 0, 0).unwrap())?;
        let tomorrow = (now + Duration::days(1)).date_naive();
        return local_to_utc_ms(tomorrow.and_time(time));
    }

    // pure time "HH:MM" => today
    if s.contains(':') && !s.contains('-') {
        if let Ok(t) = NaiveTime::parse_from_str(s, "%H:%M") {
            return local_to_utc_ms(now.date_naive().and_time(t));
        }
    }

    let s_norm = s.replace('T', " ");
    if let Ok(dt) = NaiveDateTime::parse_from_str(&s_norm, "%Y-%m-%d %H:%M") {
        return local_to_utc_ms(dt);
    }
    if let Ok(d) = NaiveDate::parse_from_str(&s_norm, "%Y-%m-%d") {
        return local_to_utc_ms(d.and_time(NaiveTime::from_hms_opt(0, 0, 0).unwrap()));
    }

    Err(anyhow!("could not parse time: '{}'", s))
}

fn split_number_unit(s: &str) -> Result<(i64, char)> {
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    if i == 0 {
        return Err(anyhow!("invalid relative time: '{}'", s));
    }
    let num: i64 = s[..i]
        .parse()
        .map_err(|_| anyhow!("invalid relative time: '{}'", s))?;
    let unit = s[i..]
        .chars()
        .next()
        .ok_or_else(|| anyhow!("missing unit in '{}'", s))?;
    Ok((num, unit))
}

fn parse_optional_time(s: &str, default: NaiveTime) -> Result<NaiveTime> {
    let s = s.trim();
    if s.is_empty() {
        return Ok(default);
    }
    NaiveTime::parse_from_str(s, "%H:%M").map_err(|_| anyhow!("invalid time: '{}'", s))
}

/// Minimal, self-contained RRULE expansion (no external crate).
/// Supports FREQ=DAILY|WEEKLY|MONTHLY with INTERVAL, COUNT, UNTIL.
/// `anchor_ms` is the task's scheduled_start_at (UTC ms). Occurrences start at
/// the anchor (inclusive) and stop at COUNT / `limit` / UNTIL.
pub fn rrule_occurrences(rrule: &str, anchor_ms: i64, limit: usize) -> Result<Vec<i64>> {
    let anchor = Utc
        .timestamp_millis_opt(anchor_ms)
        .single()
        .ok_or_else(|| anyhow!("invalid anchor timestamp"))?;

    let mut freq = "DAILY".to_string();
    let mut interval: i64 = 1;
    let mut count: i64 = limit as i64;
    let mut until_ms: Option<i64> = None;
    let mut byday: Vec<chrono::Weekday> = Vec::new();

    for part in rrule.split(';') {
        if part.is_empty() {
            continue;
        }
        let (k, v) = part.split_once('=').unwrap_or((part, ""));
        match k.to_uppercase().as_str() {
            "FREQ" => freq = v.to_uppercase(),
            "INTERVAL" => interval = v.parse().unwrap_or(1).max(1),
            "COUNT" => count = v.parse().unwrap_or(limit as i64).max(1),
            "UNTIL" => until_ms = Some(parse_until(v)?),
            "BYDAY" => {
                for d in v.split(',') {
                    match d.trim().to_uppercase().as_str() {
                        "MO" => byday.push(chrono::Weekday::Mon),
                        "TU" => byday.push(chrono::Weekday::Tue),
                        "WE" => byday.push(chrono::Weekday::Wed),
                        "TH" => byday.push(chrono::Weekday::Thu),
                        "FR" => byday.push(chrono::Weekday::Fri),
                        "SA" => byday.push(chrono::Weekday::Sat),
                        "SU" => byday.push(chrono::Weekday::Sun),
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }

    let mut out = Vec::new();
    let mut cur = anchor;
    let max_iter = (count as usize).min(limit);
    
    use chrono::Datelike;
    if freq == "WEEKLY" && !byday.is_empty() {
        let mut occurrences_found = 0;
        let mut current_day = cur;
        
        // Always include the anchor if it's the very first explicitly scheduled occurrence,
        // but it's more standard to only include days that match BYDAY.
        // We'll iterate up to max_iter occurrences matching BYDAY.
        while occurrences_found < max_iter {
            if byday.contains(&current_day.weekday()) {
                let ms = current_day.timestamp_millis();
                if let Some(u) = until_ms {
                    if ms > u { break; }
                }
                out.push(ms);
                occurrences_found += 1;
            }
            current_day = current_day + chrono::Duration::days(1);
            if current_day.weekday() == chrono::Weekday::Mon && interval > 1 {
                current_day = current_day + chrono::Duration::weeks(interval - 1);
            }
        }
    } else {
        for _ in 0..max_iter {
            let ms = cur.timestamp_millis();
            if let Some(u) = until_ms {
                if ms > u {
                    break;
                }
            }
            out.push(ms);
            cur = step(cur, &freq, interval)?;
        }
    }
    Ok(out)
}

fn step(dt: DateTime<Utc>, freq: &str, interval: i64) -> Result<DateTime<Utc>> {
    match freq {
        "DAILY" => Ok(dt + Duration::days(interval)),
        "WEEKLY" => Ok(dt + Duration::weeks(interval)),
        "MONTHLY" => Ok(add_months(dt, interval)),
        _ => Err(anyhow!("unsupported FREQ in RRULE: {}", freq)),
    }
}

fn add_months(dt: DateTime<Utc>, interval: i64) -> DateTime<Utc> {
    let d = dt.date_naive();
    let total = d.year() as i64 * 12 + (d.month() as i64 - 1) + interval;
    let y = (total / 12) as i32;
    let m = ((total % 12 + 12) % 12 + 1) as u32;
    let last_day = days_in_month(y, m);
    let day = d.day().min(last_day);
    let nd = NaiveDate::from_ymd_opt(y, m, day)
        .unwrap_or(d)
        .and_time(dt.time());
    Utc.from_utc_datetime(&nd)
}

fn days_in_month(y: i32, m: u32) -> u32 {
    let (year, month) = if m == 12 { (y + 1, 1) } else { (y, m + 1) };
    let first_next = NaiveDate::from_ymd_opt(year, month, 1).unwrap();
    let this_first = NaiveDate::from_ymd_opt(y, m, 1).unwrap();
    (first_next - this_first).num_days() as u32
}

fn parse_until(v: &str) -> Result<i64> {
    if let Ok(ms) = v.parse::<i64>() {
        return Ok(ms);
    }
    let norm = v.replace('T', " ");
    if let Ok(dt) = NaiveDateTime::parse_from_str(&norm, "%Y-%m-%d %H:%M") {
        return local_to_utc_ms(dt);
    }
    if let Ok(dt) = NaiveDateTime::parse_from_str(&norm, "%Y%m%dT%H%M%SZ") {
        return Ok(Utc.from_utc_datetime(&dt).timestamp_millis());
    }
    Err(anyhow!("invalid RRULE UNTIL: '{}'", v))
}
