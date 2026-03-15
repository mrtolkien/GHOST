use std::net::IpAddr;

use url::Url;

use super::error::BrowserError;

/// Validate that a URL is safe to navigate to.
///
/// Blocks:
/// - Non-http(s) schemes (file:, javascript:, data:)
/// - Private/reserved IP ranges (127/8, 10/8, 172.16/12, 192.168/16, 169.254/16)
/// - localhost
pub fn validate_url(raw: &str) -> Result<Url, BrowserError> {
    let url = Url::parse(raw).map_err(|e| BrowserError::UrlBlocked {
        reason: format!("invalid URL: {e}"),
    })?;

    match url.scheme() {
        "http" | "https" => {}
        scheme => {
            return Err(BrowserError::UrlBlocked {
                reason: format!("scheme '{scheme}' not allowed, use http or https"),
            });
        }
    }

    if let Some(host_str) = url.host_str()
        && host_str == "localhost"
    {
        return Err(BrowserError::UrlBlocked {
            reason: "localhost not allowed".into(),
        });
    }
    // Use url::Host for reliable IP parsing (handles IPv6 brackets).
    let ip = match url.host() {
        Some(url::Host::Ipv4(v4)) => Some(IpAddr::V4(v4)),
        Some(url::Host::Ipv6(v6)) => Some(IpAddr::V6(v6)),
        _ => None,
    };
    if let Some(ip) = ip
        && is_private_ip(ip)
    {
        return Err(BrowserError::UrlBlocked {
            reason: format!("private IP {ip} not allowed"),
        });
    }

    Ok(url)
}

fn is_private_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => v4.is_loopback() || v4.is_private() || v4.is_link_local(),
        IpAddr::V6(v6) => {
            v6.is_loopback()
                || is_ipv6_unique_local(&v6)
                || is_ipv6_link_local(&v6)
                || is_ipv4_mapped_private(&v6)
        }
    }
}

fn is_ipv6_unique_local(v6: &std::net::Ipv6Addr) -> bool {
    (v6.segments()[0] & 0xfe00) == 0xfc00 // fc00::/7
}

fn is_ipv6_link_local(v6: &std::net::Ipv6Addr) -> bool {
    (v6.segments()[0] & 0xffc0) == 0xfe80 // fe80::/10
}

fn is_ipv4_mapped_private(v6: &std::net::Ipv6Addr) -> bool {
    // ::ffff:127.0.0.1, ::ffff:10.x.x.x, etc.
    match v6.to_ipv4_mapped() {
        Some(v4) => v4.is_loopback() || v4.is_private() || v4.is_link_local(),
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allows_http_urls() {
        assert!(validate_url("https://example.com").is_ok());
        assert!(validate_url("http://example.com").is_ok());
    }

    #[test]
    fn blocks_non_http_schemes() {
        assert!(validate_url("file:///etc/passwd").is_err());
        assert!(validate_url("javascript:alert(1)").is_err());
        assert!(validate_url("data:text/html,<h1>hi</h1>").is_err());
    }

    #[test]
    fn blocks_private_ips() {
        assert!(validate_url("http://127.0.0.1").is_err());
        assert!(validate_url("http://10.0.0.1").is_err());
        assert!(validate_url("http://192.168.1.1").is_err());
        assert!(validate_url("http://172.16.0.1").is_err());
    }

    #[test]
    fn blocks_localhost() {
        assert!(validate_url("http://localhost").is_err());
        assert!(validate_url("http://localhost:9222").is_err());
    }

    #[test]
    fn blocks_ipv6_private() {
        assert!(validate_url("http://[::1]").is_err());
        assert!(validate_url("http://[fc00::1]").is_err());
        assert!(validate_url("http://[fe80::1]").is_err());
        assert!(validate_url("http://[::ffff:127.0.0.1]").is_err());
        assert!(validate_url("http://[::ffff:10.0.0.1]").is_err());
    }
}
