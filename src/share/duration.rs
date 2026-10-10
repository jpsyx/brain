use anyhow::{Result, bail};
use std::time::Duration;

pub const MAX_SECONDS: u64 = 7 * 24 * 60 * 60;

/// Parse a whole-number duration and enforce Backblaze's direct-link limit.
pub fn expiration(input: &str) -> Result<Duration> {
    let input = input.trim();
    if input.is_empty() {
        return Ok(Duration::from_secs(MAX_SECONDS));
    }
    let split = input
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(input.len());
    let quantity: u64 = input[..split]
        .parse()
        .map_err(|_| anyhow::anyhow!("Enter a positive duration, such as 7 days or 1h"))?;
    let unit = input[split..].trim().to_ascii_lowercase();
    let seconds = match unit.as_str() {
        "s" | "sec" | "secs" | "second" | "seconds" => 1,
        "m" | "min" | "mins" | "minute" | "minutes" => 60,
        "h" | "hr" | "hrs" | "hour" | "hours" => 3600,
        "d" | "day" | "days" => 86400,
        "w" | "wk" | "wks" | "week" | "weeks" => 604_800,
        "mo" | "mos" | "month" | "months" => 30 * 86400,
        "y" | "yr" | "yrs" | "year" | "years" => 365 * 86400,
        _ => bail!(
            "Unknown expiration unit; use seconds, minutes, hours, days, weeks, months, or years"
        ),
    };
    let total = quantity
        .checked_mul(seconds)
        .ok_or_else(|| anyhow::anyhow!("Backblaze download links can last at most 7 days"))?;
    if total == 0 {
        bail!("Expiration must be at least 1 second");
    }
    if total > MAX_SECONDS {
        bail!("Backblaze download links can last at most 7 days; choose 7 days or less");
    }
    Ok(Duration::from_secs(total))
}
