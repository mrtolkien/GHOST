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

    if let Some(host) = url.host_str() {
        if host == "localhost" {
            return Err(BrowserError::UrlBlocked {
                reason: "localhost not allowed".into(),
            });
        }
        if let Ok(ip) = host.parse::<IpAddr>()
            && is_private_ip(ip)
        {
            return Err(BrowserError::UrlBlocked {
                reason: format!("private IP {ip} not allowed"),
            });
        }
    }

    Ok(url)
}

fn is_private_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => v4.is_loopback() || v4.is_private() || v4.is_link_local(),
        IpAddr::V6(v6) => v6.is_loopback(),
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
}
