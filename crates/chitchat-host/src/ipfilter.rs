//! Owner IP rules: block lists, optional allowlist mode, CIDR ranges for IPv4 and IPv6.

use std::net::IpAddr;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cidr {
    addr: IpAddr,
    prefix: u8,
}

impl Cidr {
    /// Parses `1.2.3.4`, `1.2.3.0/24`, `2001:db8::/32`. The host bits are cleared.
    pub fn parse(s: &str) -> Option<Cidr> {
        let s = s.trim();
        let (ip, prefix) = match s.split_once('/') {
            Some((ip, p)) => (ip.parse::<IpAddr>().ok()?, Some(p.parse::<u8>().ok()?)),
            None => (s.parse::<IpAddr>().ok()?, None),
        };
        let ip = normalize(ip);
        let max = if ip.is_ipv4() { 32 } else { 128 };
        let prefix = prefix.unwrap_or(max);
        if prefix > max {
            return None;
        }
        Some(Cidr { addr: mask(ip, prefix), prefix })
    }

    pub fn contains(&self, ip: IpAddr) -> bool {
        let ip = normalize(ip);
        ip.is_ipv4() == self.addr.is_ipv4() && mask(ip, self.prefix) == self.addr
    }
}

impl std::fmt::Display for Cidr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let max = if self.addr.is_ipv4() { 32 } else { 128 };
        if self.prefix == max { write!(f, "{}", self.addr) } else { write!(f, "{}/{}", self.addr, self.prefix) }
    }
}

/// IPv4-mapped IPv6 (`::ffff:1.2.3.4`) is treated as IPv4.
pub fn normalize(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V6(v6) => v6.to_ipv4_mapped().map(IpAddr::V4).unwrap_or(ip),
        v4 => v4,
    }
}

fn mask(ip: IpAddr, prefix: u8) -> IpAddr {
    match ip {
        IpAddr::V4(v4) => {
            let bits = u32::from(v4);
            let m = if prefix == 0 { 0 } else { u32::MAX << (32 - prefix) };
            IpAddr::V4((bits & m).into())
        }
        IpAddr::V6(v6) => {
            let bits = u128::from(v6);
            let m = if prefix == 0 { 0 } else { u128::MAX << (128 - prefix) };
            IpAddr::V6((bits & m).into())
        }
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Action {
    Allow,
    Block,
}

#[derive(Clone)]
pub struct Rule {
    pub cidr: Cidr,
    pub action: Action,
}

#[derive(Clone, Default)]
pub struct IpFilter {
    /// When true, only addresses matching an allow rule get in.
    pub allowlist: bool,
    pub rules: Vec<Rule>,
}

impl IpFilter {
    /// The host machine itself (loopback) is always allowed, so the owner can't lock themselves out.
    pub fn allows(&self, ip: IpAddr) -> bool {
        let ip = normalize(ip);
        if ip.is_loopback() {
            return true;
        }
        if self.rules.iter().any(|r| r.action == Action::Block && r.cidr.contains(ip)) {
            return false;
        }
        !self.allowlist || self.rules.iter().any(|r| r.action == Action::Allow && r.cidr.contains(ip))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(s: &str) -> IpAddr {
        s.parse().unwrap()
    }

    #[test]
    fn cidr_matching() {
        let c = Cidr::parse("192.168.1.77/24").unwrap();
        assert_eq!(c.to_string(), "192.168.1.0/24");
        assert!(c.contains(ip("192.168.1.5")));
        assert!(c.contains(ip("::ffff:192.168.1.5")));
        assert!(!c.contains(ip("192.168.2.5")));
        assert!(Cidr::parse("10.0.0.1").unwrap().contains(ip("10.0.0.1")));
        assert!(Cidr::parse("2001:db8::/32").unwrap().contains(ip("2001:db8:1::5")));
        assert!(Cidr::parse("1.2.3.4/33").is_none());
        assert!(Cidr::parse("nonsense").is_none());
    }

    #[test]
    fn filter_modes() {
        let mut f = IpFilter::default();
        assert!(f.allows(ip("8.8.8.8")));
        f.rules.push(Rule { cidr: Cidr::parse("8.8.8.0/24").unwrap(), action: Action::Block });
        assert!(!f.allows(ip("8.8.8.8")));
        f.allowlist = true;
        f.rules.push(Rule { cidr: Cidr::parse("1.1.1.1").unwrap(), action: Action::Allow });
        assert!(f.allows(ip("1.1.1.1")));
        assert!(!f.allows(ip("1.1.1.2")));
        assert!(f.allows(ip("127.0.0.1")));
        assert!(f.allows(ip("::1")));
    }
}
