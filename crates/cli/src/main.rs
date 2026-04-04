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
                let content_hash = format!("{:x}", md5_simple(&doc.content));
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
        println!("{}. {} [score: {:.2}]", i + 1, title, hit.score);
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

/// Simple MD5-like hash for content deduplication (not cryptographic).
fn md5_simple(input: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    input.hash(&mut hasher);
    hasher.finish()
}
