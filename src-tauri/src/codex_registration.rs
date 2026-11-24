use std::{
    env, fs,
    io::{self, Write},
    path::{Path, PathBuf},
};
use tempfile::NamedTempFile;
use toml_edit::{Array, DocumentMut, Item, Table, value};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

struct Runtime {
    command: PathBuf,
    script: PathBuf,
}

impl Runtime {
    fn at(install: &Path) -> Self {
        let dir = install.join("mcp");
        Self {
            command: dir.join("cutroom-node.exe"),
            script: dir.join("server.cjs"),
        }
    }

    fn matches(&self, command: &str, script: &str) -> bool {
        let normalize = |p: &str| p.replace('/', "\\").to_lowercase();
        normalize(command) == normalize(&self.command.to_string_lossy())
            && normalize(script) == normalize(&self.script.to_string_lossy())
    }
}

fn edit_config(input: &str, runtime: &Runtime, register: bool) -> Result<Option<String>> {
    let mut doc = input
        .trim_start_matches('\u{feff}')
        .parse::<DocumentMut>()
        .map_err(|_| "Codex config.toml is not valid TOML")?;
    if !doc.contains_key("mcp_servers") {
        if !register {
            return Ok(None);
        }
        let mut table = Table::new();
        table.set_implicit(true);
        doc.insert("mcp_servers", Item::Table(table));
    }
    let servers = doc["mcp_servers"]
        .as_table_like_mut()
        .ok_or("Invalid mcp_servers table")?;
    if let Some(current) = servers.get("cutroom") {
        let command = current.get("command").and_then(Item::as_str).unwrap_or("");
        let args = current.get("args").and_then(Item::as_array);
        let script = args
            .filter(|a| a.len() == 1)
            .and_then(|a| a.get(0))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let owned = runtime.matches(command, script);
        if !register {
            if !owned {
                return Ok(None);
            }
            servers.remove("cutroom");
        } else if !owned {
            // Migrate the development command and older installations of this server only.
            let node = Path::new(command)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("");
            let dev = (node.eq_ignore_ascii_case("node.exe") || node.eq_ignore_ascii_case("node"))
                && script
                    .replace('\\', "/")
                    .ends_with("/integrations/cutroom-mcp/server.mjs");
            let previous = Path::new(command)
                .parent()
                .and_then(Path::parent)
                .is_some_and(|dir| Runtime::at(dir).matches(command, script));
            if !dev && !previous {
                return Err("Another server already uses the Codex name cutroom".into());
            }
        } else if current.get("tool_timeout_sec").is_some() {
            return Ok(None);
        }
    } else if !register {
        return Ok(None);
    }

    if register {
        let server = servers
            .entry("cutroom")
            .or_insert(Item::Table(Table::new()))
            .as_table_like_mut()
            .ok_or("Invalid cutroom server table")?;
        let mut args = Array::new();
        args.push(runtime.script.to_str().ok_or("Invalid server path")?);
        server.insert(
            "command",
            value(runtime.command.to_str().ok_or("Invalid runtime path")?),
        );
        server.insert("args", value(args));
        if !server.contains_key("tool_timeout_sec") {
            server.insert("tool_timeout_sec", value(1800));
        }
    }
    let mut output = doc.to_string();
    if input.starts_with('\u{feff}') {
        output.insert(0, '\u{feff}');
    }
    if input.contains("\r\n") {
        output = output.replace("\r\n", "\n").replace('\n', "\r\n");
    }
    Ok(Some(output))
}

fn read_config(path: &Path) -> io::Result<Option<String>> {
    match fs::read_to_string(path) {
        Ok(s) => Ok(Some(s)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

pub fn run(register: bool) -> Result<()> {
    let exe = env::current_exe()?;
    let runtime = Runtime::at(exe.parent().ok_or("Missing install directory")?);
    if register && (!runtime.command.is_file() || !runtime.script.is_file()) {
        return Err("Installed Cutroom MCP runtime is missing".into());
    }
    let home = env::var_os("CODEX_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| env::home_dir().map(|p| p.join(".codex")))
        .ok_or("Missing Codex home directory")?;
    let path = home.join("config.toml");
    let original = read_config(&path)?;
    let Some(output) = edit_config(original.as_deref().unwrap_or(""), &runtime, register)? else {
        return Ok(());
    };
    fs::create_dir_all(&home)?;
    let mut temp = NamedTempFile::new_in(&home)?;
    temp.write_all(output.as_bytes())?;
    temp.as_file().sync_all()?;
    if read_config(&path)? != original {
        return Err("Codex configuration changed during installation. Retry setup.".into());
    }
    if original.is_some() {
        temp.persist(path)?;
    } else {
        temp.persist_noclobber(path)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_update_and_remove_only_our_server() {
        let runtime = Runtime::at(Path::new(r"C:\테스트 경로\컷룸"));
        let existing = "# laptop\n[mcp_servers.other]\ncommand = 'unrelated'\n";
        let fresh = edit_config(existing, &runtime, true).unwrap().unwrap();
        assert!(fresh.contains(existing));
        assert!(edit_config(&fresh, &runtime, true).unwrap().is_none());
        let legacy = format!(
            "{existing}\n[mcp_servers.cutroom]\ncommand = 'node.exe'\nargs = ['D:/편집앱/tauri-app/integrations/cutroom-mcp/server.mjs']\ntool_timeout_sec = 300\nenabled = false\n"
        );
        let migrated = edit_config(&legacy, &runtime, true).unwrap().unwrap();
        let doc = migrated.parse::<DocumentMut>().unwrap();
        assert_eq!(
            doc["mcp_servers"]["cutroom"]["enabled"].as_bool(),
            Some(false)
        );
        assert_eq!(
            doc["mcp_servers"]["cutroom"]["tool_timeout_sec"].as_integer(),
            Some(300)
        );
        let clean = edit_config(&migrated, &runtime, false).unwrap().unwrap();
        assert!(clean.contains(existing) && !clean.contains("cutroom"));
        let another = Runtime::at(Path::new(r"D:\새 컷룸"));
        assert!(edit_config(&fresh, &another, false).unwrap().is_none());
        assert!(
            edit_config(&fresh, &another, true)
                .unwrap()
                .unwrap()
                .contains("새 컷룸")
        );
    }

    #[test]
    fn preserve_invalid_files_and_unrelated_server_with_same_name() {
        let runtime = Runtime::at(Path::new(r"C:\컷룸"));
        assert!(edit_config("[broken", &runtime, true).is_err());
        let unrelated = "[mcp_servers.cutroom]\nurl = 'https://example.invalid'\n";
        assert!(edit_config(unrelated, &runtime, true).is_err());
        assert!(edit_config(unrelated, &runtime, false).unwrap().is_none());
        let inline = "mcp_servers = { other = { command = 'keep' } }\n";
        let fresh = edit_config(inline, &runtime, true).unwrap().unwrap();
        let doc = fresh.parse::<DocumentMut>().unwrap();
        assert_eq!(
            doc["mcp_servers"]["other"]["command"].as_str(),
            Some("keep")
        );
    }
}
