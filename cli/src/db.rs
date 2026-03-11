use clap::Subcommand;
use colored::Colorize;
use crate::client::ApiClient;
use serde_json::Value;

#[derive(Subcommand)]
pub enum DbCommands {
    /// Create a database (schema) inside the active project
    Create {
        /// Database name (default: "default")
        #[arg(default_value = "default")]
        name: String,
    },
    /// List databases in the active project
    List,
    /// Manage tables inside a database
    Table {
        #[command(subcommand)]
        command: TableCommands,
    },
    /// Show schema diff between two environments (or local vs remote)
    Diff {
        /// First environment (default: "staging")
        #[arg(long, default_value = "staging")]
        env1: String,
        /// Second environment (default: "production")
        #[arg(long, default_value = "production")]
        env2: String,
        /// Output format: text | sql
        #[arg(long, default_value = "text")]
        format: String,
        /// Save output to a file instead of stdout
        #[arg(long, value_name = "FILE")]
        output: Option<String>,
    },
    /// Run a SQL query against the project database
    Query {
        /// SQL statement to execute
        #[arg(long)]
        sql: Option<String>,
        /// Path to a .sql file to execute
        #[arg(long, value_name = "FILE")]
        file: Option<String>,
        /// Database name (default: "default")
        #[arg(long, default_value = "default")]
        database: String,
    },
    /// Open an interactive SQL shell (psql) for the project database
    Shell {
        /// Database name (default: "default")
        #[arg(long, default_value = "default")]
        database: String,
    },
    /// Show full mutation history for a single row
    History {
        /// Table name
        table: String,
        /// Primary-key value when the pk column is named "id" (e.g. 42 or "abc")
        #[arg(long)]
        id: Option<String>,
        /// Composite/custom pk as JSON (e.g. '{"order_id":99}')
        #[arg(long)]
        pk: Option<String>,
        /// Database name (default: "default")
        #[arg(long, default_value = "default")]
        database: String,
        /// Max rows to display (default 50)
        #[arg(long, default_value = "50")]
        limit: u32,
    },
    /// Show who last modified each row in a table
    Blame {
        /// Table name
        table: String,
        /// Database name (default: "default")
        #[arg(long, default_value = "default")]
        database: String,
        /// Max rows to display (default 100)
        #[arg(long, default_value = "100")]
        limit: u32,
    },
    /// List all mutations in a time window (incident replay)
    Replay {
        /// Window start as RFC-3339 timestamp (e.g. 2026-03-09T15:00:00Z)
        #[arg(long)]
        from: String,
        /// Window end as RFC-3339 timestamp (e.g. 2026-03-09T15:05:00Z)
        #[arg(long)]
        to: String,
        /// Database name (default: "default")
        #[arg(long, default_value = "default")]
        database: String,
        /// Max rows to display (default 500)
        #[arg(long, default_value = "500")]
        limit: u32,
    },
    /// Manage SQL migrations for the project database
    Migration {
        #[command(subcommand)]
        command: MigrationCommands,
    },
}

#[derive(Subcommand)]
pub enum MigrationCommands {
    /// Scaffold a new migration file
    Create {
        /// Migration description (used in filename)
        name: String,
    },
    /// Apply pending migrations
    Apply {
        /// Database name (default: "default")
        #[arg(long, default_value = "default")]
        database: String,
        /// Apply only this many migrations (default: all pending)
        #[arg(long, value_name = "N")]
        count: Option<u32>,
    },
    /// Roll back the last applied migration
    Rollback {
        /// Database name (default: "default")
        #[arg(long, default_value = "default")]
        database: String,
    },
    /// List migrations and their applied status
    Status {
        /// Database name (default: "default")
        #[arg(long, default_value = "default")]
        database: String,
    },
}

#[derive(Subcommand)]
pub enum TableCommands {
    /// List tables in the database
    List {
        /// Database name (default: "default")
        #[arg(default_value = "default")]
        database: String,
    },
    /// Create a table (columns as JSON, e.g. '[{"name":"id","type":"uuid"},...]')
    Create {
        /// Table name
        name: String,
        /// Database name (default: "default")
        #[arg(long, default_value = "default")]
        database: String,
        /// Column definitions as JSON array
        /// e.g. '[{"name":"id","type":"uuid","primary_key":true},{"name":"email","type":"text"}]'
        #[arg(long)]
        columns: Option<String>,
    },
}

pub async fn execute(command: DbCommands) -> anyhow::Result<()> {
    let client = ApiClient::new().await?;

    match command {
        // ── flux db create [name] ─────────────────────────────────────────────
        DbCommands::Create { name } => {
            println!("Creating database \"{}\"...", name);
            let res = client.client
                .post(format!("{}/db/databases", client.base_url))
                .json(&serde_json::json!({ "name": name }))
                .send()
                .await?;
            let status = res.status();
            let json: Value = res.json().await.unwrap_or_default();
            if status.is_success() {
                println!("✓ Database \"{}\" created", name);
                let schema = json.get("schema").and_then(|v| v.as_str()).unwrap_or("");
                if !schema.is_empty() {
                    println!("  schema: {}", schema);
                }
            } else {
                let msg = json
                    .get("error").and_then(|v| v.as_str())
                    .or_else(|| json.get("message").and_then(|v| v.as_str()))
                    .unwrap_or("unknown error");
                anyhow::bail!("Failed to create database: {} — {}", status, msg);
            }
        }

        // ── flux db list ────────────────────────────────────────────────────
        DbCommands::List => {
            let res = client.client
                .get(format!("{}/db/databases", client.base_url))
                .send()
                .await?;
            let json: Value = res.error_for_status()?.json().await?;
            let databases = json
                .get("databases")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();
            if databases.is_empty() {
                println!("No databases found. Run `flux db create` to provision one.");
            } else {
                println!("{:<30}", "DATABASE");
                for db in &databases {
                    println!("{:<30}", db.as_str().unwrap_or(""));
                }
            }
        }

        // ── flux db table list/create ────────────────────────────────────────
        DbCommands::Table { command } => match command {
            TableCommands::List { database } => {
                let res = client.client
                    .get(format!("{}/db/tables/{}", client.base_url, database))
                    .send()
                    .await?;
                let json: Value = res.error_for_status()?.json().await?;
                let tables = json
                    .get("tables")
                    .and_then(|v| v.as_array())
                    .cloned()
                    .unwrap_or_default();
                if tables.is_empty() {
                    println!("No tables in database \"{}\".", database);
                } else {
                    println!("{:<30} {}", "TABLE", "COLUMNS");
                    for t in &tables {
                        let tname = t.get("name").and_then(|v| v.as_str()).unwrap_or("");
                        let cols: Vec<&str> = t.get("columns")
                            .and_then(|v| v.as_array())
                            .map(|arr| arr.iter()
                                .filter_map(|c| c.get("name").and_then(|n| n.as_str()))
                                .collect())
                            .unwrap_or_default();
                        println!("{:<30} {}", tname, cols.join(", "));
                    }
                }
            }

            TableCommands::Create { name, database, columns } => {
                // Default columns if none provided: id (uuid pk) + created_at
                let cols: Value = if let Some(raw) = columns {
                    serde_json::from_str(&raw)
                        .map_err(|e| anyhow::anyhow!("Invalid --columns JSON: {}", e))?
                } else {
                    serde_json::json!([
                        { "name": "id",         "type": "uuid",        "primary_key": true, "default": "gen_random_uuid()" },
                        { "name": "created_at", "type": "timestamptz", "default": "now()" }
                    ])
                };

                let payload = serde_json::json!({
                    "name":     name,
                    "database": database,
                    "columns":  cols,
                });

                let res = client.client
                    .post(format!("{}/db/tables", client.base_url))
                    .json(&payload)
                    .send()
                    .await?;
                let status = res.status();
                let json: Value = res.json().await.unwrap_or_default();
                if status.is_success() {
                    println!("✓ Table \"{}\" created in database \"{}\"", name, database);
                } else {
                    let msg = json
                        .get("error").and_then(|v| v.as_str())
                        .or_else(|| json.get("message").and_then(|v| v.as_str()))
                        .unwrap_or("unknown error");
                    anyhow::bail!("Failed to create table: {} — {}", status, msg);
                }
            }
        },

        // ── flux db diff ──────────────────────────────────────────────────────
        DbCommands::Diff { env1, env2, format, output } => {
            let res = client.client
                .get(format!("{}/db/diff?env1={}&env2={}&format={}", client.base_url, env1, env2, format))
                .send()
                .await?;
            let json: Value = res.error_for_status()?.json().await?;
            let diff_text = json["diff"].as_str().unwrap_or("No differences found.");
            if let Some(path) = output {
                std::fs::write(&path, diff_text)?;
                println!("{} Diff saved to {}", "✔".green().bold(), path.cyan());
            } else {
                println!("{}", diff_text);
            }
        }

        // ── flux db query ─────────────────────────────────────────────────────
        DbCommands::Query { sql, file, database } => {
            let statement = if let Some(s) = sql {
                s
            } else if let Some(f) = file {
                std::fs::read_to_string(&f)
                    .map_err(|e| anyhow::anyhow!("Cannot read SQL file {}: {}", f, e))?
            } else {
                anyhow::bail!("Provide --sql 'SELECT …' or --file query.sql");
            };

            let payload = serde_json::json!({ "sql": statement, "database": database });
            let res = client.client
                .post(format!("{}/db/query", client.base_url))
                .json(&payload)
                .send()
                .await?;
            let json: Value = res.error_for_status()?.json().await?;

            if let Some(rows) = json["rows"].as_array() {
                if rows.is_empty() {
                    println!("Query returned 0 rows.");
                } else {
                    // Print as a simple table: collect column names from first row
                    let cols: Vec<String> = rows[0].as_object()
                        .map(|o| o.keys().cloned().collect())
                        .unwrap_or_default();
                    let widths: Vec<usize> = cols.iter().map(|c| c.len().max(12)).collect();
                    let header: Vec<String> = cols.iter().zip(&widths)
                        .map(|(c, w)| format!("{:<width$}", c, width = w))
                        .collect();
                    println!("{}", header.join("  ").bold());
                    println!("{}", "─".repeat(header.join("  ").len()).dimmed());
                    for row in rows {
                        let cells: Vec<String> = cols.iter().zip(&widths).map(|(c, w)| {
                            let v = &row[c];
                            let s = match v {
                                Value::String(s) => s.clone(),
                                Value::Null => "(null)".to_string(),
                                other => other.to_string(),
                            };
                            format!("{:<width$}", s, width = w)
                        }).collect();
                        println!("{}", cells.join("  "));
                    }
                    let count = rows.len();
                    println!("\n({} row{})", count, if count == 1 { "" } else { "s" });
                }
            } else {
                let affected = json["affected_rows"].as_i64().unwrap_or(0);
                println!("{} Query OK ({} row{} affected)", "✔".green().bold(), affected, if affected == 1 { "" } else { "s" });
            }
        }

        // ── flux db shell ─────────────────────────────────────────────────────
        DbCommands::Shell { database } => {
            // Fetch connection string from API, then exec psql
            let res = client.client
                .get(format!("{}/db/connection?database={}", client.base_url, database))
                .send()
                .await?;
            let json: Value = res.error_for_status()?.json().await?;
            let conn_str = json["connection_string"].as_str()
                .ok_or_else(|| anyhow::anyhow!("API didn't return a connection_string"))?
                .to_string();

            println!("{} Connecting to database {}…", "→".cyan(), database.bold());
            let status = std::process::Command::new("psql")
                .arg(&conn_str)
                .status()
                .map_err(|e| {
                    if e.kind() == std::io::ErrorKind::NotFound {
                        anyhow::anyhow!("'psql' not found — install PostgreSQL client tools")
                    } else {
                        anyhow::anyhow!("psql error: {}", e)
                    }
                })?;
            if !status.success() {
                anyhow::bail!("psql exited with status {}", status);
            }
        }

        // ── flux db history <table> --id <pk> ─────────────────────────────────
        DbCommands::History { table, id, pk, database, limit } => {
            let mut url = format!("{}/db/history/{}/{}", client.base_url, database, table);
            let mut sep = '?';
            if let Some(ref v) = id  { url.push_str(&format!("{sep}id={v}"));  sep = '&'; }
            if let Some(ref v) = pk  { url.push_str(&format!("{sep}pk={}", urlencoding::encode(v))); sep = '&'; }
            url.push_str(&format!("{sep}limit={limit}"));

            let res = client.client.get(&url).send().await?;
            let json: Value = res.error_for_status()?.json().await?;
            let rows = json["history"].as_array().cloned().unwrap_or_default();
            if rows.is_empty() {
                println!("No mutations found for {} id={}", table, id.as_deref().or(pk.as_deref()).unwrap_or("?"));
            } else {
                println!("{:<8} {:<10} {:<20} {}",
                    "VERSION".bold(), "OP".bold(), "ACTOR".bold(), "TIME".bold());
                println!("{}", "─".repeat(65).dimmed());
                for r in &rows {
                    let ver    = r["version"].as_i64().unwrap_or(0);
                    let op     = r["operation"].as_str().unwrap_or("");
                    let actor  = r["actor_id"].as_str().unwrap_or("–");
                    let ts     = r["created_at"].as_str().unwrap_or("");
                    let op_col = match op {
                        "insert" => "insert".green().to_string(),
                        "update" => "update".yellow().to_string(),
                        "delete" => "delete".red().to_string(),
                        other    => other.to_string(),
                    };
                    println!("{:<8} {:<10} {:<20} {}", ver, op_col, actor, ts);
                }
                println!("\n({} row{})", rows.len(), if rows.len() == 1 { "" } else { "s" });
            }
        }

        // ── flux db blame <table> ─────────────────────────────────────────────
        DbCommands::Blame { table, database, limit } => {
            let url = format!("{}/db/blame/{}/{}?limit={}", client.base_url, database, table, limit);
            let res = client.client.get(&url).send().await?;
            let json: Value = res.error_for_status()?.json().await?;
            let rows = json["blame"].as_array().cloned().unwrap_or_default();
            if rows.is_empty() {
                println!("No blame data found for table '{}'.", table);
            } else {
                println!("{:<30} {:<20} {:<8} {}",
                    "ROW PK".bold(), "LAST ACTOR".bold(), "VERSION".bold(), "TIME".bold());
                println!("{}", "─".repeat(75).dimmed());
                for r in &rows {
                    let pk     = r["record_pk"].to_string();
                    let actor  = r["actor_id"].as_str().unwrap_or("–");
                    let ver    = r["version"].as_i64().unwrap_or(0);
                    let ts     = r["created_at"].as_str().unwrap_or("");
                    println!("{:<30} {:<20} {:<8} {}", pk, actor, ver, ts);
                }
                println!("\n({} row{})", rows.len(), if rows.len() == 1 { "" } else { "s" });
            }
        }

        // ── flux db replay --from … --to … ───────────────────────────────────
        DbCommands::Replay { from, to, database, limit } => {
            let url = format!(
                "{}/db/replay/{}?from={}&to={}&limit={}",
                client.base_url, database,
                urlencoding::encode(&from), urlencoding::encode(&to), limit
            );
            let res = client.client.get(&url).send().await?;
            let json: Value = res.error_for_status()?.json().await?;
            let rows = json["replay"].as_array().cloned().unwrap_or_default();
            if rows.is_empty() {
                println!("No mutations found between {} and {}.", from, to);
            } else {
                println!("{:<25} {:<10} {:<8} {:<20} {}",
                    "TIME".bold(), "TABLE".bold(), "OP".bold(), "ACTOR".bold(), "PK".bold());
                println!("{}", "─".repeat(85).dimmed());
                for r in &rows {
                    let ts     = r["created_at"].as_str().unwrap_or("");
                    let tbl    = r["table_name"].as_str().unwrap_or("");
                    let op     = r["operation"].as_str().unwrap_or("");
                    let actor  = r["actor_id"].as_str().unwrap_or("–");
                    let pk     = r["record_pk"].to_string();
                    let op_col = match op {
                        "insert" => "insert".green().to_string(),
                        "update" => "update".yellow().to_string(),
                        "delete" => "delete".red().to_string(),
                        other    => other.to_string(),
                    };
                    println!("{:<25} {:<10} {:<8} {:<20} {}", ts, tbl, op_col, actor, pk);
                }
                println!("\n({} event{})", rows.len(), if rows.len() == 1 { "" } else { "s" });
            }
        }

        // ── flux db migration … ───────────────────────────────────────────────
        DbCommands::Migration { command } => match command {
            MigrationCommands::Create { name } => {
                let ts = chrono::Utc::now().format("%Y%m%d%H%M%S");
                let filename = format!("migrations/{}_{}.sql", ts, name.replace(' ', "_"));
                std::fs::create_dir_all("migrations")?;
                std::fs::write(&filename, format!("-- Migration: {}\n-- Created: {}\n\n", name, ts))?;
                println!("{} Created {}", "✔".green().bold(), filename.cyan());
            }

            MigrationCommands::Apply { database, count } => {
                let mut body = serde_json::json!({ "database": database });
                if let Some(n) = count {
                    body["count"] = serde_json::json!(n);
                }
                let res = client.client
                    .post(format!("{}/db/migrations/apply", client.base_url))
                    .json(&body)
                    .send()
                    .await?;
                let json: Value = res.error_for_status()?.json().await?;
                let applied = json["applied"].as_array().cloned().unwrap_or_default();
                if applied.is_empty() {
                    println!("Nothing to apply — database is up to date.");
                } else {
                    for m in &applied {
                        println!("  {} {}", "↑".green(), m.as_str().unwrap_or(""));
                    }
                    println!("{} Applied {} migration{}", "✔".green().bold(), applied.len(), if applied.len() == 1 { "" } else { "s" });
                }
            }

            MigrationCommands::Rollback { database } => {
                let res = client.client
                    .post(format!("{}/db/migrations/rollback", client.base_url))
                    .json(&serde_json::json!({ "database": database }))
                    .send()
                    .await?;
                let json: Value = res.error_for_status()?.json().await?;
                let name = json["rolled_back"].as_str().unwrap_or("(unknown)");
                println!("{} Rolled back: {}", "✔".green().bold(), name.yellow());
            }

            MigrationCommands::Status { database } => {
                let res = client.client
                    .get(format!("{}/db/migrations?database={}", client.base_url, database))
                    .send()
                    .await?;
                let json: Value = res.error_for_status()?.json().await?;
                let migrations = json["migrations"].as_array().cloned().unwrap_or_default();
                println!("{:<50} {}", "MIGRATION".bold(), "STATUS".bold());
                println!("{}", "─".repeat(60).dimmed());
                for m in &migrations {
                    let fname = m["name"].as_str().unwrap_or("");
                    let applied = m["applied"].as_bool().unwrap_or(false);
                    let status_col = if applied { "applied".green() } else { "pending".yellow() };
                    println!("{:<50} {}", fname, status_col);
                }
            }
        },
    }

    Ok(())
}
