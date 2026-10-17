//! Quick release tagging: s/m/l bumps from the highest stable version tag
//! across the local repository and the selected remote, mirroring xiaoli /tag.
use crate::git::Repository;
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Prefix<'a> {
    Plain,
    BigV,
    SmallV,
    Custom(&'a str),
}
impl<'a> Prefix<'a> {
    fn render(self) -> &'a str {
        match self {
            Prefix::SmallV => "v",
            Prefix::BigV => "V",
            Prefix::Plain => "",
            Prefix::Custom(prefix) => prefix,
        }
    }
    /// Same numbers: custom beats v, then V, then no prefix.
    fn rank(self) -> u8 {
        match self {
            Prefix::Custom(_) => 3,
            Prefix::SmallV => 2,
            Prefix::BigV => 1,
            Prefix::Plain => 0,
        }
    }
}

type Version<'a> = (Prefix<'a>, u64, u64, u64);

/// Accepts a stable x.y.z version with an optional prefix made of ASCII
/// letters, digits, hyphens and underscores (for example ease-v0.2.1).
/// Rejects prerelease/build suffixes and leading zeros in version numbers.
fn parse(name: &str) -> Option<Version<'_>> {
    let mut parts: Vec<&str> = name.split('.').collect();
    if parts.len() != 3 {
        return None;
    }
    let split = parts[0]
        .bytes()
        .rposition(|b| !b.is_ascii_digit())
        .map_or(0, |i| i + 1);
    let (prefix, major) = parts[0].split_at(split);
    if prefix.starts_with('-')
        || !prefix
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        return None;
    }
    let prefix = match prefix {
        "" => Prefix::Plain,
        "v" => Prefix::SmallV,
        "V" => Prefix::BigV,
        other => Prefix::Custom(other),
    };
    parts[0] = major;
    let mut nums = [0u64; 3];
    for (i, part) in parts.iter().enumerate() {
        if part.is_empty() || part.len() > 9 || !part.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        if part.len() > 1 && part.starts_with('0') {
            return None;
        }
        nums[i] = part.parse().ok()?;
    }
    Some((prefix, nums[0], nums[1], nums[2]))
}

/// Highest stable version by numeric compare, prefix breaking ties.
/// Falls back to v0.0.0 when no tag qualifies.
pub fn highest(tags: &[String]) -> Version<'_> {
    tags.iter()
        .filter_map(|t| parse(t))
        .max_by_key(|(p, a, b, c)| (*a, *b, *c, p.rank(), p.render()))
        .unwrap_or((Prefix::SmallV, 0, 0, 0))
}

/// s = patch +1, m = minor +1, l = major +1, keeping the base prefix.
pub fn bump(base: Version<'_>, level: &str) -> Result<String, String> {
    let (p, a, b, c) = base;
    let (a, b, c) = match level {
        "s" => (a, b, c + 1),
        "m" => (a, b + 1, 0),
        "l" => (a + 1, 0, 0),
        _ => return Err("Tag level must be s, m or l.".into()),
    };
    Ok(format!("{}{a}.{b}.{c}", p.render()))
}

fn render(version: Version<'_>) -> String {
    format!(
        "{}{}.{}.{}",
        version.0.render(),
        version.1,
        version.2,
        version.3
    )
}

#[derive(Clone, Debug, Serialize)]
pub struct TagPreview {
    pub base: String,
    pub versions: BTreeMap<String, String>,
    pub head: String,
    pub head_short: String,
    pub branch: String,
    pub summary: String,
    pub remote: String,
    pub push_url: String,
}

/// origin wins; a single remote is used; several remotes without origin error out.
fn select_remote(repo: &Repository) -> Result<Option<String>, String> {
    let remotes = repo.remotes()?;
    if remotes.is_empty() {
        return Ok(None);
    }
    if remotes.iter().any(|r| r == "origin") {
        return Ok(Some("origin".into()));
    }
    if remotes.len() == 1 {
        return Ok(Some(remotes[0].clone()));
    }
    Err(format!(
        "Multiple remotes ({}) and no origin; cannot pick one.",
        remotes.join(", ")
    ))
}

/// Formal confirmation preview: requires a clean tree (untracked included) and,
/// whenever a remote is configured, a successful remote tag read — even when the
/// user only wants a local tag.
pub fn prepare(repo: &Repository) -> Result<TagPreview, String> {
    if !repo.clean()? {
        return Err("Tagging requires a clean working tree, including untracked files.".into());
    }
    let remote = select_remote(repo)?;
    let mut tags = repo.local_tags()?;
    let mut push_url = String::new();
    if let Some(name) = &remote {
        push_url = repo.push_url(name)?;
        tags.extend(repo.remote_tags(&push_url)?);
    }
    let base = highest(&tags);
    let (head, summary) = repo.head_info()?;
    let branch = repo.branch()?;
    let mut versions = BTreeMap::new();
    for level in ["s", "m", "l"] {
        versions.insert(level.to_string(), bump(base, level)?);
    }
    Ok(TagPreview {
        base: render(base),
        versions,
        head_short: head[..7.min(head.len())].to_string(),
        head,
        branch,
        summary,
        remote: remote.unwrap_or_default(),
        push_url,
    })
}

#[derive(Debug)]
pub enum TagError {
    Plain(String),
    PushFailed {
        message: String,
        version: String,
        commit: String,
        remote: String,
        push_url: String,
    },
}

/// Re-checks the tree and HEAD against the preview, creates the annotated tag,
/// then optionally verifies push URL and tag target once more before pushing.
/// A failed push keeps the local tag; retry reuses the same version.
pub fn create(
    repo: &Repository,
    version: &str,
    head: &str,
    push: bool,
    remote: &str,
    push_url: &str,
) -> Result<String, TagError> {
    if parse(version).is_none() {
        return Err(TagError::Plain(format!("Invalid tag name {version}.")));
    }
    if !repo.clean().map_err(TagError::Plain)? {
        return Err(TagError::Plain(
            "Working tree changed since the preview; run Tag again.".into(),
        ));
    }
    if repo.head_sha().map_err(TagError::Plain)? != head {
        return Err(TagError::Plain(
            "HEAD moved since the preview; run Tag again.".into(),
        ));
    }
    repo.create_tag(version, head).map_err(TagError::Plain)?;
    if !push {
        return Ok(format!("Created local tag {version}"));
    }
    let fail = |message: String| TagError::PushFailed {
        message,
        version: version.to_string(),
        commit: head.to_string(),
        remote: remote.to_string(),
        push_url: push_url.to_string(),
    };
    push_checked(repo, version, head, remote, push_url, &fail)?;
    Ok(format!("Created and pushed {version}"))
}

/// Retries a push for an already-created tag: verifies the tag still points at
/// the previewed commit and the push URL is unchanged. No tree/HEAD checks.
pub fn retry_push(
    repo: &Repository,
    version: &str,
    commit: &str,
    remote: &str,
    push_url: &str,
) -> Result<String, TagError> {
    let fail = |message: String| TagError::PushFailed {
        message,
        version: version.to_string(),
        commit: commit.to_string(),
        remote: remote.to_string(),
        push_url: push_url.to_string(),
    };
    push_checked(repo, version, commit, remote, push_url, &fail)?;
    Ok(format!("Pushed {version}"))
}

fn push_checked(
    repo: &Repository,
    version: &str,
    commit: &str,
    remote: &str,
    push_url: &str,
    fail: &dyn Fn(String) -> TagError,
) -> Result<(), TagError> {
    let now = repo.push_url(remote).map_err(fail)?;
    if now != push_url {
        return Err(fail(format!(
            "Remote push URL changed since the preview ({push_url} → {now}). Tag {version} kept locally."
        )));
    }
    if repo.tag_commit(version).map_err(fail)? != commit {
        return Err(fail(format!(
            "Tag {version} no longer points at the previewed commit."
        )));
    }
    repo.push_tag(remote, version)
        .map_err(|e| fail(format!("Push failed: {e}. Tag {version} kept locally.")))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(names: &[&str]) -> Vec<String> {
        names.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn parses_strict_versions() {
        for name in [
            "1.2.3",
            "v1.2.3",
            "V1.2.3",
            "v0.0.0",
            "ease-v0.2.1",
            "release-1.2.3",
            "app2_v1.2.3",
        ] {
            assert!(parse(name).is_some(), "{name} should parse");
        }
        for name in [
            "v1.2.3-beta",
            "v1.2.3+build",
            "v1.02.3",
            "ease-v01.2.3",
            "-ease-v1.2.3",
            "ease v1.2.3",
            "ease-v1.2.3-beta",
            "v1.2",
            "v1.2.3.4",
            "vx.y.z",
            "",
        ] {
            assert!(parse(name).is_none(), "{name} should not parse");
        }
    }

    #[test]
    fn highest_compares_numerically_and_keeps_prefix() {
        assert_eq!(render(highest(&v(&["v1.3.8", "v1.10.0"]))), "v1.10.0");
        assert_eq!(render(highest(&v(&["2.0.0", "V1.9.9"]))), "2.0.0");
        assert_eq!(render(highest(&v(&["V1.2.3", "v1.2.3"]))), "v1.2.3");
        assert_eq!(render(highest(&v(&["1.2.3", "V1.2.3"]))), "V1.2.3");
        assert_eq!(render(highest(&v(&["v1.2.3-rc1", "junk"]))), "v0.0.0");
        assert_eq!(render(highest(&[])), "v0.0.0");
    }

    #[test]
    fn custom_prefix_is_preserved_and_ties_are_deterministic() {
        let tags = v(&["ease-v0.2.1", "ease-v0.10.0", "ease-v0.9.9"]);
        let base = highest(&tags);
        assert_eq!(render(base), "ease-v0.10.0");
        assert_eq!(bump(base, "s").unwrap(), "ease-v0.10.1");
        assert_eq!(bump(base, "m").unwrap(), "ease-v0.11.0");
        assert_eq!(bump(base, "l").unwrap(), "ease-v1.0.0");
        let mut tags = v(&["v0.2.1", "ease-v0.2.1", "app-v0.2.1"]);
        assert_eq!(render(highest(&tags)), "ease-v0.2.1");
        tags.reverse();
        assert_eq!(render(highest(&tags)), "ease-v0.2.1");
    }

    #[test]
    fn bumps_each_level() {
        let base = parse("v1.3.8").unwrap();
        assert_eq!(bump(base, "s").unwrap(), "v1.3.9");
        assert_eq!(bump(base, "m").unwrap(), "v1.4.0");
        assert_eq!(bump(base, "l").unwrap(), "v2.0.0");
        assert!(bump(base, "1.2.3").is_err());
        let plain = parse("0.0.0").unwrap();
        assert_eq!(bump(plain, "s").unwrap(), "0.0.1");
    }
}
