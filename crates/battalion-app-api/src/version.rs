use std::cmp::Ordering;

/// `major.minor.patch`, numeric. A `-pre` suffix sorts before the release.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Version {
    pub parts: [u64; 3],
    pub pre: Option<String>,
}

impl Version {
    pub fn parse(s: &str) -> Option<Version> {
        let (core, pre) = match s.split_once('-') {
            Some((c, p)) if !p.is_empty() => (c, Some(p.to_string())),
            Some(_) => return None,
            None => (s, None),
        };
        let mut it = core.split('.');
        let mut parts = [0u64; 3];
        for p in parts.iter_mut() {
            let seg = it.next()?;
            if seg.is_empty() || !seg.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            *p = seg.parse().ok()?;
        }
        if it.next().is_some() {
            return None;
        }
        Some(Version { parts, pre })
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}
impl Ord for Version {
    fn cmp(&self, o: &Self) -> Ordering {
        self.parts.cmp(&o.parts).then_with(|| match (&self.pre, &o.pre) {
            (None, None) => Ordering::Equal,
            (Some(_), None) => Ordering::Less,
            (None, Some(_)) => Ordering::Greater,
            (Some(a), Some(b)) => a.cmp(b),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_and_orders() {
        assert!(Version::parse("1.2").is_none());
        assert!(Version::parse("1.2.x").is_none());
        assert!(Version::parse("1.2.3.4").is_none());
        assert!(Version::parse("1.2.3-").is_none());
        assert!(Version::parse("0.9.0").unwrap() < Version::parse("0.10.0").unwrap());
        assert!(Version::parse("1.0.0-rc1").unwrap() < Version::parse("1.0.0").unwrap());
    }
}
