use super::*;
use std::fs;
use std::path::Path;

/// Install a stateful fake `gh` that implements the GitHub contents API
/// against a state directory next to the shim (same shim pattern as the
/// catalog github tests): raw GETs `cat` the file, JSON GETs return its
/// sha, PUTs base64-decode `content=` into place — and, like the real API,
/// reject an update of an existing file that doesn't supply `sha`.
pub(crate) fn repo_with_shim(dir: &Path) -> MemoryRepo {
    let state = dir.join("state");
    fs::create_dir_all(&state).unwrap();
    let shim = dir.join("gh");
    fs::write(
        &shim,
        format!(
            r#"#!/bin/sh
STATE="{state}"
case "$*" in
  "api -X PUT repos/acme/team-memory/contents/"*)
    file="${{4#repos/acme/team-memory/contents/}}"
    if [ -e "$STATE/$file" ]; then
      case "$*" in
        *" sha="*) ;;
        *) echo 'gh: Invalid request. "sha" wasn'\''t supplied. (HTTP 422)' >&2; exit 1 ;;
      esac
    fi
    mkdir -p "$STATE/$(dirname "$file")"
    printf '%s' "${{8#content=}}" | base64 -d > "$STATE/$file"
    printf '{{"content":{{"path":"%s"}}}}' "$file"
    ;;
  "api repos/acme/team-memory/contents/"*" -H Accept: application/vnd.github.raw")
    file="${{2#repos/acme/team-memory/contents/}}"
    if [ -e "$STATE/$file" ]; then cat "$STATE/$file"; else echo 'gh: Not Found (HTTP 404)' >&2; exit 1; fi
    ;;
  "api repos/acme/team-memory/contents/"*)
    file="${{2#repos/acme/team-memory/contents/}}"
    if [ -e "$STATE/$file" ]; then
      printf '{{"sha":"%s"}}' "$(cksum "$STATE/$file" | cut -d' ' -f1)"
    else
      echo 'gh: Not Found (HTTP 404)' >&2; exit 1
    fi
    ;;
  *)
    echo "gh shim: unexpected args: $*" >&2; exit 1
    ;;
esac
"#,
            state = state.display()
        ),
    )
    .unwrap();
    make_executable(&shim);
    MemoryRepo::new(shim, "acme/team-memory")
}

/// chmod +x, then probe until the script actually execs: a child forked by
/// a parallel test during the write can briefly hold the shim's write fd,
/// failing the first exec with ETXTBSY ("Text file busy").
fn make_executable(shim: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(shim, fs::Permissions::from_mode(0o755)).unwrap();
    }
    for _ in 0..100 {
        match std::process::Command::new(shim).arg("--probe").output() {
            Err(e) if e.raw_os_error() == Some(26) => {
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            _ => break,
        }
    }
}

pub(crate) fn state_file(dir: &Path, path: &str) -> String {
    fs::read_to_string(dir.join("state").join(path)).unwrap()
}

#[test]
fn put_file_creates_then_updates_in_place() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = repo_with_shim(tmp.path());

    // Create: no sha exists yet — the shim would 422 an update without one.
    repo.put_file("notes.md", "first version\n", "memory: create")
        .unwrap();
    assert_eq!(state_file(tmp.path(), "notes.md"), "first version\n");

    // Update: put_file must fetch the blob sha and pass it through.
    repo.put_file("notes.md", "second version\n", "memory: update")
        .unwrap();
    assert_eq!(state_file(tmp.path(), "notes.md"), "second version\n");
}

#[test]
fn read_file_returns_content_or_none() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = repo_with_shim(tmp.path());
    assert!(repo.read_file("index.md").is_none());

    repo.put_file("index.md", "# Index\n", "memory: seed")
        .unwrap();
    assert_eq!(repo.read_file("index.md").as_deref(), Some("# Index\n"));
}

#[test]
fn publish_writes_session_doc_and_index_to_the_default_branch() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = repo_with_shim(tmp.path());
    assert_eq!(repo.repo(), "acme/team-memory");
    assert_eq!(repo.html_url(), "https://github.com/acme/team-memory");

    repo.publish(
        "sessions/2026-08-11-fix-retries-abcd1234.md",
        "# Session: Fix retries\n",
        "# Session knowledge index\n\n- [Fix retries](sessions/2026-08-11-fix-retries-abcd1234.md)\n",
        "memory: Fix retries",
    )
    .unwrap();

    assert_eq!(
        state_file(tmp.path(), "sessions/2026-08-11-fix-retries-abcd1234.md"),
        "# Session: Fix retries\n"
    );
    assert!(state_file(tmp.path(), "index.md").contains("- [Fix retries]"));

    // Re-publishing (both files now exist) exercises the update path end to end.
    repo.publish(
        "sessions/2026-08-11-fix-retries-abcd1234.md",
        "# Session: Fix retries (amended)\n",
        "# Session knowledge index\n\n- [Fix retries v2](sessions/2026-08-11-fix-retries-abcd1234.md)\n",
        "memory: Fix retries",
    )
    .unwrap();
    assert!(state_file(tmp.path(), "index.md").contains("Fix retries v2"));
}

#[test]
fn publish_surfaces_gh_failures() {
    let tmp = tempfile::tempdir().unwrap();

    // A gh that always fails: the error message reaches the caller.
    let shim = tmp.path().join("gh");
    fs::write(
        &shim,
        "#!/bin/sh\necho 'gh: Forbidden (HTTP 403)' >&2\nexit 1\n",
    )
    .unwrap();
    make_executable(&shim);
    let repo = MemoryRepo::new(&shim, "acme/team-memory");
    let err = repo.publish("a.md", "x", "y", "m").unwrap_err();
    assert!(err.contains("403"), "{err}");
    // Reads never fail — they degrade to None.
    assert!(repo.read_file("index.md").is_none());

    // Missing binary → spawn error pointing at the install docs.
    let repo = MemoryRepo::new(tmp.path().join("not-a-real-gh"), "acme/team-memory");
    let err = repo.put_file("a.md", "x", "m").unwrap_err();
    assert!(err.contains("failed to run"), "{err}");
    assert!(err.contains("https://cli.github.com"), "{err}");

    // Logged-out gh → error says how to authenticate.
    let auth_shim = tmp.path().join("gh-logged-out");
    fs::write(
        &auth_shim,
        "#!/bin/sh\necho 'To get started with GitHub CLI, please run:  gh auth login' >&2\nexit 4\n",
    )
    .unwrap();
    make_executable(&auth_shim);
    let repo = MemoryRepo::new(&auth_shim, "acme/team-memory");
    let err = repo.put_file("a.md", "x", "m").unwrap_err();
    assert!(err.contains("not authenticated"), "{err}");
    assert!(err.contains("gh auth login"), "{err}");
}

#[test]
fn github_remotes_parse_to_repo_entities() {
    for url in [
        "git@github.com:acme/payments.git",
        "https://github.com/acme/payments",
        "https://github.com/acme/payments.git",
        "https://github.com/acme/payments/",
        "ssh://git@github.com/acme/payments.git",
        "https://github.acme-corp.com/acme/payments", // GitHub Enterprise
    ] {
        assert_eq!(
            github_entity_from_remote(url).as_deref(),
            Some("repo:acme/payments"),
            "{url}"
        );
    }
    for url in [
        "https://gitlab.com/acme/payments.git", // not GitHub — no repo: memory
        "git@gitlab.com:acme/payments.git",
        "https://github.com/acme",                // no repo segment
        "https://github.com/acme/payments/extra", // too deep
        "/local/bare/repo.git",                   // not a URL
        "",
    ] {
        assert_eq!(github_entity_from_remote(url), None, "{url}");
    }
}

#[test]
fn for_repo_reads_the_committed_memory_repo_file() {
    let _guard = catalog_mcp::testutil::ENV_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    std::env::set_var("OPENADE_GH_BIN", "/custom/gh");

    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();

    // No file → no team-level memory repo.
    assert!(MemoryRepo::for_repo(root).is_none());

    // Committed config: comments and blank lines are fine.
    fs::create_dir_all(root.join(".openade")).unwrap();
    fs::write(
        root.join(MEMORY_REPO_FILE),
        "# where session knowledge goes\n\nacme/team-memory\n",
    )
    .unwrap();
    let repo = MemoryRepo::for_repo(root).unwrap();
    assert_eq!(repo.repo(), "acme/team-memory");

    // Junk content is warned about and ignored.
    fs::write(root.join(MEMORY_REPO_FILE), "not-owner-name\n").unwrap();
    assert!(MemoryRepo::for_repo(root).is_none());

    // Valid file but gh disabled/unavailable: warned and disabled.
    fs::write(root.join(MEMORY_REPO_FILE), "acme/team-memory\n").unwrap();
    std::env::set_var("OPENADE_GITHUB_MEMORY", "0");
    assert!(MemoryRepo::for_repo(root).is_none());
    std::env::remove_var("OPENADE_GITHUB_MEMORY");
    std::env::remove_var("OPENADE_GH_BIN");
}

#[test]
fn from_env_requires_owner_name_and_a_gh_binary() {
    let _guard = catalog_mcp::testutil::ENV_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());

    // Valid owner/name with an explicit gh override.
    std::env::set_var(MEMORY_REPO_ENV, "acme/team-memory");
    std::env::set_var("OPENADE_GH_BIN", "/custom/gh");
    let repo = MemoryRepo::from_env().unwrap();
    assert_eq!(repo.repo(), "acme/team-memory");

    // Anything that isn't exactly owner/name is rejected.
    for bad in ["", "no-slash", "a/b/c", "a/", "/b", "//"] {
        std::env::set_var(MEMORY_REPO_ENV, bad);
        assert!(MemoryRepo::from_env().is_none(), "accepted {bad:?}");
    }

    // Configured repo but gh disabled/unavailable: warned about and
    // disabled rather than silently half-working.
    std::env::set_var(MEMORY_REPO_ENV, "acme/team-memory");
    std::env::remove_var("OPENADE_GH_BIN");
    std::env::set_var("OPENADE_GITHUB_MEMORY", "0");
    assert!(MemoryRepo::from_env().is_none());
    std::env::remove_var("OPENADE_GITHUB_MEMORY");

    // Unset → disabled.
    std::env::remove_var(MEMORY_REPO_ENV);
    assert!(MemoryRepo::from_env().is_none());
}
