use crate::errors;
use anyhow::{bail, Context, Result};
use std::collections::HashSet;
use std::time::Duration;

#[derive(Debug, Clone, PartialEq)]
pub struct DurationSpec {
    total: Duration,
    display: String,
}

#[derive(Debug, Clone, PartialEq)]
struct DurationPart {
    value: f64,
    unit: Option<char>,
}

impl DurationSpec {
    pub fn total(&self) -> Duration {
        self.total
    }

    pub fn display(&self) -> &str {
        &self.display
    }

    pub fn sample_seconds(&self) -> u64 {
        let whole = self.total.as_secs();

        if self.total.subsec_nanos() == 0 {
            whole
        } else {
            whole + 1
        }
    }

    #[cfg(test)]
    pub fn total_seconds_exact(&self) -> f64 {
        self.total.as_secs_f64()
    }
}

pub fn parse_duration(parts: &[String]) -> Result<DurationSpec> {
    if parts.is_empty() {
        bail!("A monitoring duration is required.");
    }

    let parsed_parts = parts
        .iter()
        .map(|part| parse_duration_part(part))
        .collect::<Result<Vec<_>>>()?;

    validate_unit_mixing(&parsed_parts)?;
    validate_duplicate_units(&parsed_parts)?;

    let total_seconds =
        parsed_parts
            .iter()
            .map(part_seconds)
            .try_fold(0.0, |total, seconds| -> Result<f64> {
                let seconds = seconds?;
                let total = total + seconds;

                if total.is_finite() {
                    Ok(total)
                } else {
                    bail!("Duration is too large.");
                }
            })?;

    if total_seconds <= 0.0 {
        bail!("Duration must be greater than zero seconds.");
    }

    Ok(DurationSpec {
        total: Duration::from_secs_f64(total_seconds),
        display: parts.join(" "),
    })
}

fn parse_duration_part(input: &str) -> Result<DurationPart> {
    let trimmed = input.trim();

    if trimmed.is_empty() {
        bail!("Duration values cannot be empty.");
    }

    let numeric_end = trimmed
        .char_indices()
        .take_while(|(_, character)| character.is_ascii_digit() || *character == '.')
        .map(|(index, character)| index + character.len_utf8())
        .last()
        .unwrap_or(0);

    let numeric = &trimmed[..numeric_end];
    let unit_text = &trimmed[numeric_end..];

    if numeric.is_empty() || numeric == "." {
        if unit_text.is_empty() {
            bail!("Invalid duration value '{trimmed}'.");
        }

        bail!("{}", errors::unsupported_duration_unit(unit_text));
    }

    let value = numeric
        .parse::<f64>()
        .with_context(|| format!("Invalid duration value '{trimmed}'."))?;

    if !value.is_finite() || value <= 0.0 {
        bail!("Duration value '{trimmed}' must be greater than zero.");
    }

    let unit = match unit_text {
        "" => None,
        "s" | "m" | "h" | "d" => unit_text.chars().next(),
        unsupported => bail!("{}", errors::unsupported_duration_unit(unsupported)),
    };

    Ok(DurationPart { value, unit })
}

fn validate_unit_mixing(parts: &[DurationPart]) -> Result<()> {
    let has_unitless = parts.iter().any(|part| part.unit.is_none());
    let has_units = parts.iter().any(|part| part.unit.is_some());

    if has_unitless && has_units {
        bail!("{}", errors::MIXED_DURATION_UNITS);
    }

    if has_unitless && parts.len() > 1 {
        bail!("Unitless duration must be supplied as a single value.");
    }

    Ok(())
}

fn validate_duplicate_units(parts: &[DurationPart]) -> Result<()> {
    let mut units = HashSet::new();

    for unit in parts.iter().filter_map(|part| part.unit) {
        if !units.insert(unit) {
            bail!("{}", errors::duplicate_duration_unit(unit));
        }
    }

    Ok(())
}

fn part_seconds(part: &DurationPart) -> Result<f64> {
    let multiplier = match part.unit {
        None | Some('s') => 1.0,
        Some('m') => 60.0,
        Some('h') => 60.0 * 60.0,
        Some('d') => 24.0 * 60.0 * 60.0,
        Some(unit) => bail!("{}", errors::unsupported_duration_unit(&unit.to_string())),
    };

    Ok(part.value * multiplier)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(input: &[&str]) -> Result<DurationSpec> {
        parse_duration(
            &input
                .iter()
                .map(|value| value.to_string())
                .collect::<Vec<_>>(),
        )
    }

    #[test]
    fn parses_unitless_seconds() {
        assert_eq!(parse(&["15"]).unwrap().total_seconds_exact(), 15.0);
    }

    #[test]
    fn parses_duration_examples() {
        let examples = [
            (vec!["15s"], 15.0),
            (vec!["7m"], 420.0),
            (vec!["3.5m"], 210.0),
            (vec!["4m", "20s"], 260.0),
            (vec!["7m", "150s"], 570.0),
            (vec!["7.5m", "20s"], 470.0),
            (vec!["9.4m", "200s"], 764.0),
            (vec!["1h", "30m"], 5_400.0),
            (vec!["1d", "4h", "20m"], 102_000.0),
        ];

        for (input, expected_seconds) in examples {
            assert_eq!(
                parse(&input).unwrap().total_seconds_exact(),
                expected_seconds,
                "{input:?}"
            );
        }
    }

    #[test]
    fn parses_decimal_values_for_all_units() {
        let examples = [
            (vec!["1.5s"], 1.5),
            (vec!["1.5m"], 90.0),
            (vec!["2.25h"], 8_100.0),
            (vec!["0.5d"], 43_200.0),
        ];

        for (input, expected_seconds) in examples {
            assert_eq!(
                parse(&input).unwrap().total_seconds_exact(),
                expected_seconds,
                "{input:?}"
            );
        }
    }

    #[test]
    fn rounds_fractional_seconds_up_for_sampling() {
        assert_eq!(parse(&["1.5s"]).unwrap().sample_seconds(), 2);
    }

    #[test]
    fn rejects_duplicate_units() {
        let error = parse(&["1m", "11m"]).unwrap_err().to_string();
        assert!(error.contains("Duration unit 'm' was specified more than once."));
    }

    #[test]
    fn rejects_mixed_unitless_and_unit_based_values() {
        let error = parse(&["15", "20s"]).unwrap_err().to_string();
        assert!(error.contains(errors::MIXED_DURATION_UNITS));

        let error = parse(&["15s", "20"]).unwrap_err().to_string();
        assert!(error.contains(errors::MIXED_DURATION_UNITS));
    }

    #[test]
    fn rejects_invalid_units() {
        for input in [["7.5c"], ["3w"], ["10years"], ["abc"]] {
            let error = parse(&input).unwrap_err().to_string();
            assert!(
                error.contains("Supported units are: s, m, h, d."),
                "{error}"
            );
        }
    }

    #[test]
    fn calculates_total_duration() {
        assert_eq!(
            parse(&["1h", "30m", "15s"]).unwrap().total_seconds_exact(),
            5_415.0
        );
    }
}
