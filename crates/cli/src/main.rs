use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use doxus_core::db;
use doxus_core::search::SearchQuery;
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "doxus", about = "doxus — multi-source document search hub")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Add a project (Obsidian vault or directory)
    Project(ProjectArgs),
    /// Index all active projects
    Index,
    /// Search across indexed documents
    Search {
        /// Search query
        query: String,
        /// Max results to show
        #[arg(short, long, default_value = "10")]
        limit: usize,
        /// Restrict to project name
        #[arg(short, long)]
        project: Option<String>,
    },
    /// Show server status
    Status,
    /// Manage plugins
    Plugin(PluginArgs),
    /// Manage workspaces
    Workspace(WorkspaceArgs),
}

#[derive(Parser)]
struct ProjectArgs {
    #[command(subcommand)]
    action: ProjectAction,
}

#[derive(Subcommand)]
enum ProjectAction {
    /// Add a new project
    Add {
        /// Project name (slug)
        name: String,
        /// Path to the vault or directory
        path: PathBuf,
        /// Human-readable display name
        #[arg(long)]
        display_name: Option<String>,
    },
    /// List all projects
    List,
    /// Remove a project (index only — original files are never deleted)
    Remove {
        name: String,
    },
    /// Enable a project
    Enable {
        name: String,
    },
    /// Disable a project (keeps index, excludes from search)
    Disable {
        name: String,
    },
}

fn db_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
    let path = PathBuf::from(home).join(".doxus").join("db");
    std::fs::create_dir_all(&path).ok();
    path.join("doxus.db")
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt().with_env_filter("info").init();

    let cli = Cli::parse();
    let db_path = std::env::var("DOXUS_DB_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|_| db_path());

    let conn = db::open(&db_path).context("failed to open database")?;

    match cli.command {
        Commands::Project(args) => handle_project(&conn, args.action)?,
        Commands::Index => handle_index(&conn).await?,
        Commands::Search { query, limit, project } => {
            handle_search(&conn, query, limit, project)?
        }
        Commands::Status => handle_status(&conn)?,
        Commands::Plugin(args) => handle_plugin(&conn, args.action)?,
        Commands::Workspace(args) => handle_workspace(&conn, args.action)?,
    }

    Ok(())
}

fn handle_project(conn: &rusqlite::Connection, action: ProjectAction) -> Result<()> {
    match action {
        ProjectAction::Add { name, path, display_name } => {
            let display = display_name.unwrap_or_else(|| name.clone());
            let path_str = path.to_string_lossy();
            conn.execute(
                "INSERT INTO projects(name, display_name, path, created_at, updated_at)
                 VALUES (?1, ?2, ?3, unixepoch(), unixepoch())",
                rusqlite::params![name, display, path_str],
            )
            .context("failed to add project")?;
            println!("✅ Added project '{name}' → {path_str}");
        }
        ProjectAction::List => {
            let mut stmt = conn.prepare(
                "SELECT name, display_name, path, status FROM projects ORDER BY name",
            )?;
            let rows = stmt.query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                ))
            })?;
            println!("{:<20} {:<25} {:<10} PATH", "NAME", "DISPLAY", "STATUS");
            println!("{}", "─".repeat(80));
            for row in rows.flatten() {
                println!("{:<20} {:<25} {:<10} {}", row.0, row.1, row.3, row.2);
            }
        }
        ProjectAction::Remove { name } => {
            // remove_project = index data only, original files NEVER deleted
            let n = conn.execute(
                "DELETE FROM projects WHERE name = ?1",
                rusqlite::params![name],
            )?;
            if n > 0 {
                println!("✅ Removed project '{name}' (index only — original files untouched)");
            } else {
                println!("⚠️  Project '{name}' not found");
            }
        }
        ProjectAction::Enable { name } => {
            conn.execute(
                "UPDATE projects SET status='active', updated_at=unixepoch() WHERE name=?1",
                rusqlite::params![name],
            )?;
            println!("✅ Enabled '{name}'");
        }
        ProjectAction::Disable { name } => {
            conn.execute(
                "UPDATE projects SET status='disabled', updated_at=unixepoch() WHERE name=?1",
                rusqlite::params![name],
            )?;
            println!("✅ Disabled '{name}' (index preserved)");
        }
    }
    Ok(())
}

async fn handle_index(conn: &rusqlite::Connection) -> Result<()> {
    use doxus_plugin_sdk::{DocSource, FetchAllOpts, PluginConfig, PluginSecrets};
    use doxus_plugin_obsidian::ObsidianPlugin;

    let mut stmt = conn.prepare(
        "SELECT id, name, path FROM projects WHERE status='active'",
    )?;
    let projects: Vec<(i64, String, String)> = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
        .filter_map(|r| r.ok())
        .collect();

    if projects.is_empty() {
        println!("No active projects. Add one with: doxus project add <name> <path>");
        return Ok(());
    }

    for (pid, name, path) in projects {
        println!("📚 Indexing '{name}'...");

        let mut plugin = ObsidianPlugin::new();
        let mut config = PluginConfig::default();
        config.fields.insert("path".into(), serde_json::json!(path));
        plugin.initialize(config, PluginSecrets::default()).await?;

        let mut cursor = None;
        let mut total = 0usize;

        loop {
            let stream = plugin
                .fetch_all(FetchAllOpts { cursor, page_size: 50 })
                .await
                .map_err(|e| anyhow::anyhow!("fetch error: {e}"))?;

            for doc in &stream.documents {
                let content_hash = content_hash(&doc.content);
                conn.execute(
                    "INSERT INTO documents(project_id, source_doc_id, title, content, content_hash, file_path, last_indexed)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, unixepoch())
                     ON CONFLICT(project_id, source_doc_id) DO UPDATE SET
                       content=excluded.content,
                       content_hash=excluded.content_hash,
                       last_indexed=excluded.last_indexed",
                    rusqlite::params![
                        pid, doc.id.0, doc.title, doc.content, content_hash, doc.id.0
                    ],
                )?;

                // Insert chunk
                let doc_id: i64 = conn.query_row(
                    "SELECT id FROM documents WHERE project_id=?1 AND source_doc_id=?2",
                    rusqlite::params![pid, doc.id.0],
                    |r| r.get(0),
                )?;
                conn.execute(
                    "INSERT OR REPLACE INTO chunks(document_id, content, chunk_index)
                     VALUES (?1, ?2, 0)",
                    rusqlite::params![doc_id, doc.content],
                )?;
            }

            total += stream.documents.len();
            cursor = stream.next_cursor;
            if cursor.is_none() {
                break;
            }
        }

        println!("  ✅ Indexed {total} documents");
    }

    Ok(())
}

fn handle_search(
    conn: &rusqlite::Connection,
    query_text: String,
    limit: usize,
    project: Option<String>,
) -> Result<()> {
    use doxus_core::search::SearchEngine;

    let project_ids: Vec<i64> = if let Some(name) = project {
        let id: i64 = conn
            .query_row("SELECT id FROM projects WHERE name=?1", rusqlite::params![name], |r| r.get(0))
            .context("project not found")?;
        vec![id]
    } else {
        vec![]
    };

    let engine = SearchEngine::new(conn);
    let query = SearchQuery::new(&query_text)
        .with_projects(project_ids)
        .with_limit(limit);

    let hits = engine.search(&query)?;

    if hits.is_empty() {
        println!("No results for '{query_text}'");
        return Ok(());
    }

    println!("Found {} results for '{query_text}':\n", hits.len());
    for (i, hit) in hits.iter().enumerate() {
        let title = hit.title.as_deref().unwrap_or("(untitled)");
        let path = hit.file_path.as_deref().unwrap_or("");
        println!("{}. {} [score: {:.6}]", i + 1, title, hit.score);
        println!("   📄 {path}");
        println!("   {}", hit.snippet);
        println!();
    }

    Ok(())
}

fn handle_status(conn: &rusqlite::Connection) -> Result<()> {
    let project_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM projects", [], |r| r.get(0))?;
    let doc_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM documents", [], |r| r.get(0))?;
    let chunk_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM chunks", [], |r| r.get(0))?;

    println!("doxus status");
    println!("  Projects: {project_count}");
    println!("  Documents: {doc_count}");
    println!("  Chunks: {chunk_count}");
    Ok(())
}

// ── Plugin subcommand ──────────────────────────────────────────────────────

#[derive(Parser)]
struct PluginArgs {
    #[command(subcommand)]
    action: PluginAction,
}

#[derive(Subcommand)]
enum PluginAction {
    /// List all installed plugins
    List,
    /// Show status for a specific plugin
    Status {
        /// Plugin ID (e.g. com.doxus.confluence)
        plugin_id: String,
    },
    /// Install a plugin from a registry URL or plugin ID
    Install {
        /// Plugin ID to install (e.g. com.doxus.confluence)
        plugin_id: String,
    },
    /// Remove an installed plugin
    Remove {
        /// Plugin ID to remove
        plugin_id: String,
    },
    /// Update an installed plugin to latest version
    Update {
        /// Plugin ID to update
        plugin_id: String,
    },
}

fn handle_plugin(conn: &rusqlite::Connection, action: PluginAction) -> Result<()> {
    match action {
        PluginAction::List => {
            let mut stmt = conn.prepare(
                "SELECT plugin_id, COUNT(*) as instances FROM source_instances GROUP BY plugin_id ORDER BY plugin_id",
            )?;
            let rows = stmt.query_map([], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
            })?;
            println!("{:<40} INSTANCES", "PLUGIN_ID");
            println!("{}", "─".repeat(50));
            for row in rows.flatten() {
                println!("{:<40} {}", row.0, row.1);
            }
        }
        PluginAction::Status { plugin_id } => {
            let result = conn.query_row(
                "SELECT COUNT(*), MAX(last_synced) FROM source_instances WHERE plugin_id = ?1",
                rusqlite::params![plugin_id],
                |r| Ok((r.get::<_, i64>(0)?, r.get::<_, Option<i64>>(1)?)),
            )?;
            let last_sync = result.1.map(|t| t.to_string()).unwrap_or_else(|| "never".into());
            println!("Plugin:     {plugin_id}");
            println!("Instances:  {}", result.0);
            println!("Last sync:  {last_sync}");
        }
        PluginAction::Install { plugin_id } => {
            conn.execute(
                "INSERT OR IGNORE INTO plugins(id, name, version, kind, trust_level, manifest_json, installed_at)
                 VALUES (?1, ?1, '0.0.0', 'external', 'unverified', '{}', unixepoch())",
                rusqlite::params![plugin_id],
            )?;
            println!("Plugin '{plugin_id}' registered.");
        }
        PluginAction::Remove { plugin_id } => {
            let rows = conn.execute(
                "DELETE FROM plugins WHERE id = ?1",
                rusqlite::params![plugin_id],
            )?;
            if rows == 0 {
                println!("Plugin '{plugin_id}' not found.");
            } else {
                println!("Plugin '{plugin_id}' removed.");
            }
        }
        PluginAction::Update { plugin_id } => {
            let rows = conn.execute(
                "UPDATE plugins SET version = '0.0.1', installed_at = unixepoch() WHERE id = ?1",
                rusqlite::params![plugin_id],
            )?;
            if rows == 0 {
                println!("Plugin '{plugin_id}' not found. Install it first.");
            } else {
                println!("Plugin '{plugin_id}' updated.");
            }
        }
    }
    Ok(())
}

// ── Workspace subcommand ───────────────────────────────────────────────────

#[derive(Parser)]
struct WorkspaceArgs {
    #[command(subcommand)]
    action: WorkspaceAction,
}

#[derive(Subcommand)]
enum WorkspaceAction {
    /// List all workspaces
    List,
    /// Create a new workspace
    Create {
        /// Workspace name (unique slug)
        name: String,
        /// Optional description
        #[arg(long)]
        description: Option<String>,
    },
}

fn handle_workspace(conn: &rusqlite::Connection, action: WorkspaceAction) -> Result<()> {
    match action {
        WorkspaceAction::List => {
            let mut stmt = conn.prepare(
                "SELECT name, description FROM workspaces ORDER BY name",
            )?;
            let rows = stmt.query_map([], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?))
            })?;
            println!("{:<30} DESCRIPTION", "NAME");
            println!("{}", "─".repeat(60));
            for row in rows.flatten() {
                let desc = row.1.as_deref().unwrap_or("");
                println!("{:<30} {}", row.0, desc);
            }
        }
        WorkspaceAction::Create { name, description } => {
            conn.execute(
                "INSERT INTO workspaces(name, description, created_at, updated_at)
                 VALUES (?1, ?2, unixepoch(), unixepoch())",
                rusqlite::params![name, description],
            )
            .context("failed to create workspace")?;
            println!("Created workspace '{name}'");
        }
    }
    Ok(())
}

/// SHA-256 hash of content for deduplication.
fn content_hash(input: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(input.as_bytes());
    hex::encode(h.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn setup_test_db() -> (rusqlite::Connection, TempDir) {
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("test.db");
        let conn = doxus_core::db::open(&db_path).unwrap();
        (conn, dir)
    }

    // ── Plugin tests ──────────────────────────────────────────────────────

    #[test]
    fn test_plugin_list_empty() {
        let (conn, _dir) = setup_test_db();
        // Should succeed with no output (no rows)
        handle_plugin(&conn, PluginAction::List).unwrap();
    }

    #[test]
    fn test_plugin_status_no_instances() {
        let (conn, _dir) = setup_test_db();
        // plugin_id not in source_instances → COUNT=0, MAX=NULL
        handle_plugin(
            &conn,
            PluginAction::Status { plugin_id: "com.doxus.confluence".into() },
        )
        .unwrap();
    }

    #[test]
    fn test_plugin_list_with_data() {
        let (conn, _dir) = setup_test_db();

        // Insert a plugin row first (source_instances FK references plugins)
        conn.execute(
            "INSERT INTO plugins(id, name, version, kind, trust_level, manifest_json, installed_at)
             VALUES (?1, ?2, ?3, 'external', 'unverified', '{}', unixepoch())",
            rusqlite::params!["com.doxus.test", "Test Plugin", "1.0.0"],
        )
        .unwrap();

        // Insert a project row (source_instances FK references projects)
        conn.execute(
            "INSERT INTO projects(name, display_name, path, created_at, updated_at)
             VALUES ('p1', 'P1', '/tmp/p1', unixepoch(), unixepoch())",
            [],
        )
        .unwrap();
        let project_id: i64 =
            conn.query_row("SELECT id FROM projects WHERE name='p1'", [], |r| r.get(0)).unwrap();

        conn.execute(
            "INSERT INTO source_instances(plugin_id, project_id, name, config_json, created_at)
             VALUES (?1, ?2, 'inst1', '{}', unixepoch())",
            rusqlite::params!["com.doxus.test", project_id],
        )
        .unwrap();

        handle_plugin(&conn, PluginAction::List).unwrap();

        // Verify data is queryable
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM source_instances WHERE plugin_id='com.doxus.test'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn test_plugin_status_with_data() {
        let (conn, _dir) = setup_test_db();

        conn.execute(
            "INSERT INTO plugins(id, name, version, kind, trust_level, manifest_json, installed_at)
             VALUES ('com.doxus.conf', 'Confluence', '1.0.0', 'external', 'unverified', '{}', unixepoch())",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO projects(name, display_name, path, created_at, updated_at)
             VALUES ('proj', 'Proj', '/tmp', unixepoch(), unixepoch())",
            [],
        )
        .unwrap();
        let pid: i64 =
            conn.query_row("SELECT id FROM projects WHERE name='proj'", [], |r| r.get(0)).unwrap();
        conn.execute(
            "INSERT INTO source_instances(plugin_id, project_id, name, config_json, last_synced, created_at)
             VALUES ('com.doxus.conf', ?1, 'i1', '{}', 1700000000, unixepoch())",
            rusqlite::params![pid],
        )
        .unwrap();

        handle_plugin(
            &conn,
            PluginAction::Status { plugin_id: "com.doxus.conf".into() },
        )
        .unwrap();
    }

    // ── Workspace tests ───────────────────────────────────────────────────

    #[test]
    fn test_workspace_list_empty() {
        let (conn, _dir) = setup_test_db();
        handle_workspace(&conn, WorkspaceAction::List).unwrap();
    }

    #[test]
    fn test_workspace_create_and_list() {
        let (conn, _dir) = setup_test_db();

        handle_workspace(
            &conn,
            WorkspaceAction::Create {
                name: "my-ws".into(),
                description: Some("My workspace".into()),
            },
        )
        .unwrap();

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM workspaces WHERE name='my-ws'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1);

        handle_workspace(&conn, WorkspaceAction::List).unwrap();
    }

    #[test]
    fn test_workspace_create_no_description() {
        let (conn, _dir) = setup_test_db();

        handle_workspace(
            &conn,
            WorkspaceAction::Create { name: "bare-ws".into(), description: None },
        )
        .unwrap();

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM workspaces WHERE name='bare-ws'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1);
    }

    // ── Clap parsing tests ────────────────────────────────────────────────

    #[test]
    fn cli_parses_search_command() {
        let cli = Cli::try_parse_from(["doxus", "search", "hello"]).unwrap();
        match cli.command {
            Commands::Search { query, .. } => assert_eq!(query, "hello"),
            _ => panic!("expected Search command"),
        }
    }

    #[test]
    fn cli_parses_project_add() {
        let cli = Cli::try_parse_from([
            "doxus", "project", "add", "my-proj", "/tmp/vault",
        ])
        .unwrap();
        match cli.command {
            Commands::Project(args) => match args.action {
                ProjectAction::Add { name, path, .. } => {
                    assert_eq!(name, "my-proj");
                    assert_eq!(path, std::path::PathBuf::from("/tmp/vault"));
                }
                _ => panic!("expected Add action"),
            },
            _ => panic!("expected Project command"),
        }
    }

    #[test]
    fn cli_parses_status() {
        let cli = Cli::try_parse_from(["doxus", "status"]).unwrap();
        assert!(matches!(cli.command, Commands::Status));
    }

    #[test]
    fn plugin_action_install_parses() {
        let args = Cli::try_parse_from(["doxus", "plugin", "install", "com.test.plugin"]);
        assert!(args.is_ok());
    }

    #[test]
    fn plugin_action_remove_parses() {
        let args = Cli::try_parse_from(["doxus", "plugin", "remove", "com.test.plugin"]);
        assert!(args.is_ok());
    }

    #[test]
    fn plugin_action_update_parses() {
        let args = Cli::try_parse_from(["doxus", "plugin", "update", "com.test.plugin"]);
        assert!(args.is_ok());
    }

    #[test]
    fn test_plugin_install_and_remove() {
        let (conn, _dir) = setup_test_db();

        // Install
        handle_plugin(&conn, PluginAction::Install { plugin_id: "com.test.plugin".into() }).unwrap();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM plugins WHERE id='com.test.plugin'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1);

        // Remove
        handle_plugin(&conn, PluginAction::Remove { plugin_id: "com.test.plugin".into() }).unwrap();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM plugins WHERE id='com.test.plugin'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn test_plugin_remove_not_found() {
        let (conn, _dir) = setup_test_db();
        // Should not error even when plugin doesn't exist
        handle_plugin(&conn, PluginAction::Remove { plugin_id: "com.nonexistent".into() }).unwrap();
    }

    #[test]
    fn test_plugin_update_not_found() {
        let (conn, _dir) = setup_test_db();
        // Should not error even when plugin doesn't exist
        handle_plugin(&conn, PluginAction::Update { plugin_id: "com.nonexistent".into() }).unwrap();
    }

    #[test]
    fn test_plugin_install_idempotent() {
        let (conn, _dir) = setup_test_db();
        handle_plugin(&conn, PluginAction::Install { plugin_id: "com.test.plugin".into() }).unwrap();
        // Second install should be a no-op (INSERT OR IGNORE)
        handle_plugin(&conn, PluginAction::Install { plugin_id: "com.test.plugin".into() }).unwrap();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM plugins WHERE id='com.test.plugin'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn test_workspace_create_duplicate_fails() {
        let (conn, _dir) = setup_test_db();

        handle_workspace(
            &conn,
            WorkspaceAction::Create { name: "dup".into(), description: None },
        )
        .unwrap();

        let result = handle_workspace(
            &conn,
            WorkspaceAction::Create { name: "dup".into(), description: None },
        );
        assert!(result.is_err());
    }
}
