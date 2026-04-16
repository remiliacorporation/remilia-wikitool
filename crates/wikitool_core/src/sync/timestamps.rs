pub(super) fn timestamps_match_with_tolerance(
    stored: &str,
    remote: &str,
    tolerance_seconds: i64,
) -> bool {
    if !stored.ends_with('Z') || !remote.ends_with('Z') {
        return true;
    }
    let parsed_stored = chrono_like_parse_timestamp(stored);
    let parsed_remote = chrono_like_parse_timestamp(remote);
    match (parsed_stored, parsed_remote) {
        (Some(stored), Some(remote)) => (stored - remote).abs() <= tolerance_seconds,
        _ => true,
    }
}

fn chrono_like_parse_timestamp(value: &str) -> Option<i64> {
    // Matches MediaWiki UTC format: YYYY-MM-DDTHH:MM:SSZ
    if value.len() != 20 {
        return None;
    }
    let year = value.get(0..4)?.parse::<i32>().ok()?;
    let month = value.get(5..7)?.parse::<u32>().ok()?;
    let day = value.get(8..10)?.parse::<u32>().ok()?;
    let hour = value.get(11..13)?.parse::<u32>().ok()?;
    let minute = value.get(14..16)?.parse::<u32>().ok()?;
    let second = value.get(17..19)?.parse::<u32>().ok()?;

    if value.as_bytes().get(4) != Some(&b'-')
        || value.as_bytes().get(7) != Some(&b'-')
        || value.as_bytes().get(10) != Some(&b'T')
        || value.as_bytes().get(13) != Some(&b':')
        || value.as_bytes().get(16) != Some(&b':')
        || value.as_bytes().get(19) != Some(&b'Z')
    {
        return None;
    }

    let days_before_year = days_before_year(year)?;
    let days_before_month = days_before_month(year, month)?;
    let day_index = i64::from(day.checked_sub(1)?);

    Some(
        (days_before_year + days_before_month + day_index) * 86_400
            + i64::from(hour) * 3_600
            + i64::from(minute) * 60
            + i64::from(second),
    )
}

fn days_before_year(year: i32) -> Option<i64> {
    let y = i64::from(year);
    let y1 = y.checked_sub(1)?;
    let leap_days = y1 / 4 - y1 / 100 + y1 / 400;
    y1.checked_mul(365)?.checked_add(leap_days)
}

fn days_before_month(year: i32, month: u32) -> Option<i64> {
    let month_days: [u32; 12] = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    if !(1..=12).contains(&month) {
        return None;
    }
    let mut days = 0i64;
    for current in 1..month {
        let mut value = i64::from(*month_days.get(usize::try_from(current - 1).ok()?)?);
        if current == 2 && is_leap_year(year) {
            value += 1;
        }
        days += value;
    }
    Some(days)
}

fn is_leap_year(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}
