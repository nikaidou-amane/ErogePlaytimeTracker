/// Normalizes a user-entered proxy setting.
///
/// An empty setting means "use the system proxy", which is what the HTTP client
/// does when no proxy is configured. A value without a scheme is read as an HTTP
/// proxy, because that is what a user copying `host:port` from a VPN client
/// expects. Anything that is not a usable proxy URL is rejected instead of being
/// passed to the client, so a typo cannot silently disable network access.
pub fn normalize(value: &str) -> Result<Option<String>, String> {
    let value = value.trim();
    if value.is_empty() {
        return Ok(None);
    }
    let candidate = if value.contains("://") {
        value.to_string()
    } else {
        format!("http://{value}")
    };
    let url = url::Url::parse(&candidate)
        .map_err(|_| "プロキシURLの形式が正しくありません。".to_string())?;
    if !matches!(url.scheme(), "http" | "https" | "socks5" | "socks5h") {
        return Err("プロキシはhttp・https・socks5のいずれかを指定してください。".into());
    }
    if url.host_str().is_none_or(str::is_empty) {
        return Err("プロキシのホスト名を指定してください。".into());
    }
    Ok(Some(url.as_str().trim_end_matches('/').to_string()))
}

#[cfg(test)]
mod tests {
    use super::normalize;

    #[test]
    fn empty_setting_uses_the_system_proxy() {
        assert_eq!(normalize(""), Ok(None));
        assert_eq!(normalize("   "), Ok(None));
    }

    #[test]
    fn host_and_port_defaults_to_http() {
        assert_eq!(
            normalize(" 127.0.0.1:7890 "),
            Ok(Some("http://127.0.0.1:7890".into()))
        );
    }

    #[test]
    fn keeps_an_explicit_supported_scheme() {
        for value in [
            "http://127.0.0.1:7890",
            "https://proxy.example:8443",
            "socks5://127.0.0.1:1080",
        ] {
            assert_eq!(normalize(value), Ok(Some(value.into())));
        }
        assert_eq!(
            normalize("socks5://127.0.0.1:1080/"),
            Ok(Some("socks5://127.0.0.1:1080".into())),
            "a trailing slash does not change the proxy target"
        );
    }

    #[test]
    fn rejects_unusable_values() {
        assert!(normalize("ftp://127.0.0.1:21").is_err());
        assert!(normalize("http://").is_err());
        assert!(normalize("http://:7890").is_err());
    }
}
