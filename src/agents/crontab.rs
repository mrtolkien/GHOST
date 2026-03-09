use std::path::Path;

use mlua::prelude::*;

/// What kind of schedule a crontab entry uses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CrontabTrigger {
    Cron { expr: String },
    Idle { minutes: u64 },
}

/// A single entry in the crontab.
#[derive(Debug, Clone)]
pub struct CrontabEntry {
    pub run: String,
    pub kind: CrontabTrigger,
}

/// Parse `$WORKSPACE/agents/crontab.lua` into a list of entries.
pub fn load_crontab(workspace: &Path) -> Result<Vec<CrontabEntry>, String> {
    let crontab_path = workspace.join("agents/crontab.lua");
    let source = std::fs::read_to_string(&crontab_path)
        .map_err(|e| format!("failed to read crontab.lua: {e}"))?;
    parse_crontab(&source)
}

fn parse_crontab(source: &str) -> Result<Vec<CrontabEntry>, String> {
    let lua = Lua::new();
    let table: LuaTable = lua
        .load(source)
        .eval()
        .map_err(|e| format!("crontab.lua eval error: {e}"))?;

    let mut entries = Vec::new();
    for item in table.sequence_values::<LuaTable>() {
        let item = item.map_err(|e| format!("crontab entry error: {e}"))?;

        let run: String = item
            .get("run")
            .map_err(|e| format!("missing 'run' field: {e}"))?;
        if run.is_empty() {
            return Err("crontab entry has empty 'run' field".to_string());
        }

        let cron: Option<String> = item.get("cron").map_err(|e| format!("cron field: {e}"))?;
        let idle_minutes: Option<u64> = item
            .get("idle_minutes")
            .map_err(|e| format!("idle_minutes field: {e}"))?;

        let kind = match (cron, idle_minutes) {
            (Some(expr), None) => {
                if expr.is_empty() {
                    return Err(format!("crontab entry '{run}' has empty cron expression"));
                }
                // Validate cron expression (prepend seconds field for cron crate)
                let full = format!("0 {expr}");
                full.parse::<cron::Schedule>()
                    .map_err(|e| format!("crontab entry '{run}' has invalid cron '{expr}': {e}"))?;
                CrontabTrigger::Cron { expr }
            }
            (None, Some(minutes)) => {
                if minutes == 0 {
                    return Err(format!(
                        "crontab entry '{run}' has idle_minutes=0 (must be > 0)"
                    ));
                }
                CrontabTrigger::Idle { minutes }
            }
            (Some(_), Some(_)) => {
                return Err(format!(
                    "crontab entry '{run}' has both 'cron' and 'idle_minutes' — pick one"
                ));
            }
            (None, None) => {
                return Err(format!(
                    "crontab entry '{run}' must have either 'cron' or 'idle_minutes'"
                ));
            }
        };

        entries.push(CrontabEntry { run, kind });
    }

    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_idle_entry() {
        let entries =
            parse_crontab(r#"return { { idle_minutes = 30, run = "chat-reflection" } }"#).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].run, "chat-reflection");
        assert_eq!(entries[0].kind, CrontabTrigger::Idle { minutes: 30 });
    }

    #[test]
    fn parse_cron_entry() {
        let entries =
            parse_crontab(r#"return { { cron = "0 3 * * *", run = "daily-summary" } }"#).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].run, "daily-summary");
        assert_eq!(
            entries[0].kind,
            CrontabTrigger::Cron {
                expr: "0 3 * * *".to_string()
            }
        );
    }

    #[test]
    fn parse_multiple_entries() {
        let entries = parse_crontab(
            r#"return {
                { idle_minutes = 30, run = "chat-reflection" },
                { cron = "0 3 * * *", run = "daily-summary" },
            }"#,
        )
        .unwrap();
        assert_eq!(entries.len(), 2);
    }

    #[test]
    fn parse_empty_crontab() {
        let entries = parse_crontab("return {}").unwrap();
        assert!(entries.is_empty());
    }

    #[test]
    fn reject_missing_run() {
        let err = parse_crontab(r#"return { { idle_minutes = 30 } }"#).unwrap_err();
        assert!(err.contains("run"), "error: {err}");
    }

    #[test]
    fn reject_empty_run() {
        let err = parse_crontab(r#"return { { idle_minutes = 30, run = "" } }"#).unwrap_err();
        assert!(err.contains("empty 'run'"), "error: {err}");
    }

    #[test]
    fn reject_both_cron_and_idle() {
        let err =
            parse_crontab(r#"return { { cron = "0 3 * * *", idle_minutes = 30, run = "test" } }"#)
                .unwrap_err();
        assert!(err.contains("both"), "error: {err}");
    }

    #[test]
    fn reject_neither_cron_nor_idle() {
        let err = parse_crontab(r#"return { { run = "test" } }"#).unwrap_err();
        assert!(err.contains("must have either"), "error: {err}");
    }

    #[test]
    fn reject_invalid_cron() {
        let err = parse_crontab(r#"return { { cron = "not valid", run = "test" } }"#).unwrap_err();
        assert!(err.contains("invalid cron"), "error: {err}");
    }

    #[test]
    fn reject_zero_idle_minutes() {
        let err = parse_crontab(r#"return { { idle_minutes = 0, run = "test" } }"#).unwrap_err();
        assert!(err.contains("must be > 0"), "error: {err}");
    }

    #[test]
    fn default_crontab_parses() {
        let crontab_content = crate::bundled::bundled_files()
            .iter()
            .find(|f| f.path == "agents/crontab.lua")
            .expect("crontab.lua should be in bundled files")
            .content;
        let entries = parse_crontab(crontab_content).unwrap();
        assert!(!entries.is_empty(), "default crontab should have entries");
    }

    #[test]
    fn bundled_install_creates_crontab() {
        let dir = tempfile::TempDir::new().unwrap();
        crate::bundled::install_all(dir.path()).unwrap();
        assert!(dir.path().join("agents/crontab.lua").exists());
    }
}
