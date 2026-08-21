//! `scema connect` — wiring the loop into whatever assistant the operator already uses.
//!
//! Omni is meant to be **universal**: the same loop behind a terminal, a browser tab and a
//! coding assistant, with no per-host reimplementation. `scema-mcp` already does the last
//! of those — it links `scema-agent` directly rather than proxying the daemon, so there is
//! one implementation and no way for two surfaces to disagree. What was missing was the
//! last hundred metres: every host wants the same three facts (a command, its arguments,
//! and where the config lives) written in a slightly different shape, in a file whose
//! location nobody remembers.
//!
//! So this module holds a table, and the table is the whole feature.
//!
//! ## `--write` only touches project-local files, and that is not timidity
//!
//! A per-project `.mcp.json` sits next to the code, is visible in `git status`, and is
//! reviewed like anything else in the repository. A user-level assistant config is none of
//! those things: it is invisible to the project, shared by every project, and editing it on
//! somebody's behalf means a tool they installed for one repository quietly gained the
//! ability to observe every repository they open afterwards.
//!
//! The distinction is [`Scope`], and the rule is that [`Scope::User`] targets are *printed*
//! with their path and never written. It costs one paste and it keeps "I installed a CLI"
//! from meaning "I changed my editor's global configuration".
//!
//! ## The command that gets written is `scema-mcp`, with `--allow` pinned to the project
//!
//! Not `--allow ~`, and not an unconfined server. `scema_tools::Workspace` answers *where*,
//! and it only works if somebody sets the roots to something narrower than a home
//! directory. A cooperative model asked to audit a project will reason its way to `~/.ssh`
//! — that being genuinely relevant to an audit — and the confinement is what stops it.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use serde_json::{json, Value};

/// Where a host keeps the configuration this writes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    /// A file inside the project. Reviewable, committable, and safe to write.
    Project,
    /// A file in the operator's home directory, shared by every project they open.
    /// **Printed, never written.**
    User,
}

/// How a host spells an MCP server entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shape {
    /// `{"mcpServers": {"<name>": {"command": ..., "args": [...]}}}`
    McpServers,
    /// `{"servers": {"<name>": {"command": ..., "args": [...], "type": "stdio"}}}`
    VsCodeServers,
    /// `{"context_servers": {"<name>": {"command": {"path": ..., "args": [...]}}}}`
    ZedContextServers,
    /// `[mcp_servers.<name>]` in TOML.
    TomlTable,
}

/// One assistant.
#[derive(Clone, Copy, Debug)]
pub struct Host {
    /// What the operator types.
    pub key: &'static str,
    pub label: &'static str,
    pub shape: Shape,
    pub scope: Scope,
    /// Path relative to the project root, for [`Scope::Project`].
    pub project_path: &'static str,
    /// Where to look, for [`Scope::User`]. Rendered with `~` unexpanded on purpose —
    /// spelling out somebody's home directory in terminal output that ends up in a bug
    /// report is a small leak with no upside.
    pub user_hint: &'static str,
    /// Anything the operator has to do that this cannot: restart, a toggle, a menu.
    pub after: &'static str,
}

pub const HOSTS: [Host; 7] = [
    Host {
        key: "claude-code",
        label: "Claude Code",
        shape: Shape::McpServers,
        scope: Scope::Project,
        project_path: ".mcp.json",
        user_hint: "",
        after: "Run `/mcp` in Claude Code to confirm the server is listed. A project \
                 `.mcp.json` prompts for approval the first time it is seen.",
    },
    Host {
        key: "claude-desktop",
        label: "Claude Desktop",
        shape: Shape::McpServers,
        scope: Scope::User,
        project_path: "",
        user_hint: "macOS  ~/Library/Application Support/Claude/claude_desktop_config.json\n\
                    Windows %APPDATA%\\Claude\\claude_desktop_config.json",
        after: "Quit and reopen Claude Desktop — it reads this file once at start.",
    },
    Host {
        key: "cursor",
        label: "Cursor",
        shape: Shape::McpServers,
        scope: Scope::Project,
        project_path: ".cursor/mcp.json",
        user_hint: "",
        after: "Settings -> MCP should list `scema` once the window reloads.",
    },
    Host {
        key: "vscode",
        label: "VS Code (Copilot / agent mode)",
        shape: Shape::VsCodeServers,
        scope: Scope::Project,
        project_path: ".vscode/mcp.json",
        user_hint: "",
        after: "Open the file in VS Code and use the `Start` lens above the server entry.",
    },
    Host {
        key: "windsurf",
        label: "Windsurf",
        shape: Shape::McpServers,
        scope: Scope::User,
        project_path: "",
        user_hint: "~/.codeium/windsurf/mcp_config.json",
        after: "Refresh the MCP server list in Windsurf's settings.",
    },
    Host {
        key: "zed",
        label: "Zed",
        shape: Shape::ZedContextServers,
        scope: Scope::User,
        project_path: "",
        user_hint: "~/.config/zed/settings.json  (merge into the existing object)",
        after: "Zed reloads settings on save.",
    },
    Host {
        key: "codex",
        label: "OpenAI Codex CLI",
        shape: Shape::TomlTable,
        scope: Scope::User,
        project_path: "",
        user_hint: "~/.codex/config.toml",
        after: "Start a new Codex session; the config is read at launch.",
    },
];

pub fn host(key: &str) -> Option<&'static Host> {
    HOSTS.iter().find(|h| h.key.eq_ignore_ascii_case(key))
}

/// The command and arguments every host is being told to run.
///
/// One function so that a change to how the server is invoked — a new flag, a renamed
/// binary — cannot be applied to six hosts and forgotten on the seventh.
pub fn invocation(allow: &Path, allow_decide: bool) -> (String, Vec<String>) {
    let mut args = vec!["--allow".to_string(), allow.display().to_string()];
    if allow_decide {
        // Off unless asked for, and asked for explicitly here rather than defaulted, because
        // `omni_decide` is not even *advertised* to a model without it. A tool a model can
        // see and cannot use teaches it to retry; a tool that seals records without the
        // operator choosing that is worse.
        args.push("--allow-decide".to_string());
    }
    ("scema-mcp".to_string(), args)
}

/// The snippet for one host, as text ready to paste.
pub fn snippet(h: &Host, allow: &Path, allow_decide: bool) -> Result<String> {
    let (command, args) = invocation(allow, allow_decide);
    Ok(match h.shape {
        Shape::McpServers => pretty(&json!({
            "mcpServers": { "scema": { "command": command, "args": args } }
        }))?,
        Shape::VsCodeServers => pretty(&json!({
            "servers": { "scema": { "type": "stdio", "command": command, "args": args } }
        }))?,
        Shape::ZedContextServers => pretty(&json!({
            "context_servers": {
                "scema": { "source": "custom", "command": { "path": command, "args": args } }
            }
        }))?,
        Shape::TomlTable => {
            let rendered: Vec<String> = args.iter().map(|a| format!("{a:?}")).collect();
            format!(
                "[mcp_servers.scema]\ncommand = {command:?}\nargs = [{}]\n",
                rendered.join(", ")
            )
        }
    })
}

fn pretty(v: &Value) -> Result<String> {
    Ok(serde_json::to_string_pretty(v)? + "\n")
}

/// The result of a `--write`.
#[derive(Debug)]
pub enum Written {
    /// Created a file that did not exist.
    Created(PathBuf),
    /// Merged the `scema` entry into an existing file, leaving everything else alone.
    Merged(PathBuf),
    /// Already present and identical.
    Unchanged(PathBuf),
}

/// Merge the `scema` entry into a host's project-local config.
///
/// **Merge, never overwrite.** A project `.mcp.json` routinely holds three other servers
/// somebody set up, and a tool that replaced the file to add one entry would delete them.
/// The merge is deliberately shallow: it replaces the value at `scema` and touches nothing
/// else, so a hand-edited entry is updated rather than duplicated.
pub fn write(h: &Host, project: &Path, allow: &Path, allow_decide: bool) -> Result<Written> {
    if h.scope != Scope::Project {
        return Err(anyhow!(
            "`{}` keeps its configuration outside the project, so this will not write it.\n\n  {}\n\n  A user-level assistant config is shared by every project you open. Editing it\n  on your behalf would mean a tool installed for one repository quietly gained\n  the ability to observe all of them. Paste the snippet above instead.",
            h.label,
            h.user_hint.replace('\n', "\n  ")
        ));
    }

    let target = project.join(h.project_path);
    let (command, args) = invocation(allow, allow_decide);
    let (container, entry) = match h.shape {
        Shape::McpServers => ("mcpServers", json!({ "command": command, "args": args })),
        Shape::VsCodeServers => (
            "servers",
            json!({ "type": "stdio", "command": command, "args": args }),
        ),
        // Neither of the remaining shapes has a project-local target in `HOSTS`, so this is
        // unreachable today. It is a hard error rather than a silent fallback because the
        // failure mode of guessing is a config file in a shape the host does not read, and
        // the symptom is "the server does not appear" with nothing to grep for.
        other => {
            return Err(anyhow!(
                "no project-local writer for {other:?}; print the snippet and paste it"
            ))
        }
    };

    let mut root: Value = if target.exists() {
        let text = std::fs::read_to_string(&target)
            .with_context(|| format!("reading {}", target.display()))?;
        if text.trim().is_empty() {
            json!({})
        } else {
            serde_json::from_str(&text).with_context(|| {
                format!(
                    "{} is not valid JSON. Fix or move it first — merging into a file this \
                     cannot parse would mean rewriting it from scratch and losing whatever \
                     is in there.",
                    target.display()
                )
            })?
        }
    } else {
        json!({})
    };

    let existed = target.exists();
    let servers = root
        .as_object_mut()
        .ok_or_else(|| anyhow!("{} is JSON but not an object", target.display()))?
        .entry(container)
        .or_insert_with(|| json!({}));
    let servers = servers
        .as_object_mut()
        .ok_or_else(|| anyhow!("`{container}` in {} is not an object", target.display()))?;

    if servers.get("scema") == Some(&entry) {
        return Ok(Written::Unchanged(target));
    }
    servers.insert("scema".to_string(), entry);

    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
    }
    // Write-then-rename, the same convention the bot workspace uses for every state file: a
    // half-written config is a config the host refuses to parse, and it would look like this
    // tool having corrupted it.
    let tmp = target.with_extension("tmp");
    std::fs::write(&tmp, pretty(&root)?).with_context(|| format!("writing {}", tmp.display()))?;
    std::fs::rename(&tmp, &target).with_context(|| format!("renaming into {}", target.display()))?;

    Ok(if existed { Written::Merged(target) } else { Written::Created(target) })
}

/// Every host, for `scema connect --list`.
pub fn catalogue() -> BTreeMap<&'static str, &'static Host> {
    HOSTS.iter().map(|h| (h.key, h)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn scratch() -> PathBuf {
        let p = std::env::temp_dir().join(format!(
            "scema-connect-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn every_host_produces_a_snippet_naming_the_same_binary() {
        // The reason `invocation` exists. Six hosts and one command: a change to how the
        // server is started cannot be applied to five of them and forgotten on the sixth.
        for h in HOSTS {
            let text = snippet(&h, Path::new("/proj"), false).unwrap();
            assert!(text.contains("scema-mcp"), "{}: {text}", h.key);
            assert!(text.contains("--allow"), "{}: {text}", h.key);
        }
    }

    #[test]
    fn decide_is_absent_unless_it_was_asked_for() {
        // A snippet that quietly enabled record-sealing would put a write path behind a
        // model's tool call, which is the one thing this workspace does not do by default.
        for h in HOSTS {
            assert!(!snippet(&h, Path::new("/proj"), false).unwrap().contains("--allow-decide"));
            assert!(snippet(&h, Path::new("/proj"), true).unwrap().contains("--allow-decide"));
        }
    }

    #[test]
    fn a_user_scoped_host_is_refused_rather_than_written() {
        let dir = scratch();
        let h = host("windsurf").unwrap();
        let err = write(h, &dir, &dir, false).unwrap_err().to_string();
        assert!(err.contains("outside the project"), "{err}");
        assert!(err.contains(".codeium"), "the refusal must say where to paste it: {err}");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn writing_creates_the_file_when_it_is_absent() {
        let dir = scratch();
        let h = host("claude-code").unwrap();
        match write(h, &dir, &dir, false).unwrap() {
            Written::Created(p) => assert!(p.exists()),
            other => panic!("expected Created, got {other:?}"),
        }
        let text = fs::read_to_string(dir.join(".mcp.json")).unwrap();
        assert!(text.contains("scema-mcp"));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn writing_merges_and_never_clobbers_another_server() {
        // The failure this test exists for: somebody has three MCP servers configured and a
        // tool "adds" a fourth by replacing the file.
        let dir = scratch();
        fs::write(
            dir.join(".mcp.json"),
            r#"{"mcpServers":{"other":{"command":"other-thing","args":[]}}}"#,
        )
        .unwrap();
        let h = host("claude-code").unwrap();
        write(h, &dir, &dir, false).unwrap();
        let v: Value = serde_json::from_str(&fs::read_to_string(dir.join(".mcp.json")).unwrap()).unwrap();
        assert!(v["mcpServers"]["other"]["command"] == "other-thing", "{v}");
        assert!(v["mcpServers"]["scema"]["command"] == "scema-mcp", "{v}");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn writing_twice_is_a_no_op_the_second_time() {
        let dir = scratch();
        let h = host("claude-code").unwrap();
        write(h, &dir, &dir, false).unwrap();
        assert!(matches!(write(h, &dir, &dir, false).unwrap(), Written::Unchanged(_)));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn an_unparseable_config_is_refused_rather_than_replaced() {
        // Rewriting a file this cannot parse would destroy whatever is in it, and the
        // operator would find out later.
        let dir = scratch();
        fs::write(dir.join(".mcp.json"), "{ not json at all").unwrap();
        let h = host("claude-code").unwrap();
        let err = write(h, &dir, &dir, false).unwrap_err().to_string();
        assert!(err.contains("not valid JSON"), "{err}");
        assert_eq!(fs::read_to_string(dir.join(".mcp.json")).unwrap(), "{ not json at all");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn vscode_gets_its_own_shape() {
        // VS Code reads `servers`, not `mcpServers`. Getting this wrong produces a valid
        // JSON file that the host silently ignores, which is the worst kind of wrong.
        let text = snippet(host("vscode").unwrap(), Path::new("/p"), false).unwrap();
        assert!(text.contains("\"servers\""), "{text}");
        assert!(!text.contains("mcpServers"), "{text}");
    }

    #[test]
    fn the_toml_host_gets_toml() {
        let text = snippet(host("codex").unwrap(), Path::new("/p"), false).unwrap();
        assert!(text.starts_with("[mcp_servers.scema]"), "{text}");
        assert!(!text.contains('{'), "{text}");
    }

    #[test]
    fn every_host_key_is_unique_and_lowercase() {
        let mut keys: Vec<&str> = HOSTS.iter().map(|h| h.key).collect();
        keys.sort_unstable();
        let before = keys.len();
        keys.dedup();
        assert_eq!(before, keys.len());
        assert!(HOSTS.iter().all(|h| h.key == h.key.to_lowercase()));
    }

    #[test]
    fn every_host_says_what_the_operator_must_do_afterwards() {
        // The step this tool cannot take. A config written into a file the host has already
        // read is a config that does nothing, and "it did not work" is the report.
        assert!(HOSTS.iter().all(|h| !h.after.is_empty()));
    }
}
