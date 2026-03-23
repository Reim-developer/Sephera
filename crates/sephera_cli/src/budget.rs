use anyhow::{Result, bail};

/// # Errors
///
/// Returns an error when the token budget is empty, malformed, or zero.
pub fn parse_token_budget(raw_value: &str) -> Result<u64> {
    let trimmed_value = raw_value.trim();
    if trimmed_value.is_empty() {
        bail!("token budget cannot be empty");
    }

    let (digits, multiplier) = match trimmed_value
        .chars()
        .last()
        .unwrap_or_default()
        .to_ascii_lowercase()
    {
        'k' => (&trimmed_value[..trimmed_value.len() - 1], 1_000_u64),
        'm' => (&trimmed_value[..trimmed_value.len() - 1], 1_000_000_u64),
        _ => (trimmed_value, 1_u64),
    };

    if digits.is_empty() {
        bail!("token budget must include digits");
    }

    let parsed_value = digits
        .parse::<u64>()
        .map_err(|_| anyhow::anyhow!("invalid token budget `{raw_value}`"))?;
    if parsed_value == 0 {
        bail!("token budget must be greater than zero");
    }

    parsed_value.checked_mul(multiplier).ok_or_else(|| {
        anyhow::anyhow!("token budget `{raw_value}` is too large")
    })
}
