use clap::Subcommand;
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
    }

    Ok(())
}
