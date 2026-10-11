//! Finder-launched apps do not inherit shell proxy variables. Supply macOS
//! manual system proxies to Git without changing global Git configuration.
use std::{collections::BTreeMap, process::Command};

pub(crate) fn apply(command: &mut Command, args: &[&str]) {
    if !args
        .first()
        .is_some_and(|a| matches!(*a, "fetch" | "pull" | "push" | "clone" | "ls-remote"))
    {
        return;
    }
    #[cfg(target_os = "macos")]
    if let Ok(output) = Command::new("/usr/sbin/scutil").arg("--proxy").output()
        && output.status.success()
    {
        let existing: BTreeMap<String, String> = std::env::vars_os()
            .filter_map(|(k, v)| Some((k.into_string().ok()?, v.into_string().ok()?)))
            .collect();
        command.envs(fallback(
            &String::from_utf8_lossy(&output.stdout),
            &existing,
        ));
    }
}

fn fallback(settings: &str, existing: &BTreeMap<String, String>) -> BTreeMap<String, String> {
    let mut values = BTreeMap::new();
    let mut exceptions = Vec::new();
    let mut in_exceptions = false;
    for line in settings.lines().map(str::trim) {
        if line.starts_with("ExceptionsList :") {
            in_exceptions = true;
            continue;
        }
        if line == "}" {
            in_exceptions = false;
            continue;
        }
        if let Some((key, value)) = line.split_once(" : ") {
            if in_exceptions {
                exceptions.push(value.strip_prefix('*').unwrap_or(value));
            } else {
                values.insert(key, value);
            }
        }
    }
    let has =
        |key: &str| existing.contains_key(key) || existing.contains_key(&key.to_ascii_uppercase());
    let mut result = BTreeMap::new();
    // Explicit environment values (including an empty value) take precedence.
    if has("all_proxy") {
        return result;
    }
    for (prefix, variable) in [("HTTP", "http_proxy"), ("HTTPS", "https_proxy")] {
        if has(variable) || values.get(format!("{prefix}Enable").as_str()) != Some(&"1") {
            continue;
        }
        let Some(host) = values.get(format!("{prefix}Proxy").as_str()) else {
            continue;
        };
        let Some(port) = values
            .get(format!("{prefix}Port").as_str())
            .and_then(|p| p.parse::<u16>().ok())
            .filter(|p| *p != 0)
        else {
            continue;
        };
        if host.is_empty() || host.contains(['/', '@', ' ', '\n']) {
            continue;
        }
        let host = if host.contains(':') && !host.starts_with('[') {
            format!("[{host}]")
        } else {
            host.to_string()
        };
        result.insert(variable.into(), format!("http://{host}:{port}"));
    }
    if !result.is_empty() && !has("no_proxy") && !exceptions.is_empty() {
        result.insert("no_proxy".into(), exceptions.join(","));
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    const SETTINGS: &str = "<dictionary> {\nExceptionsList : <array> {\n0 : localhost\n1 : *.local\n2 : 10.0.0.0/8\n}\nHTTPEnable : 1\nHTTPProxy : 127.0.0.1\nHTTPPort : 7890\nHTTPSEnable : 1\nHTTPSProxy : ::1\nHTTPSPort : 7891\n}";
    #[test]
    fn system_proxies_and_exclusions() {
        let result = fallback(SETTINGS, &BTreeMap::new());
        assert_eq!(result["http_proxy"], "http://127.0.0.1:7890");
        assert_eq!(result["https_proxy"], "http://[::1]:7891");
        assert_eq!(result["no_proxy"], "localhost,.local,10.0.0.0/8");
        assert!(
            fallback(
                "HTTPEnable : 0\nHTTPProxy : localhost\nHTTPPort : 80",
                &BTreeMap::new()
            )
            .is_empty()
        );
    }
    #[test]
    fn explicit_environment_wins() {
        let existing = BTreeMap::from([
            ("HTTPS_PROXY".into(), "".into()),
            ("no_proxy".into(), "custom".into()),
        ]);
        let result = fallback(SETTINGS, &existing);
        assert!(!result.contains_key("https_proxy"));
        assert!(!result.contains_key("no_proxy"));
        assert!(
            fallback(
                SETTINGS,
                &BTreeMap::from([("ALL_PROXY".into(), "socks5://localhost:1".into())])
            )
            .is_empty()
        );
    }
}
