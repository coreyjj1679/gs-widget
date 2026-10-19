//! Generate copyable local-client configuration; never overwrite another app's settings.
use serde_json::json;
use slint::ComponentHandle;

use crate::{db, App};

struct Config {
    command: String,
    database: String,
    codex: String,
    cursor: String,
    claude: String,
    cli: String,
}

fn plain_path(path: &std::path::Path) -> String {
    let path = path.to_string_lossy();
    if let Some(unc) = path.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{unc}")
    } else {
        path.strip_prefix(r"\\?\").unwrap_or(&path).to_string()
    }
}

fn shell_quote(text: &str, windows: bool) -> String {
    if windows {
        format!("'{}'", text.replace('\'', "''"))
    } else {
        format!("'{}'", text.replace('\'', "'\"'\"'"))
    }
}

fn configuration(command: String, database: String, windows: bool) -> Config {
    let server = json!({"command": command, "args": ["--mcp"], "env": {"GERSANG_DB": database}});
    let claude = serde_json::to_string_pretty(&json!({"mcpServers": {"gersang": server}})).unwrap();
    let mut cursor_server = server;
    cursor_server["type"] = json!("stdio");
    let cursor =
        serde_json::to_string_pretty(&json!({"mcpServers": {"gersang": cursor_server}})).unwrap();
    // JSON strings are also valid TOML basic strings, including Windows backslashes.
    let codex = format!("[mcp_servers.gersang]\ncommand = {}\nargs = [\"--mcp\"]\n\n[mcp_servers.gersang.env]\nGERSANG_DB = {}\n",
        serde_json::to_string(&command).unwrap(), serde_json::to_string(&database).unwrap());
    let cli = format!(
        "claude mcp add --transport stdio --scope user gersang --env {} -- {} --mcp",
        shell_quote(&format!("GERSANG_DB={database}"), windows),
        shell_quote(&command, windows)
    );
    Config {
        command,
        database,
        codex,
        cursor,
        claude,
        cli,
    }
}

pub fn install(app: &App) {
    let config = std::env::current_exe()
        .map_err(|error| error.to_string())
        .and_then(|exe| {
            db::mcp::database_path()
                .map(|db| configuration(plain_path(&exe), plain_path(&db), cfg!(windows)))
        });
    match config {
        Ok(config) => {
            app.set_mcp_command(config.command.into());
            app.set_mcp_database(config.database.into());
            app.set_mcp_codex_config(config.codex.into());
            app.set_mcp_cursor_config(config.cursor.into());
            app.set_mcp_claude_config(config.claude.into());
            app.set_mcp_claude_cli(config.cli.into());
        }
        Err(error) => app.set_mcp_setup_error(db::localized_error(&app.get_lang(), &error).into()),
    }
    app.set_mcp_windows(cfg!(windows));
    let weak = app.as_weak();
    app.on_mcp_copy(move |text| {
        let Some(app) = weak.upgrade() else { return };
        let copied = crate::window::try_copy_to_clipboard(&text);
        let lang = app.get_lang();
        app.set_mcp_copy_notice(if copied {
            crate::db::ui_text(&lang, "已複製到剪貼簿", "Copied to clipboard")
        } else {
            crate::db::ui_text(&lang, "無法複製，請選取文字後手動複製。", "Couldn't copy. Select the text and copy it manually.")
        }.into());
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn configurations_preserve_spaces_unicode_and_quotes() {
        for windows in [true, false] {
            let exe = if windows {
                r"C:\Users\玩家's PC\巨商\gersang-widget.exe"
            } else {
                "/Users/玩家's Mac/巨商/gersang-widget"
            };
            let db = format!("{exe}/../data/gersang.db");
            let config = configuration(exe.into(), db.clone(), windows);
            for body in [&config.cursor, &config.claude] {
                let value: serde_json::Value = serde_json::from_str(body).unwrap();
                assert_eq!(value["mcpServers"]["gersang"]["command"], exe);
                assert_eq!(value["mcpServers"]["gersang"]["env"]["GERSANG_DB"], db);
                assert_eq!(value["mcpServers"]["gersang"]["args"], json!(["--mcp"]));
            }
            assert!(config.codex.contains(&serde_json::to_string(exe).unwrap()));
            assert!(config.cli.contains(if windows {
                "玩家''s PC"
            } else {
                "玩家'\"'\"'s Mac"
            }));
        }
    }
}
