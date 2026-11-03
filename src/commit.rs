//! Follow-up actions for a successful AI commit. Never restage or recommit.
use crate::{
    git::Repository,
    tag::{self, TagError},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AfterCommit {
    None,
    Push,
    PushAndTag,
}

impl AfterCommit {
    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "" | "commit" => Ok(Self::None),
            "push" => Ok(Self::Push),
            "tag" => Ok(Self::PushAndTag),
            _ => Err("Unknown commit action.".into()),
        }
    }
}

/// Called only after Git has successfully created the commit. Errors describe
/// the completed steps; retrying Push/Tag must never create another commit.
pub fn finish(
    repo: &Repository,
    action: AfterCommit,
    commit: &str,
    push_tag: bool,
) -> Result<String, TagError> {
    if action == AfterCommit::None {
        return Ok("Committed staged changes.".into());
    }
    if repo.head_sha().map_err(TagError::Plain)? != commit {
        return Err(TagError::Plain(
            "Commit succeeded, but HEAD changed; automatic Push stopped.".into(),
        ));
    }
    repo.network("push").map_err(|error| {
        TagError::Plain(format!(
            "Commit succeeded; Push failed: {error}. The local commit is kept. Retry with Push."
        ))
    })?;
    if action == AfterCommit::Push {
        return Ok("Committed and pushed staged changes.".into());
    }
    let tag_error = |error| {
        TagError::Plain(format!(
            "Commit and Push succeeded; Tag failed: {error}. Retry with Tag."
        ))
    };
    if repo.head_sha().map_err(tag_error)? != commit {
        return Err(tag_error(
            "HEAD changed after Push; no tag was created.".to_string(),
        ));
    }
    let remote = repo.push_remote().map_err(tag_error)?;
    let preview = tag::prepare_for_remote(repo, Some(remote)).map_err(tag_error)?;
    if preview.head != commit {
        return Err(tag_error(
            "HEAD changed while preparing the tag.".to_string(),
        ));
    }
    let version = &preview.versions["s"];
    match tag::create(
        repo,
        version,
        commit,
        push_tag,
        &preview.remote,
        &preview.push_url,
    ) {
        Ok(done) => Ok(format!("Commit and Push succeeded. {done}")),
        Err(TagError::Plain(error)) => Err(tag_error(error)),
        Err(TagError::PushFailed {
            message,
            version,
            commit,
            remote,
            push_url,
        }) => Err(TagError::PushFailed {
            message: format!("Commit and Push succeeded; {message}"),
            version,
            commit,
            remote,
            push_url,
        }),
    }
}
