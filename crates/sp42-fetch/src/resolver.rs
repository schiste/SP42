//! SSRF guard expressed as a DNS resolver (ADR-0015).
//!
//! The guard validates the *resolved* address, not the URL string: a hostname
//! that resolves to a private/loopback/link-local/metadata address is refused,
//! which closes the DNS-rebinding gap (#60). `reqwest` runs the resolver for
//! every connection, including each redirect hop.

use std::net::IpAddr;
use std::sync::Arc;

use ip_network::{Ipv4Network, Ipv6Network};
use reqwest::dns::{Addrs, Name, Resolve, Resolving};

/// A `reqwest` DNS resolver that wraps an inner resolver and drops any resolved
/// address that is not globally routable, refusing the lookup if nothing
/// survives. `reqwest` connects only to the addresses returned here, on the
/// initial request and every redirect hop — so an attacker-influenced hostname
/// that resolves to an internal/metadata address cannot be reached (#60).
#[derive(Clone)]
pub(crate) struct GuardedResolver {
    inner: Arc<dyn Resolve>,
}

impl GuardedResolver {
    pub(crate) fn new(inner: Arc<dyn Resolve>) -> Self {
        Self { inner }
    }
}

impl Resolve for GuardedResolver {
    fn resolve(&self, name: Name) -> Resolving {
        let inner = Arc::clone(&self.inner);
        Box::pin(async move {
            let resolved = inner.resolve(name).await?;
            let public: Vec<_> = resolved.filter(|addr| is_public_ip(addr.ip())).collect();
            if public.is_empty() {
                return Err("SSRF: host resolved only to non-public addresses".into());
            }
            Ok(Box::new(public.into_iter()) as Addrs)
        })
    }
}

/// Whether an address is safe to connect to from attacker-influenced input —
/// i.e. globally routable, not in any reserved/internal range (loopback,
/// RFC1918 private, link-local incl. the `169.254.0.0/16` cloud-metadata range,
/// CGNAT, IPv6 ULA/link-local, unspecified, …).
///
/// This is a pure local range classification (`ip_network::is_global`), not
/// authentication of the address — see ADR-0015.
#[must_use]
pub(crate) fn is_public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => Ipv4Network::new(v4, 32).is_ok_and(|net| net.is_global()),
        // An IPv4-mapped IPv6 address (`::ffff:a.b.c.d`) reaches the embedded IPv4
        // host, but `Ipv6Network::is_global()` treats the whole `::ffff/96` block
        // as global — so unwrap and classify the embedded IPv4 instead, or a
        // mapped loopback/metadata address would slip through.
        IpAddr::V6(v6) => match v6.to_ipv4_mapped() {
            Some(v4) => is_public_ip(IpAddr::V4(v4)),
            None => Ipv6Network::new(v6, 128).is_ok_and(|net| net.is_global()),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::{GuardedResolver, is_public_ip};
    use reqwest::dns::{Addrs, Name, Resolve, Resolving};
    use std::net::{IpAddr, SocketAddr};
    use std::str::FromStr;
    use std::sync::Arc;

    fn ip(s: &str) -> IpAddr {
        s.parse().expect("valid ip")
    }

    /// An inner resolver that returns a fixed set of addresses regardless of name.
    struct StubResolver(Vec<SocketAddr>);

    impl Resolve for StubResolver {
        fn resolve(&self, _name: Name) -> Resolving {
            let addrs = self.0.clone();
            Box::pin(async move { Ok(Box::new(addrs.into_iter()) as Addrs) })
        }
    }

    fn addr(s: &str) -> SocketAddr {
        s.parse().expect("valid socket addr")
    }

    #[tokio::test]
    async fn guard_keeps_only_public_resolved_addresses() {
        let inner = StubResolver(vec![addr("127.0.0.1:80"), addr("1.2.3.4:80")]);
        let guard = GuardedResolver::new(Arc::new(inner));

        let resolved: Vec<_> = guard
            .resolve(Name::from_str("mixed.test").expect("name"))
            .await
            .expect("at least one public address survives")
            .collect();

        assert_eq!(resolved, vec![addr("1.2.3.4:80")]);
    }

    #[tokio::test]
    async fn guard_refuses_when_every_resolved_address_is_private() {
        let inner = StubResolver(vec![addr("127.0.0.1:80"), addr("169.254.169.254:80")]);
        let guard = GuardedResolver::new(Arc::new(inner));

        let result = guard
            .resolve(Name::from_str("evil.test").expect("name"))
            .await;

        assert!(
            result.is_err(),
            "a host resolving only to private IPs must be refused"
        );
    }

    #[test]
    fn blocks_reserved_and_internal_ranges() {
        for blocked in [
            "127.0.0.1",              // loopback
            "10.0.0.1",               // RFC1918
            "192.168.1.1",            // RFC1918
            "172.16.0.1",             // RFC1918
            "169.254.169.254",        // link-local / cloud metadata
            "100.64.0.1",             // CGNAT
            "0.0.0.0",                // unspecified
            "::1",                    // v6 loopback
            "fc00::1",                // v6 ULA
            "fe80::1",                // v6 link-local
            "::ffff:127.0.0.1",       // IPv4-mapped loopback
            "::ffff:169.254.169.254", // IPv4-mapped cloud metadata
            "::ffff:10.0.0.1",        // IPv4-mapped RFC1918
        ] {
            assert!(!is_public_ip(ip(blocked)), "{blocked} must be blocked");
        }
    }

    #[test]
    fn allows_globally_routable_addresses() {
        for public in [
            "1.1.1.1",
            "8.8.8.8",
            "208.80.154.224",
            "2606:4700:4700::1111",
        ] {
            assert!(is_public_ip(ip(public)), "{public} must be allowed");
        }
    }
}
