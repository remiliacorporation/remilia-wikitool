use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, ToSocketAddrs};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use reqwest::Url;
use reqwest::blocking::Client;
use reqwest::redirect::Policy;

pub(crate) struct ValidatedOutboundResearchUrl {
    url: Url,
    resolver_host: String,
    socket_addrs: Vec<SocketAddr>,
}

impl ValidatedOutboundResearchUrl {
    pub(crate) fn url(&self) -> &Url {
        &self.url
    }

    /// Build a no-redirect client whose DNS result is the exact public address
    /// set checked by policy. This prevents a second resolver lookup from
    /// turning validation into a DNS-rebinding window.
    pub(crate) fn pinned_client(&self, timeout: Duration) -> Result<Client> {
        Client::builder()
            .timeout(timeout)
            .redirect(Policy::none())
            .resolve_to_addrs(&self.resolver_host, &self.socket_addrs)
            .build()
            .context("failed to build pinned source HTTP client")
    }
}

pub(crate) fn validate_outbound_source_url(raw_url: &str) -> Result<ValidatedOutboundResearchUrl> {
    let url = Url::parse(raw_url).with_context(|| format!("invalid source URL: {raw_url}"))?;
    if !matches!(url.scheme(), "http" | "https") {
        bail!("source URL scheme must be http or https: {raw_url}");
    }
    if !url.username().is_empty() || url.password().is_some() {
        bail!("source URL must not contain embedded credentials: {raw_url}");
    }
    let host = url
        .host_str()
        .ok_or_else(|| anyhow::anyhow!("source URL has no host: {raw_url}"))?
        .to_string();
    let normalized_host = host.trim_end_matches('.').to_ascii_lowercase();
    if normalized_host == "localhost"
        || normalized_host.ends_with(".localhost")
        || normalized_host.ends_with(".local")
    {
        bail!("source URL resolves to a local hostname: {host}");
    }

    let port = url
        .port_or_known_default()
        .ok_or_else(|| anyhow::anyhow!("source URL has no usable port: {raw_url}"))?;
    let socket_addrs = if let Ok(ip) = normalized_host.parse::<IpAddr>() {
        ensure_public_ip(ip, &host)?;
        vec![SocketAddr::new(ip, port)]
    } else {
        (normalized_host.as_str(), port)
            .to_socket_addrs()
            .with_context(|| format!("failed to resolve source URL host: {host}"))?
            .collect::<Vec<_>>()
    };
    if socket_addrs.is_empty() {
        bail!("source URL host resolved to no addresses: {host}");
    }
    for address in &socket_addrs {
        ensure_public_ip(address.ip(), &host)?;
    }
    Ok(ValidatedOutboundResearchUrl {
        url,
        resolver_host: host,
        socket_addrs,
    })
}

fn ensure_public_ip(ip: IpAddr, host: &str) -> Result<()> {
    let blocked = match ip {
        IpAddr::V4(ip) => ipv4_is_non_public(ip),
        IpAddr::V6(ip) => ipv6_is_non_public(ip),
    };
    if blocked {
        bail!("source URL host resolves to a non-public address: {host} -> {ip}");
    }
    Ok(())
}

fn ipv4_is_non_public(ip: Ipv4Addr) -> bool {
    let [a, b, c, _] = ip.octets();
    ip.is_private()
        || ip.is_loopback()
        || ip.is_link_local()
        || ip.is_broadcast()
        || ip.is_documentation()
        || ip.is_unspecified()
        || ip.is_multicast()
        || a == 0
        || a >= 240
        || (a == 100 && (64..=127).contains(&b))
        || (a == 192 && b == 0 && c == 0)
        || (a == 198 && matches!(b, 18 | 19))
}

fn ipv6_is_non_public(ip: Ipv6Addr) -> bool {
    let octets = ip.octets();
    if let Some(mapped) = ip.to_ipv4_mapped() {
        return ipv4_is_non_public(mapped);
    }
    ip.is_loopback()
        || ip.is_unspecified()
        || ip.is_multicast()
        || (octets[0] & 0xfe) == 0xfc
        || (octets[0] == 0xfe && (octets[1] & 0xc0) == 0x80)
        || (octets[0] == 0x20 && octets[1] == 0x01 && octets[2] == 0x0d && octets[3] == 0xb8)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_local_and_special_targets() {
        for url in [
            "http://127.0.0.1/admin",
            "http://10.0.0.1/",
            "http://169.254.169.254/latest/meta-data",
            "http://[::1]/",
            "http://localhost/",
            "file:///etc/passwd",
        ] {
            assert!(validate_outbound_source_url(url).is_err(), "{url}");
        }
    }

    #[test]
    fn rejects_embedded_credentials_before_resolution() {
        assert!(validate_outbound_source_url("https://user:secret@example.invalid/").is_err());
    }
}
