use crate::config::Backend;

pub fn parse_backend_spec(spec: &str) -> Option<Backend> {
    let (name, raw_target) = spec.split_once('=')?;
    let name = name.trim();
    let raw_target = raw_target.trim();
    if name.is_empty() || raw_target.is_empty() {
        return None;
    }

    let (raw_url, priority) = if has_explicit_priority(raw_target) {
        match raw_target.rsplit_once(':') {
            Some((url_part, priority_part)) => {
                let priority = priority_part.parse::<u32>().ok()?;
                (url_part.trim(), priority)
            }
            None => (raw_target, 50),
        }
    } else {
        (raw_target, 50)
    };

    if raw_url.is_empty() {
        return None;
    }

    let url = if raw_url.starts_with("http://") || raw_url.starts_with("https://") {
        raw_url.to_string()
    } else {
        format!("http://{}", raw_url)
    };

    Some(Backend {
        name: name.to_string(),
        url,
        priority,
        ..Default::default()
    })
}

fn has_explicit_priority(raw_target: &str) -> bool {
    let remainder = raw_target
        .strip_prefix("http://")
        .or_else(|| raw_target.strip_prefix("https://"))
        .unwrap_or(raw_target);

    if remainder.starts_with('[') {
        if let Some(end) = remainder.find(']') {
            let after_bracket = &remainder[end + 1..];
            return after_bracket.matches(':').count() >= 2;
        }
    }

    remainder.matches(':').count() >= 2
}

#[cfg(test)]
mod tests {
    use super::parse_backend_spec;

    #[test]
    fn parses_documented_format() {
        let backend = parse_backend_spec("citadel=http://citadel:11434:100").unwrap();
        assert_eq!(backend.name, "citadel");
        assert_eq!(backend.url, "http://citadel:11434");
        assert_eq!(backend.priority, 100);
    }

    #[test]
    fn defaults_priority_when_omitted() {
        let backend = parse_backend_spec("edge=http://edge:11434").unwrap();
        assert_eq!(backend.url, "http://edge:11434");
        assert_eq!(backend.priority, 50);
    }

    #[test]
    fn accepts_https_and_bracketed_ipv6() {
        let backend = parse_backend_spec("gpu=https://[::1]:11434:70").unwrap();
        assert_eq!(backend.url, "https://[::1]:11434");
        assert_eq!(backend.priority, 70);
    }
}
