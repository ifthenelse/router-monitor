pub const MIXED_DURATION_UNITS: &str = "Cannot mix unitless durations and unit-based durations.";

pub fn duplicate_duration_unit(unit: char) -> String {
    format!("Duration unit '{unit}' was specified more than once.")
}

pub fn unsupported_duration_unit(unit: &str) -> String {
    format!("Unsupported duration unit '{unit}'. Supported units are: s, m, h, d.")
}
