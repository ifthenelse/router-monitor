use std::net::Ipv4Addr;

pub fn parse_ipv4_address(input: &str) -> Result<Ipv4Addr, String> {
    input.parse::<Ipv4Addr>().map_err(|_| {
        format!("Invalid IPv4 address '{input}'. Please provide a value like 192.168.1.1.")
    })
}

pub fn parse_latitude(input: &str) -> Result<f64, String> {
    parse_coordinate(input, -90.0, 90.0, "latitude")
}

pub fn parse_longitude(input: &str) -> Result<f64, String> {
    parse_coordinate(input, -180.0, 180.0, "longitude")
}

fn parse_coordinate(input: &str, min: f64, max: f64, name: &str) -> Result<f64, String> {
    let value = input
        .parse::<f64>()
        .map_err(|_| format!("Invalid {name} '{input}'. Please provide a decimal number."))?;

    if !value.is_finite() || value < min || value > max {
        return Err(format!(
            "Invalid {name} '{input}'. Please provide a value between {min} and {max}."
        ));
    }

    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_valid_ipv4_addresses() {
        for address in ["192.168.1.1", "192.168.0.1", "10.0.0.1", "172.16.1.254"] {
            assert!(parse_ipv4_address(address).is_ok(), "{address}");
        }
    }

    #[test]
    fn rejects_invalid_ipv4_addresses() {
        for address in [
            "999.999.999.999",
            "abc.def.ghi.jkl",
            "192.168.1",
            "router.local",
        ] {
            assert!(parse_ipv4_address(address).is_err(), "{address}");
        }
    }

    #[test]
    fn validates_latitude_and_longitude_ranges() {
        assert_eq!(parse_latitude("45.484").unwrap(), 45.484);
        assert_eq!(parse_longitude("9.204").unwrap(), 9.204);
        assert!(parse_latitude("91").is_err());
        assert!(parse_longitude("181").is_err());
        assert!(parse_latitude("nan").is_err());
    }
}
