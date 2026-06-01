use std::net::Ipv4Addr;

pub fn parse_ipv4_address(input: &str) -> Result<Ipv4Addr, String> {
    input.parse::<Ipv4Addr>().map_err(|_| {
        format!("Invalid IPv4 address '{input}'. Please provide a value like 192.168.1.1.")
    })
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
}
