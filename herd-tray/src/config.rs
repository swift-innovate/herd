//! Gateway URL resolution: `--gateway` flag > `$HERD_TRAY_GATEWAY` > default.

pub const DEFAULT_GATEWAY: &str = "http://127.0.0.1:40114";
pub const ENV_GATEWAY: &str = "HERD_TRAY_GATEWAY";

/// Combine the (already-extracted) flag and env values with the default.
pub fn resolve_gateway(flag: Option<String>, env: Option<String>) -> String {
    flag.or(env).unwrap_or_else(|| DEFAULT_GATEWAY.to_string())
}

/// Extract `--gateway <url>` or `--gateway=<url>` from an argv iterator. The tray
/// has exactly one flag, so a full parser would be overkill.
pub fn parse_gateway_flag<I: IntoIterator<Item = String>>(args: I) -> Option<String> {
    let mut it = args.into_iter();
    while let Some(a) = it.next() {
        if a == "--gateway" {
            return it.next();
        }
        if let Some(v) = a.strip_prefix("--gateway=") {
            return Some(v.to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn flag_wins_over_env_and_default() {
        assert_eq!(
            resolve_gateway(Some("http://flag:1".into()), Some("http://env:2".into())),
            "http://flag:1"
        );
    }

    #[test]
    fn env_wins_over_default() {
        assert_eq!(
            resolve_gateway(None, Some("http://env:2".into())),
            "http://env:2"
        );
    }

    #[test]
    fn default_when_nothing_set() {
        assert_eq!(resolve_gateway(None, None), DEFAULT_GATEWAY);
    }

    #[test]
    fn parses_space_separated_flag() {
        assert_eq!(
            parse_gateway_flag(v(&["--gateway", "http://x:9"])),
            Some("http://x:9".to_string())
        );
    }

    #[test]
    fn parses_equals_form() {
        assert_eq!(
            parse_gateway_flag(v(&["--gateway=http://y:8"])),
            Some("http://y:8".to_string())
        );
    }

    #[test]
    fn absent_flag_is_none() {
        assert_eq!(parse_gateway_flag(v(&["--other", "z"])), None);
        assert_eq!(parse_gateway_flag(v(&["--gateway"])), None); // no value
    }
}
