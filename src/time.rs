use anyhow::{anyhow, Result};
use chrono::{
    DateTime, Datelike, Duration, Local, LocalResult, NaiveDate, NaiveDateTime, NaiveTime,
    TimeZone, Utc,
};

/// Current time as UTC milliseconds.
pub fn now_ms() -> i64 {
    Utc::now().timestamp_millis()
}

/// Local-day boundaries in UTC ms for a day offset (0 = today, 1 = tomorrow).
/// Returns `(start, end)` where `start` is local midnight and `end` is
/// 23:59:59.999 of the same day (both inclusive).
pub fn local_day_bounds(offset_days: i64) -> (i64, i64) {
    let day = Local::now().date_naive() + Duration::days(offset_days);
    let start =
        local_to_utc_ms(day.and_time(NaiveTime::from_hms_opt(0, 0, 0).unwrap())).unwrap_or(0);
    let end =
        local_to_utc_ms(day.and_time(NaiveTime::from_hms_milli_opt(23, 59, 59, 999).unwrap()))
            .unwrap_or(0);
    (start, end)
}

/// Format a UTC-ms timestamp for display in the user's local timezone.
/// `None` renders as "-".
pub fn format_local(ms: Option<i64>) -> String {
    match ms {
        None => "-".to_string(),
        Some(ms) => match Utc.timestamp_millis_opt(ms).single() {
            Some(dt) => dt
                .with_timezone(&Local)
                .format("%Y-%m-%d %H:%M")
                .to_string(),
            None => ms.to_string(),
        },
    }
}

/// Format a UTC-ms timestamp as a single whitespace-free token for the quick-add
/// grammar (`~YYYY-MM-DDTHH:MM`), so `parse_time` can round-trip it back exactly.
pub fn format_quick_time(ms: i64) -> String {
    match Utc.timestamp_millis_opt(ms).single() {
        Some(dt) => dt
            .with_timezone(&Local)
            .format("%Y-%m-%dT%H:%M")
            .to_string(),
        None => ms.to_string(),
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

/// Local-midnight (UTC ms) of the day containing `ms`. Used to classify a
/// timestamp by calendar day rather than a fixed 24h window.
fn day_start_ms(ms: i64) -> i64 {
    Utc.timestamp_millis_opt(ms)
        .single()
        .map(|dt| {
            let d = dt.with_timezone(&Local).date_naive();
            local_to_utc_ms(d.and_time(NaiveTime::from_hms_opt(0, 0, 0).unwrap())).unwrap_or(ms)
        })
        .unwrap_or(ms)
}

/// Whole calendar days between two timestamps (negative when `to` < `from`).
fn days_between(from_ms: i64, to_ms: i64) -> i64 {
    (day_start_ms(to_ms) - day_start_ms(from_ms)) / (24 * 3600 * 1000i64)
}

/// Compact relative description of a due/scheduled timestamp for list rows.
/// Returns `None` when `ms` is `None`. Examples: "逾期2天", "明天", "3天后",
/// "逾期5小时", "2分钟后". Past timestamps uniformly report how overdue they are
/// (逾期X分钟/小时/天); within ±24h it reports precise minutes/hours, beyond
/// that it classifies by local calendar day.
pub fn relative_due(lang: crate::i18n::Lang, ms: Option<i64>) -> Option<String> {
    let ms = ms?;
    let now = now_ms();
    let diff = ms - now;
    let day_ms = 24 * 3600 * 1000i64;

    if diff.abs() < day_ms {
        let hours = diff as f64 / (3600.0 * 1000.0);
        if hours.abs() < 1.0 {
            let m = (hours * 60.0).abs().round().max(1.0) as i64;
            return Some(if diff < 0 {
                crate::tr!(lang, "逾期{}分钟", "{}m overdue", m)
            } else {
                crate::tr!(lang, "{}分钟后", "in {}m", m)
            });
        }
        let h = hours.abs().round() as i64;
        return Some(if diff < 0 {
            crate::tr!(lang, "逾期{}小时", "{}h overdue", h)
        } else {
            crate::tr!(lang, "{}小时后", "in {}h", h)
        });
    }

    let d = days_between(now, ms);
    Some(if d >= 1 {
        if d == 1 {
            crate::tr!(lang, "明天", "tomorrow").to_string()
        } else {
            crate::tr!(lang, "{}天后", "in {}d", d)
        }
    } else {
        crate::tr!(lang, "逾期{}天", "{}d overdue", -d)
    })
}

/// Compact relative description of when a task was completed, for the Done view.
/// Returns `None` when `ms` is `None`. Examples: "3分钟前", "2小时前", "昨天", "3天前".
pub fn relative_past(lang: crate::i18n::Lang, ms: Option<i64>) -> Option<String> {
    let ms = ms?;
    let now = now_ms();
    let diff = now - ms;
    if diff < 0 {
        return None;
    }
    let day_ms = 24 * 3600 * 1000i64;
    if diff < day_ms {
        let hours = diff as f64 / (3600.0 * 1000.0);
        if hours < 1.0 {
            let m = (hours * 60.0).round().max(1.0) as i64;
            return Some(crate::tr!(lang, "{}分钟前", "{}m ago", m));
        }
        let h = hours.round() as i64;
        return Some(crate::tr!(lang, "{}小时前", "{}h ago", h));
    }
    let d = days_between(ms, now);
    if d <= 1 {
        Some(crate::tr!(lang, "昨天", "yesterday").to_string())
    } else {
        Some(crate::tr!(lang, "{}天前", "{}d ago", d))
    }
}

/// Whether a due/scheduled timestamp is overdue (strictly before now).
pub fn is_overdue(ms: Option<i64>) -> bool {
    ms.is_some_and(|m| m < now_ms())
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
    let midnight = NaiveTime::from_hms_opt(0, 0, 0).unwrap();

    if s == "now" {
        return Ok(now.with_timezone(&Utc).timestamp_millis());
    }

    // 相对偏移（+2h / +3d / +1w），可带时刻：+3d 15:30 → 3 天后的 15:30。
    if let Some(rest) = s.strip_prefix('+') {
        let (num, unit, after_unit) = split_number_unit(rest)?;
        let dur = match unit {
            'h' => Duration::hours(num),
            'm' => Duration::minutes(num),
            'd' => Duration::days(num),
            'w' => Duration::weeks(num),
            _ => {
                return Err(anyhow!(
                    "unsupported relative unit '{}' (use h/m/d/w)",
                    unit
                ))
            }
        };
        let base = now + dur;
        let after_unit = after_unit.trim();
        if after_unit.is_empty() {
            return Ok(base.with_timezone(&Utc).timestamp_millis());
        }
        let t = parse_optional_time(after_unit, midnight)?;
        return local_to_utc_ms(base.date_naive().and_time(t));
    }

    // 中文天词：今天/明天/后天（可带 HH:MM）
    if let Some(stripped) = s.strip_prefix("今天") {
        let t = parse_optional_time(stripped.trim(), midnight)?;
        return local_to_utc_ms(now.date_naive().and_time(t));
    }
    if let Some(stripped) = s.strip_prefix("明天") {
        let t = parse_optional_time(stripped.trim(), midnight)?;
        let day = (now + Duration::days(1)).date_naive();
        return local_to_utc_ms(day.and_time(t));
    }
    if let Some(stripped) = s.strip_prefix("后天") {
        let t = parse_optional_time(stripped.trim(), midnight)?;
        let day = (now + Duration::days(2)).date_naive();
        return local_to_utc_ms(day.and_time(t));
    }

    // 星期几：周X / 星期X / 下周X（X ∈ 一~日, 可带 HH:MM）
    if let Some((wd, time_part, next_week)) = parse_cn_weekday(s) {
        let t = parse_optional_time(time_part, midnight)?;
        let today = now.date_naive();
        let delta = (wd.num_days_from_monday() + 7 - today.weekday().num_days_from_monday()) % 7;
        let off = (delta as i64) + if next_week { 7 } else { 0 };
        let day = today + Duration::days(off);
        return local_to_utc_ms(day.and_time(t));
    }

    // English today/tomorrow
    if let Some(stripped) = s.strip_prefix("today") {
        let time = parse_optional_time(stripped.trim(), midnight)?;
        return local_to_utc_ms(now.date_naive().and_time(time));
    }
    if let Some(stripped) = s.strip_prefix("tomorrow") {
        let time = parse_optional_time(stripped.trim(), midnight)?;
        let tomorrow = (now + Duration::days(1)).date_naive();
        return local_to_utc_ms(tomorrow.and_time(time));
    }

    // 斜杠/点/短横线日期（可带 HH:MM）：2026/8/20、8/20、2026.8.20、8-20
    if let Some((date_part, time_part)) = split_date_time(s) {
        if let Some(d) = parse_flex_date(date_part) {
            let t = parse_optional_time(time_part, midnight)?;
            return local_to_utc_ms(d.and_time(t));
        }
    }

    // pure time "HH:MM" => today if still upcoming, otherwise tomorrow
    if s.contains(':') && !s.contains('-') {
        if let Ok(t) = NaiveTime::parse_from_str(s, "%H:%M") {
            let candidate = local_to_utc_ms(now.date_naive().and_time(t))?;
            let now_ms = now.with_timezone(&Utc).timestamp_millis();
            if candidate < now_ms {
                let tomorrow = (now + Duration::days(1)).date_naive();
                return local_to_utc_ms(tomorrow.and_time(t));
            }
            return Ok(candidate);
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

fn split_number_unit(s: &str) -> Result<(i64, char, &str)> {
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
    let after = &s[i + unit.len_utf8()..];
    Ok((num, unit, after))
}

fn single_weekday_char(c: char) -> Option<chrono::Weekday> {
    use chrono::Weekday;
    match c {
        '一' => Some(Weekday::Mon),
        '二' => Some(Weekday::Tue),
        '三' => Some(Weekday::Wed),
        '四' => Some(Weekday::Thu),
        '五' => Some(Weekday::Fri),
        '六' => Some(Weekday::Sat),
        '日' | '天' => Some(Weekday::Sun),
        _ => None,
    }
}

/// 解析中文星期词，返回 (星期几, 剩余时刻串, 是否下周)。
/// 支持 周X / 星期X / 下周X，X ∈ 一~日。
fn parse_cn_weekday(s: &str) -> Option<(chrono::Weekday, &str, bool)> {
    for (prefix, next_week) in [("下周", true), ("星期", false), ("周", false)] {
        if let Some(body) = s.strip_prefix(prefix) {
            let c = body.chars().next()?;
            let wd = single_weekday_char(c)?;
            return Some((wd, body[c.len_utf8()..].trim(), next_week));
        }
    }
    None
}

/// 把 "日期 [HH:MM]" 拆成 (日期部分, 时刻部分)，时刻部分可能为空串。
fn split_date_time(s: &str) -> Option<(&str, &str)> {
    if let Some(sp) = s.rfind(' ') {
        let time = &s[sp + 1..];
        if time.len() <= 5 && time.contains(':') && !time.contains('-') {
            let (h, m) = time.split_once(':')?;
            if !h.is_empty()
                && !m.is_empty()
                && h.chars().all(|c| c.is_ascii_digit())
                && m.chars().all(|c| c.is_ascii_digit())
                && h.len() <= 2
                && m.len() == 2
            {
                return Some((s[..sp].trim(), time));
            }
        }
    }
    Some((s.trim(), ""))
}

/// 灵活分隔日期：YYYY/M/D、M/D、YYYY.M.D、YYYY-M-D、M-D（后两者当年补零即可）。
fn parse_flex_date(date_part: &str) -> Option<NaiveDate> {
    let sep = if date_part.contains('/') {
        '/'
    } else if date_part.contains('.') {
        '.'
    } else if date_part.contains('-') {
        '-'
    } else {
        return None;
    };
    let parts: Vec<&str> = date_part.split(sep).collect();
    if parts.is_empty() || parts.len() > 3 {
        return None;
    }
    let mut nums = Vec::with_capacity(parts.len());
    for p in &parts {
        if p.is_empty() {
            return None;
        }
        nums.push(p.parse::<i32>().ok()?);
    }
    let today = Local::now().date_naive();
    match nums.len() {
        3 => {
            let (mut y, m, d) = (nums[0], nums[1], nums[2]);
            if y < 100 {
                y += 2000;
            }
            NaiveDate::from_ymd_opt(y, m as u32, d as u32)
        }
        2 => NaiveDate::from_ymd_opt(today.year(), nums[0] as u32, nums[1] as u32),
        _ => None,
    }
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
                    if ms > u {
                        break;
                    }
                }
                out.push(ms);
                occurrences_found += 1;
            }
            current_day += chrono::Duration::days(1);
            if current_day.weekday() == chrono::Weekday::Mon && interval > 1 {
                current_day += chrono::Duration::weeks(interval - 1);
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

#[cfg(test)]
mod tests {
    use super::*;

    fn local_ms(d: NaiveDate, t: NaiveTime) -> i64 {
        local_to_utc_ms(d.and_time(t)).unwrap()
    }
    fn midnight() -> NaiveTime {
        NaiveTime::from_hms_opt(0, 0, 0).unwrap()
    }

    #[test]
    fn parse_chinese_day_words() {
        let today = Local::now().date_naive();
        assert_eq!(parse_time("今天").unwrap(), local_ms(today, midnight()));
        assert_eq!(
            parse_time("明天").unwrap(),
            local_ms(today + Duration::days(1), midnight())
        );
        assert_eq!(
            parse_time("明天 09:30").unwrap(),
            local_ms(
                today + Duration::days(1),
                NaiveTime::from_hms_opt(9, 30, 0).unwrap()
            )
        );
        assert_eq!(
            parse_time("后天").unwrap(),
            local_ms(today + Duration::days(2), midnight())
        );
    }

    #[test]
    fn parse_chinese_weekday() {
        let today = Local::now().date_naive();
        let wd = today.weekday();
        let fri = chrono::Weekday::Fri;
        let delta = (fri.num_days_from_monday() + 7 - wd.num_days_from_monday()) % 7;
        let target = today + Duration::days(delta as i64 + 7);
        assert_eq!(
            parse_time("下周五 15:00").unwrap(),
            local_ms(target, NaiveTime::from_hms_opt(15, 0, 0).unwrap())
        );
        let wed = chrono::Weekday::Wed;
        let delta = (wed.num_days_from_monday() + 7 - wd.num_days_from_monday()) % 7;
        let target = today + Duration::days(delta as i64);
        assert_eq!(parse_time("周三").unwrap(), local_ms(target, midnight()));
        assert_eq!(
            parse_time("星期三 10:00").unwrap(),
            local_ms(target, NaiveTime::from_hms_opt(10, 0, 0).unwrap())
        );
        assert!(parse_time("周日 10:00").is_ok());
        assert!(parse_time("星期天 10:00").is_ok());
    }

    #[test]
    fn parse_slash_dot_dates() {
        let d = NaiveDate::from_ymd_opt(2026, 8, 20).unwrap();
        let t = NaiveTime::from_hms_opt(15, 30, 0).unwrap();
        assert_eq!(parse_time("2026/8/20 15:30").unwrap(), local_ms(d, t));
        assert_eq!(parse_time("2026.8.20 15:30").unwrap(), local_ms(d, t));
        assert_eq!(parse_time("2026-08-20 15:30").unwrap(), local_ms(d, t));
        let now = Local::now();
        let m_d = NaiveDate::from_ymd_opt(now.year(), 8, 20).unwrap();
        assert_eq!(parse_time("8/20 15:30").unwrap(), local_ms(m_d, t));
        assert_eq!(parse_time("8-20 15:30").unwrap(), local_ms(m_d, t));
    }

    #[test]
    fn parse_relative_with_clock() {
        let base = (Local::now() + Duration::days(3)).date_naive();
        let t = NaiveTime::from_hms_opt(15, 30, 0).unwrap();
        assert_eq!(parse_time("+3d 15:30").unwrap(), local_ms(base, t));
        assert!(parse_time("+2h").is_ok());
        assert!(parse_time("+1d").is_ok());
    }
}
