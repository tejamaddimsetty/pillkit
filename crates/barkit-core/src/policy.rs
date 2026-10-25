//! Checks applied before the host acts on anything it was sent.

use crate::model::{Action, Item};

/// Only URLs the developer registered in the portal may be opened.
pub fn url_allowed(url: &str, allowed_prefixes: &[String]) -> bool {
    let url = url.trim();
    if url.is_empty() || url.chars().any(|c| c.is_control()) {
        return false;
    }
    let lower = url.to_ascii_lowercase();
    // Never hand the OS a local file or script, whatever the allowlist says.
    for banned in [
        "file:",
        "javascript:",
        "data:",
        "vbscript:",
        "ms-msdt:",
        "search-ms:",
    ] {
        if lower.starts_with(banned) {
            return false;
        }
    }
    allowed_prefixes.iter().any(|p| {
        let p = p.to_ascii_lowercase();
        // "https://acme.com" is the home page of the allowed "https://acme.com/".
        !p.is_empty()
            && (lower.starts_with(&p)
                || (p.starts_with("https://") && p.ends_with('/') && lower == p[..p.len() - 1]))
    })
}

/// The URL an action would open, if it opens one and is allowed to.
pub fn resolve_url<'a>(action: &'a Action, allowed_prefixes: &[String]) -> Option<&'a str> {
    match action {
        Action::OpenUrl { url } | Action::DeepLink { url }
            if url_allowed(url, allowed_prefixes) =>
        {
            Some(url)
        }
        _ => None,
    }
}

pub const MAX_TITLE: usize = 40;
pub const MAX_SUBTITLE: usize = 60;
pub const MAX_LABEL: usize = 16;
pub const MAX_PNG_BYTES: usize = 64 * 1024;

/// Trims fields to their limits so one item can't take over the bar.
pub fn sanitize(mut item: Item) -> Item {
    item.title = clip(&item.title, MAX_TITLE);
    item.subtitle = clip(&item.subtitle, MAX_SUBTITLE);
    item.label = clip(&item.label, MAX_LABEL);
    item.duration_secs = item.duration_secs.clamp(5, 300);
    item.weight = item.weight.clamp(1, 100);
    if let crate::model::Icon::Png { data } = &item.icon {
        if data.len() > MAX_PNG_BYTES * 4 / 3 + 4 {
            item.icon = crate::model::Icon::Monogram;
        }
    }
    item
}

fn clip(s: &str, max: usize) -> String {
    let clean: String = s.chars().filter(|c| !c.is_control()).collect();
    if clean.chars().count() <= max {
        clean
    } else {
        let mut out: String = clean.chars().take(max - 1).collect();
        out.push('…');
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn allow() -> Vec<String> {
        vec!["https://acme.com/".into(), "acmeclip://".into()]
    }

    #[test]
    fn allowlist() {
        assert!(url_allowed("https://acme.com/pricing", &allow()));
        assert!(url_allowed("ACMECLIP://settings", &allow()));
        assert!(!url_allowed("https://acme.com.evil.io/", &allow()));
        assert!(url_allowed("https://acme.com", &allow()));
        assert!(!url_allowed("https://acme.co", &allow()));
        assert!(!url_allowed("https://evil.io/", &allow()));
        assert!(!url_allowed("file:///etc/passwd", &["file:".into()]));
        assert!(!url_allowed("https://acme.com/\nx", &allow()));
        assert!(!url_allowed("https://acme.com/", &[]));
    }

    #[test]
    fn clipping() {
        assert_eq!(clip("abc", 5), "abc");
        assert_eq!(clip("abcdefgh", 5), "abcd…");
    }
}
