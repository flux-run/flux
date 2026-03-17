use std::sync::{Once, OnceLock};

use anyhow::{Context, Result};
use rcgen::generate_simple_self_signed;
use runtime::JsIsolate;
use runtime::deno_runtime::{ExecutionMode, FetchCheckpoint};
use rustls::ServerConfig;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use runtime::isolate_pool::ExecutionContext;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::{Mutex, oneshot};
use tokio_rustls::TlsAcceptor;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn postgres_simple_query_replays_recorded_rows() -> Result<()> {
    let _lock = postgres_test_lock().lock().await;
    let _guard = EnvVarGuard::set("FLOWBASE_ALLOW_LOOPBACK_POSTGRES", "1");
    let (port, shutdown_tx, server_task) = spawn_mock_postgres_server().await?;

    let code = r#"
export default function handler({ input }) {
  const result = Flux.postgres.simpleQuery({
    connectionString: input.connectionString,
    sql: input.sql,
  });

  return {
    rows: result.rows,
    command: result.command,
    replay: result.replay,
  };
}
"#;

    let payload = serde_json::json!({
        "connectionString": format!("postgres://127.0.0.1:{port}/flux_test"),
        "sql": "select 1 as value",
    });

    let mut isolate = JsIsolate::new_for_run(code).context("failed to create postgres isolate")?;
    let live_output = isolate
        .execute(payload.clone(), ExecutionContext::new("postgres-live"))
        .await
        .context("live postgres execution failed")?;

    shutdown_tx.send(()).ok();
    server_task.await.context("mock postgres server task failed")??;

    assert_eq!(live_output.error, None);
    assert_eq!(
        live_output.output,
        serde_json::json!({
            "rows": [{ "value": "1" }],
            "command": "SELECT 1",
            "replay": false,
        })
    );
    assert_eq!(live_output.checkpoints.len(), 1);
    assert_eq!(live_output.checkpoints[0].boundary, "postgres");
    assert_eq!(live_output.checkpoints[0].method, "simple_query");

    let recorded = live_output.checkpoints.clone();
    let mut replay_isolate = JsIsolate::new_for_run(code).context("failed to create postgres replay isolate")?;
    let mut replay_context = ExecutionContext::new("postgres-replay");
    replay_context.mode = ExecutionMode::Replay;
    let replay_output = replay_isolate
        .execute_with_recorded(payload, replay_context, recorded)
        .await
        .context("replay postgres execution failed")?;

    assert_eq!(replay_output.error, None);
    assert_eq!(
        replay_output.output,
        serde_json::json!({
            "rows": [{ "value": "1" }],
            "command": "SELECT 1",
            "replay": true,
        })
    );
    assert_eq!(replay_output.checkpoints.len(), 1);
    assert_eq!(replay_output.checkpoints[0].boundary, "postgres");

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn postgres_simple_query_blocks_loopback_by_default_and_never_replays_it() -> Result<()> {
    let _lock = postgres_test_lock().lock().await;
    let code = r#"
export default function handler({ input }) {
  try {
    const result = Flux.postgres.simpleQuery({
      connectionString: input.connectionString,
      sql: input.sql,
    });
    return { ok: true, rows: result.rows };
  } catch (err) {
    return {
      ok: false,
      message: err?.message ?? null,
      string: String(err),
    };
  }
}
"#;

    let payload = serde_json::json!({
        "connectionString": "postgres://127.0.0.1:5432/flux_test",
        "sql": "select 1 as value",
    });

    let mut live_isolate = JsIsolate::new_for_run(code).context("failed to create blocked postgres isolate")?;
    let live_output = live_isolate
        .execute(payload.clone(), ExecutionContext::new("postgres-blocked-live"))
        .await
        .context("blocked postgres execution failed")?;

    assert_eq!(live_output.error, None);
    assert_eq!(live_output.output.get("ok"), Some(&serde_json::json!(false)));
    let live_message = live_output.output.get("message").and_then(|value| value.as_str()).unwrap_or("");
    let live_string = live_output.output.get("string").and_then(|value| value.as_str()).unwrap_or("");
    assert!(
        live_message.contains("postgres connect blocked")
            || live_string.contains("postgres connect blocked")
            || live_message.contains("private/loopback")
            || live_string.contains("private/loopback")
    );
    assert!(live_output.checkpoints.is_empty());

    let fake_recording = vec![FetchCheckpoint {
        call_index: 0,
        boundary: "postgres".to_string(),
        url: "postgres://127.0.0.1:5432/flux_test".to_string(),
        method: "simple_query".to_string(),
        request: serde_json::json!({
            "url": "postgres://127.0.0.1:5432/flux_test",
            "host": "127.0.0.1",
            "port": 5432,
            "sql": "select 1 as value",
        }),
        response: serde_json::json!({
            "rows": [{ "value": "1" }],
            "command": "SELECT 1",
            "row_count": 1,
            "replay": true,
        }),
        duration_ms: 0,
    }];

    let mut replay_isolate = JsIsolate::new_for_run(code).context("failed to create blocked postgres replay isolate")?;
    let mut replay_context = ExecutionContext::new("postgres-blocked-replay");
    replay_context.mode = ExecutionMode::Replay;
    let replay_output = replay_isolate
        .execute_with_recorded(payload, replay_context, fake_recording)
        .await
        .context("blocked postgres replay execution failed")?;

    assert_eq!(replay_output.error, None);
    assert_eq!(replay_output.output.get("ok"), Some(&serde_json::json!(false)));
    assert!(replay_output.checkpoints.is_empty());

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn postgres_query_supports_params_and_replay() -> Result<()> {
    let _lock = postgres_test_lock().lock().await;
    let _guard = EnvVarGuard::set("FLOWBASE_ALLOW_LOOPBACK_POSTGRES", "1");
    let (port, shutdown_tx, server_task) = spawn_mock_postgres_param_server().await?;

    let code = r#"
export default function handler({ input }) {
  const result = Flux.postgres.query({
    connectionString: input.connectionString,
    sql: input.sql,
    params: input.params,
  });

  return {
    rows: result.rows,
    command: result.command,
    replay: result.replay,
  };
}
"#;

    let payload = serde_json::json!({
        "connectionString": format!("postgres://127.0.0.1:{port}/flux_test"),
        "sql": "select $1::text as value",
        "params": ["hello"],
    });

    let mut isolate = JsIsolate::new_for_run(code).context("failed to create postgres param isolate")?;
    let live_output = isolate
        .execute(payload.clone(), ExecutionContext::new("postgres-param-live"))
        .await
        .context("live postgres param execution failed")?;

    shutdown_tx.send(()).ok();
    server_task.await.context("mock postgres param server task failed")??;

    assert_eq!(live_output.error, None);
    assert_eq!(
        live_output.output,
        serde_json::json!({
            "rows": [{ "value": "hello" }],
            "command": "QUERY",
            "replay": false,
        })
    );
    assert_eq!(live_output.checkpoints.len(), 1);
    assert_eq!(live_output.checkpoints[0].boundary, "postgres");
    assert_eq!(live_output.checkpoints[0].method, "query");

    let recorded = live_output.checkpoints.clone();
    let mut replay_isolate = JsIsolate::new_for_run(code).context("failed to create postgres param replay isolate")?;
    let mut replay_context = ExecutionContext::new("postgres-param-replay");
    replay_context.mode = ExecutionMode::Replay;
    let replay_output = replay_isolate
        .execute_with_recorded(payload, replay_context, recorded)
        .await
        .context("replay postgres param execution failed")?;

    assert_eq!(replay_output.error, None);
    assert_eq!(
        replay_output.output,
        serde_json::json!({
            "rows": [{ "value": "hello" }],
            "command": "QUERY",
            "replay": true,
        })
    );
    assert_eq!(replay_output.checkpoints.len(), 1);
    assert_eq!(replay_output.checkpoints[0].boundary, "postgres");

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn postgres_query_preserves_native_scalar_params_and_bool_null_results() -> Result<()> {
    let _lock = postgres_test_lock().lock().await;
    let _guard = EnvVarGuard::set("FLOWBASE_ALLOW_LOOPBACK_POSTGRES", "1");
    let (port, shutdown_tx, server_task) = spawn_mock_postgres_mixed_types_server().await?;

    let code = r#"
export default function handler({ input }) {
  const result = Flux.postgres.query({
    connectionString: input.connectionString,
    sql: input.sql,
    params: input.params,
  });

  return {
    rows: result.rows,
    command: result.command,
    replay: result.replay,
  };
}
"#;

    let payload = serde_json::json!({
        "connectionString": format!("postgres://127.0.0.1:{port}/flux_test"),
        "sql": "select ($1::int8 is not null) as has_n, $2::boolean as flag, ($3::float8 is not null) as has_ratio, $4::text as note, null::int8 as empty",
        "params": [42, true, 3.5, "hello"],
    });

    let mut isolate = JsIsolate::new_for_run(code).context("failed to create postgres mixed-type isolate")?;
    let live_output = isolate
        .execute(payload.clone(), ExecutionContext::new("postgres-mixed-types-live"))
        .await
        .context("live postgres mixed-type execution failed")?;

    shutdown_tx.send(()).ok();
    server_task.await.context("mock postgres mixed-type server task failed")??;

    assert_eq!(live_output.error, None);
    assert_eq!(
        live_output.output,
        serde_json::json!({
            "rows": [{ "has_n": true, "flag": true, "has_ratio": true, "note": "hello", "empty": null }],
            "command": "QUERY",
            "replay": false,
        })
    );

    let recorded = live_output.checkpoints.clone();
    let mut replay_isolate = JsIsolate::new_for_run(code).context("failed to create postgres mixed-type replay isolate")?;
    let mut replay_context = ExecutionContext::new("postgres-mixed-types-replay");
    replay_context.mode = ExecutionMode::Replay;
    let replay_output = replay_isolate
        .execute_with_recorded(payload, replay_context, recorded)
        .await
        .context("replay postgres mixed-type execution failed")?;

    assert_eq!(replay_output.error, None);
    assert_eq!(replay_output.output.get("rows"), live_output.output.get("rows"));
    assert_eq!(replay_output.output.get("replay"), Some(&serde_json::json!(true)));

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn postgres_simple_query_supports_tls_with_custom_ca_and_replay() -> Result<()> {
    let _lock = postgres_test_lock().lock().await;
    let _guard = EnvVarGuard::set("FLOWBASE_ALLOW_LOOPBACK_POSTGRES", "1");
    ensure_rustls_provider();

    let cert = generate_simple_self_signed(vec!["localhost".to_string()])
        .context("failed to generate postgres TLS certificate")?;
    let cert_pem = cert.cert.pem();
    let cert_der: CertificateDer<'static> = cert.cert.der().clone();
    let key_der = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(cert.key_pair.serialize_der()));

    let server_config = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(vec![cert_der], key_der)
        .context("failed to build postgres TLS server config")?;

    let acceptor = TlsAcceptor::from(std::sync::Arc::new(server_config));
    let (port, shutdown_tx, server_task) = spawn_mock_postgres_tls_server(acceptor).await?;

    let code = r#"
export default function handler({ input }) {
  const result = Flux.postgres.simpleQuery({
    connectionString: input.connectionString,
    sql: input.sql,
    tls: true,
    caCertPem: input.caCertPem,
  });

  return {
    rows: result.rows,
    command: result.command,
    replay: result.replay,
  };
}
"#;

    let payload = serde_json::json!({
        "connectionString": format!("postgres://localhost:{port}/flux_test"),
        "sql": "select 1 as value",
        "caCertPem": cert_pem,
    });

    let mut isolate = JsIsolate::new_for_run(code).context("failed to create postgres tls isolate")?;
    let live_output = isolate
        .execute(payload.clone(), ExecutionContext::new("postgres-tls-live"))
        .await
        .context("live postgres TLS execution failed")?;

    shutdown_tx.send(()).ok();
    server_task.await.context("mock postgres tls server task failed")??;

    assert_eq!(live_output.error, None);
    assert_eq!(
        live_output.output,
        serde_json::json!({
            "rows": [{ "value": "1" }],
            "command": "SELECT 1",
            "replay": false,
        })
    );
    assert_eq!(live_output.checkpoints.len(), 1);
    assert_eq!(live_output.checkpoints[0].boundary, "postgres");
    assert_eq!(live_output.checkpoints[0].method, "simple_query");
    assert_eq!(live_output.checkpoints[0].request.get("tls"), Some(&serde_json::json!(true)));

    let recorded = live_output.checkpoints.clone();
    let mut replay_isolate = JsIsolate::new_for_run(code).context("failed to create postgres tls replay isolate")?;
    let mut replay_context = ExecutionContext::new("postgres-tls-replay");
    replay_context.mode = ExecutionMode::Replay;
    let replay_output = replay_isolate
        .execute_with_recorded(payload, replay_context, recorded)
        .await
        .context("replay postgres TLS execution failed")?;

    assert_eq!(replay_output.error, None);
    assert_eq!(
        replay_output.output,
        serde_json::json!({
            "rows": [{ "value": "1" }],
            "command": "SELECT 1",
            "replay": true,
        })
    );
    assert_eq!(replay_output.checkpoints.len(), 1);
    assert_eq!(replay_output.checkpoints[0].boundary, "postgres");

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn postgres_node_pg_pool_supports_drizzle_query_shape() -> Result<()> {
    let _lock = postgres_test_lock().lock().await;
    let _guard = EnvVarGuard::set("FLOWBASE_ALLOW_LOOPBACK_POSTGRES", "1");
    let (port, shutdown_tx, server_task) = spawn_mock_postgres_transaction_server().await?;

    let code = r#"
export default async function handler({ input }) {
    const pool = Flux.postgres.createNodePgPool({
        connectionString: input.connectionString,
    });
    const client = await pool.connect();

    await client.query("begin");
    const result = await client.query(
        {
            text: input.sql,
            rowMode: "array",
            values: input.params,
        },
    );
    await client.query("commit");
    await client.release();
    await pool.end();

    return {
        isPool: pool instanceof Flux.postgres.NodePgPool,
        isClient: client instanceof Flux.postgres.NodePgClient,
        rows: result.rows,
        rowCount: result.rowCount,
        command: result.command,
        fieldNames: result.fields.map((field) => field.name),
        builtins: {
            date: Flux.postgres.nodePgTypes.builtins.DATE,
            timestampTz: Flux.postgres.nodePgTypes.builtins.TIMESTAMPTZ,
        },
    };
}
"#;

    let payload = serde_json::json!({
        "connectionString": format!("postgres://127.0.0.1:{port}/flux_test"),
        "sql": "select $1::text as value",
        "params": ["hello"],
    });

    let mut isolate = JsIsolate::new_for_run(code).context("failed to create node-pg shim isolate")?;
    let live_output = isolate
        .execute(payload.clone(), ExecutionContext::new("postgres-node-pg-shim-live"))
        .await
        .context("node-pg shim execution failed")?;

    shutdown_tx.send(()).ok();
    server_task.await.context("mock postgres transaction server task failed")??;

    assert_eq!(live_output.error, None);
    assert_eq!(
        live_output.output,
        serde_json::json!({
            "isPool": true,
            "isClient": true,
            "rows": [["hello"]],
            "rowCount": 1,
            "command": "QUERY",
            "fieldNames": ["value"],
            "builtins": {
                "date": 1082,
                "timestampTz": 1184,
            },
        })
    );
    assert_eq!(live_output.checkpoints.len(), 3);
    assert!(live_output.checkpoints.iter().all(|cp| cp.boundary == "postgres"));
    assert!(live_output.checkpoints.iter().all(|cp| cp.method == "query"));

    let recorded = live_output.checkpoints.clone();
    let mut replay_isolate = JsIsolate::new_for_run(code).context("failed to create node-pg shim replay isolate")?;
    let mut replay_context = ExecutionContext::new("postgres-node-pg-shim-replay");
    replay_context.mode = ExecutionMode::Replay;
    let replay_output = replay_isolate
        .execute_with_recorded(payload, replay_context, recorded)
        .await
        .context("node-pg shim replay execution failed")?;

    assert_eq!(replay_output.error, None);
    assert_eq!(replay_output.output, live_output.output);
    assert_eq!(replay_output.checkpoints.len(), 3);

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn postgres_node_pg_pool_applies_numeric_type_parser_with_field_oids() -> Result<()> {
    let _lock = postgres_test_lock().lock().await;
    let _guard = EnvVarGuard::set("FLOWBASE_ALLOW_LOOPBACK_POSTGRES", "1");
    let (port, shutdown_tx, server_task) = spawn_mock_postgres_numeric_result_server().await?;

    let code = r#"
export default async function handler({ input }) {
    const types = Flux.postgres.nodePgTypes;
    types.setTypeParser(types.builtins.NUMERIC, (value) => ({
        raw: value,
        scale: value.includes(".") ? value.split(".")[1].length : 0,
    }));

    const pool = Flux.postgres.createNodePgPool({
        connectionString: input.connectionString,
    });
    const result = await pool.query(input.sql);
    await pool.end();

    return {
        rows: result.rows,
        fields: result.fields,
        builtinNumeric: types.builtins.NUMERIC,
        parserPreview: types.getTypeParser(types.builtins.NUMERIC)("7.500"),
    };
}
"#;

    let payload = serde_json::json!({
        "connectionString": format!("postgres://127.0.0.1:{port}/flux_test"),
        "sql": "select 12.3400::numeric as amount",
    });

    let mut isolate = JsIsolate::new_for_run(code).context("failed to create node-pg numeric parser isolate")?;
    let live_output = isolate
        .execute(payload.clone(), ExecutionContext::new("postgres-node-pg-numeric-live"))
        .await
        .context("node-pg numeric parser execution failed")?;

    shutdown_tx.send(()).ok();
    server_task.await.context("mock postgres numeric server task failed")??;

    assert_eq!(live_output.error, None);
    assert_eq!(
        live_output.output,
        serde_json::json!({
            "rows": [{ "amount": { "raw": "12.3400", "scale": 4 } }],
            "fields": [{ "name": "amount", "dataTypeID": 1700, "format": "text" }],
            "builtinNumeric": 1700,
            "parserPreview": { "raw": "7.500", "scale": 3 },
        })
    );

    let recorded = live_output.checkpoints.clone();
    let mut replay_isolate = JsIsolate::new_for_run(code).context("failed to create node-pg numeric parser replay isolate")?;
    let mut replay_context = ExecutionContext::new("postgres-node-pg-numeric-replay");
    replay_context.mode = ExecutionMode::Replay;
    let replay_output = replay_isolate
        .execute_with_recorded(payload, replay_context, recorded)
        .await
        .context("node-pg numeric parser replay failed")?;

    assert_eq!(replay_output.error, None);
    assert_eq!(replay_output.output, live_output.output);

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn postgres_node_pg_pool_applies_json_and_array_type_parsers() -> Result<()> {
    let _lock = postgres_test_lock().lock().await;
    let _guard = EnvVarGuard::set("FLOWBASE_ALLOW_LOOPBACK_POSTGRES", "1");
    let (port, shutdown_tx, server_task) = spawn_mock_postgres_json_array_result_server().await?;

    let code = r#"
export default async function handler({ input }) {
    const types = Flux.postgres.nodePgTypes;
    types.setTypeParser(types.builtins.JSONB, (value) => {
        const payload = typeof value === "string" ? JSON.parse(value) : value;
        return {
            ok: !!payload.ok,
            label: String(payload.label ?? "").toUpperCase(),
        };
    });
    types.setTypeParser(types.builtins.INT8_ARRAY, (value) => {
        const values = Array.isArray(value) ? value : String(value).slice(1, -1).split(",");
        return values.map((item) => Number(item) + 1);
    });
    types.setTypeParser(types.builtins.TEXT_ARRAY, (value) => {
        const values = Array.isArray(value) ? value : String(value).slice(1, -1).split(",");
        return values.join("|");
    });

    const pool = Flux.postgres.createNodePgPool({
        connectionString: input.connectionString,
    });
    const result = await pool.query(input.sql);
    await pool.end();

    return {
        rows: result.rows,
        fields: result.fields,
    };
}
"#;

    let payload = serde_json::json!({
        "connectionString": format!("postgres://127.0.0.1:{port}/flux_test"),
        "sql": "select '{\"ok\":true,\"label\":\"json\"}'::jsonb as payload, '{1,2,3}'::int8[] as ids, '{\"alpha\",\"beta\"}'::text[] as tags",
    });

    let mut isolate = JsIsolate::new_for_run(code).context("failed to create node-pg json-array parser isolate")?;
    let live_output = isolate
        .execute(payload.clone(), ExecutionContext::new("postgres-node-pg-json-array-live"))
        .await
        .context("node-pg json-array parser execution failed")?;

    shutdown_tx.send(()).ok();
    server_task.await.context("mock postgres json-array server task failed")??;

    assert_eq!(live_output.error, None);
    assert_eq!(
        live_output.output,
        serde_json::json!({
            "rows": [{
                "payload": { "ok": true, "label": "JSON" },
                "ids": [2, 3, 4],
                "tags": "alpha|beta",
            }],
            "fields": [
                { "name": "payload", "dataTypeID": 3802, "format": "text" },
                { "name": "ids", "dataTypeID": 1016, "format": "text" },
                { "name": "tags", "dataTypeID": 1009, "format": "text" },
            ],
        })
    );

    let recorded = live_output.checkpoints.clone();
    let mut replay_isolate = JsIsolate::new_for_run(code).context("failed to create node-pg json-array replay isolate")?;
    let mut replay_context = ExecutionContext::new("postgres-node-pg-json-array-replay");
    replay_context.mode = ExecutionMode::Replay;
    let replay_output = replay_isolate
        .execute_with_recorded(payload, replay_context, recorded)
        .await
        .context("node-pg json-array parser replay failed")?;

    assert_eq!(replay_output.error, None);
    assert_eq!(replay_output.output, live_output.output);

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn postgres_node_pg_pool_applies_temporal_and_uuid_type_parsers() -> Result<()> {
    let _lock = postgres_test_lock().lock().await;
    let _guard = EnvVarGuard::set("FLOWBASE_ALLOW_LOOPBACK_POSTGRES", "1");
    let (port, shutdown_tx, server_task) = spawn_mock_postgres_temporal_result_server().await?;

    let code = r#"
export default async function handler({ input }) {
    const types = Flux.postgres.nodePgTypes;
    types.setTypeParser(types.builtins.DATE, (value) => `date:${value}`);
    types.setTypeParser(types.builtins.TIME, (value) => ({ time: value }));
    types.setTypeParser(types.builtins.TIMETZ, (value) => value.replace("+00", "Z"));
    types.setTypeParser(types.builtins.TIMESTAMP, (value) => value.replace(" ", "T"));
    types.setTypeParser(types.builtins.TIMESTAMPTZ, (value) => ({ utc: value.endsWith("+00") }));
    types.setTypeParser(types.builtins.INTERVAL, (value) => value.split(" ")[0]);
    types.setTypeParser(types.builtins.UUID, (value) => value.toUpperCase());

    const pool = Flux.postgres.createNodePgPool({
        connectionString: input.connectionString,
    });
    const result = await pool.query(input.sql);
    await pool.end();

    return {
        rows: result.rows,
        fields: result.fields,
        builtins: {
            date: types.builtins.DATE,
            time: types.builtins.TIME,
            timetz: types.builtins.TIMETZ,
            uuid: types.builtins.UUID,
        },
    };
}
"#;

    let payload = serde_json::json!({
        "connectionString": format!("postgres://127.0.0.1:{port}/flux_test"),
        "sql": "select '2026-03-17'::date as created_on, '12:34:56'::time as starts_at, '12:34:56+00'::timetz as starts_at_tz, '2026-03-17 12:34:56'::timestamp as created_at, '2026-03-17 12:34:56+00'::timestamptz as created_at_utc, '2 days 03:04:05'::interval as elapsed, '123e4567-e89b-12d3-a456-426614174000'::uuid as id",
    });

    let mut isolate = JsIsolate::new_for_run(code).context("failed to create node-pg temporal parser isolate")?;
    let live_output = isolate
        .execute(payload.clone(), ExecutionContext::new("postgres-node-pg-temporal-live"))
        .await
        .context("node-pg temporal parser execution failed")?;

    shutdown_tx.send(()).ok();
    server_task.await.context("mock postgres temporal server task failed")??;

    assert_eq!(live_output.error, None);
    assert_eq!(
        live_output.output,
        serde_json::json!({
            "rows": [{
                "created_on": "date:2026-03-17",
                "starts_at": { "time": "12:34:56" },
                "starts_at_tz": "12:34:56Z",
                "created_at": "2026-03-17T12:34:56",
                "created_at_utc": { "utc": true },
                "elapsed": "2",
                "id": "123E4567-E89B-12D3-A456-426614174000",
            }],
            "fields": [
                { "name": "created_on", "dataTypeID": 1082, "format": "text" },
                { "name": "starts_at", "dataTypeID": 1083, "format": "text" },
                { "name": "starts_at_tz", "dataTypeID": 1266, "format": "text" },
                { "name": "created_at", "dataTypeID": 1114, "format": "text" },
                { "name": "created_at_utc", "dataTypeID": 1184, "format": "text" },
                { "name": "elapsed", "dataTypeID": 1186, "format": "text" },
                { "name": "id", "dataTypeID": 2950, "format": "text" },
            ],
            "builtins": {
                "date": 1082,
                "time": 1083,
                "timetz": 1266,
                "uuid": 2950,
            },
        })
    );

    let recorded = live_output.checkpoints.clone();
    let mut replay_isolate = JsIsolate::new_for_run(code).context("failed to create node-pg temporal replay isolate")?;
    let mut replay_context = ExecutionContext::new("postgres-node-pg-temporal-replay");
    replay_context.mode = ExecutionMode::Replay;
    let replay_output = replay_isolate
        .execute_with_recorded(payload, replay_context, recorded)
        .await
        .context("node-pg temporal parser replay failed")?;

    assert_eq!(replay_output.error, None);
    assert_eq!(replay_output.output, live_output.output);

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn postgres_node_pg_pool_applies_bytea_and_oid_type_parsers() -> Result<()> {
    let _lock = postgres_test_lock().lock().await;
    let _guard = EnvVarGuard::set("FLOWBASE_ALLOW_LOOPBACK_POSTGRES", "1");
    let (port, shutdown_tx, server_task) = spawn_mock_postgres_bytea_oid_result_server().await?;

    let code = r#"
export default async function handler({ input }) {
    const types = Flux.postgres.nodePgTypes;
    types.setTypeParser(types.builtins.BYTEA, (value) => {
        const hex = String(value).replace(/^\\x/, "");
        return hex.match(/../g).map((pair) => parseInt(pair, 16));
    });
    types.setTypeParser(types.builtins.OID, (value) => Number(value) + 1);

    const pool = Flux.postgres.createNodePgPool({
        connectionString: input.connectionString,
    });
    const result = await pool.query(input.sql);
    await pool.end();

    return {
        rows: result.rows,
        fields: result.fields,
        builtins: {
            bytea: types.builtins.BYTEA,
            oid: types.builtins.OID,
        },
    };
}
"#;

    let payload = serde_json::json!({
        "connectionString": format!("postgres://127.0.0.1:{port}/flux_test"),
        "sql": "select '\\x6869'::bytea as payload, 42::oid as object_id",
    });

    let mut isolate = JsIsolate::new_for_run(code).context("failed to create node-pg bytea/oid parser isolate")?;
    let live_output = isolate
        .execute(payload.clone(), ExecutionContext::new("postgres-node-pg-bytea-oid-live"))
        .await
        .context("node-pg bytea/oid parser execution failed")?;

    shutdown_tx.send(()).ok();
    server_task.await.context("mock postgres bytea/oid server task failed")??;

    assert_eq!(live_output.error, None);
    assert_eq!(
        live_output.output,
        serde_json::json!({
            "rows": [{
                "payload": [104, 105],
                "object_id": 43,
            }],
            "fields": [
                { "name": "payload", "dataTypeID": 17, "format": "text" },
                { "name": "object_id", "dataTypeID": 26, "format": "text" },
            ],
            "builtins": {
                "bytea": 17,
                "oid": 26,
            },
        })
    );

    let recorded = live_output.checkpoints.clone();
    let mut replay_isolate = JsIsolate::new_for_run(code).context("failed to create node-pg bytea/oid replay isolate")?;
    let mut replay_context = ExecutionContext::new("postgres-node-pg-bytea-oid-replay");
    replay_context.mode = ExecutionMode::Replay;
    let replay_output = replay_isolate
        .execute_with_recorded(payload, replay_context, recorded)
        .await
        .context("node-pg bytea/oid parser replay failed")?;

    assert_eq!(replay_output.error, None);
    assert_eq!(replay_output.output, live_output.output);

    Ok(())
}

async fn spawn_mock_postgres_server() -> Result<(u16, oneshot::Sender<()>, tokio::task::JoinHandle<Result<()>>)> {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .context("failed to bind mock postgres listener")?;
    let port = listener
        .local_addr()
        .context("failed to get mock postgres addr")?
        .port();
    let (shutdown_tx, mut shutdown_rx) = oneshot::channel();

    let task = tokio::spawn(async move {
        tokio::select! {
            _ = &mut shutdown_rx => Ok(()),
            accepted = listener.accept() => {
                let (mut socket, _) = accepted.context("failed to accept postgres client")?;

                let _startup = read_startup_message(&mut socket).await?;
                write_authentication_ok(&mut socket).await?;
                write_parameter_status(&mut socket, b"client_encoding", b"UTF8").await?;
                write_parameter_status(&mut socket, b"server_version", b"16.0").await?;
                write_backend_key_data(&mut socket).await?;
                write_ready_for_query(&mut socket).await?;

                let query = read_typed_message(&mut socket).await?;
                if query.tag != b'Q' {
                    anyhow::bail!("expected Query message, got {:?}", query.tag as char);
                }
                let sql = String::from_utf8(query.payload[..query.payload.len().saturating_sub(1)].to_vec())
                    .context("invalid query payload")?;
                if sql != "select 1 as value" {
                    anyhow::bail!("unexpected SQL: {sql}");
                }

                write_row_description(&mut socket, b"value").await?;
                write_data_row(&mut socket, &[b"1"] ).await?;
                write_command_complete(&mut socket, b"SELECT 1").await?;
                write_ready_for_query(&mut socket).await?;

                let terminate = read_typed_message(&mut socket).await?;
                if terminate.tag != b'X' {
                    anyhow::bail!("expected Terminate message, got {:?}", terminate.tag as char);
                }
                Ok(())
            }
        }
    });

    Ok((port, shutdown_tx, task))
}

async fn spawn_mock_postgres_param_server() -> Result<(u16, oneshot::Sender<()>, tokio::task::JoinHandle<Result<()>>)> {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .context("failed to bind mock postgres param listener")?;
    let port = listener
        .local_addr()
        .context("failed to get mock postgres param addr")?
        .port();
    let (shutdown_tx, mut shutdown_rx) = oneshot::channel();

    let task = tokio::spawn(async move {
        tokio::select! {
            _ = &mut shutdown_rx => Ok(()),
            accepted = listener.accept() => {
                let (mut socket, _) = accepted.context("failed to accept postgres param client")?;

                let _startup = read_startup_message(&mut socket).await?;
                write_authentication_ok(&mut socket).await?;
                write_parameter_status(&mut socket, b"client_encoding", b"UTF8").await?;
                write_parameter_status(&mut socket, b"server_version", b"16.0").await?;
                write_backend_key_data(&mut socket).await?;
                write_ready_for_query(&mut socket).await?;

                let parse = read_typed_message(&mut socket).await?;
                if parse.tag != b'P' {
                    anyhow::bail!("expected Parse message, got {:?}", parse.tag as char);
                }
                let describe = read_typed_message(&mut socket).await?;
                if describe.tag != b'D' {
                    anyhow::bail!("expected Describe message, got {:?}", describe.tag as char);
                }
                let sync = read_typed_message(&mut socket).await?;
                if sync.tag != b'S' {
                    anyhow::bail!("expected Sync after Parse/Describe, got {:?}", sync.tag as char);
                }

                write_message(&mut socket, b'1', |_| {}).await?;
                write_parameter_description(&mut socket, &[25]).await?;
                write_row_description(&mut socket, b"value").await?;
                write_ready_for_query(&mut socket).await?;

                let bind = read_typed_message(&mut socket).await?;
                if bind.tag != b'B' {
                    anyhow::bail!("expected Bind message, got {:?}", bind.tag as char);
                }
                let param_value = parse_bind_first_text_param(&bind.payload)?;
                if param_value != "hello" {
                    anyhow::bail!("unexpected bound parameter: {param_value}");
                }

                let execute = read_typed_message(&mut socket).await?;
                if execute.tag != b'E' {
                    anyhow::bail!("expected Execute message, got {:?}", execute.tag as char);
                }
                let sync = read_typed_message(&mut socket).await?;
                if sync.tag != b'S' {
                    anyhow::bail!("expected Sync after Execute, got {:?}", sync.tag as char);
                }

                write_message(&mut socket, b'2', |_| {}).await?;
                write_data_row(&mut socket, &[b"hello"]).await?;
                write_command_complete(&mut socket, b"SELECT 1").await?;
                write_ready_for_query(&mut socket).await?;

                let next = read_typed_message(&mut socket).await?;
                if next.tag == b'C' {
                    let sync = read_typed_message(&mut socket).await?;
                    if sync.tag != b'S' {
                        anyhow::bail!("expected Sync after Close, got {:?}", sync.tag as char);
                    }
                    write_message(&mut socket, b'3', |_| {}).await?;
                    write_ready_for_query(&mut socket).await?;

                    let terminate = read_typed_message(&mut socket).await?;
                    if terminate.tag != b'X' {
                        anyhow::bail!("expected Terminate message, got {:?}", terminate.tag as char);
                    }
                } else if next.tag != b'X' {
                    anyhow::bail!("expected Close or Terminate message, got {:?}", next.tag as char);
                }
                Ok(())
            }
        }
    });

    Ok((port, shutdown_tx, task))
}

async fn spawn_mock_postgres_transaction_server() -> Result<(u16, oneshot::Sender<()>, tokio::task::JoinHandle<Result<()>>)> {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .context("failed to bind mock postgres transaction listener")?;
    let port = listener
        .local_addr()
        .context("failed to get mock postgres transaction addr")?
        .port();
    let (shutdown_tx, mut shutdown_rx) = oneshot::channel();

    let task = tokio::spawn(async move {
        tokio::select! {
            _ = &mut shutdown_rx => Ok(()),
            accepted = listener.accept() => {
                let (mut socket, _) = accepted.context("failed to accept postgres transaction client")?;

                let _startup = read_startup_message(&mut socket).await?;
                write_authentication_ok(&mut socket).await?;
                write_parameter_status(&mut socket, b"client_encoding", b"UTF8").await?;
                write_parameter_status(&mut socket, b"server_version", b"16.0").await?;
                write_backend_key_data(&mut socket).await?;
                write_ready_for_query(&mut socket).await?;

                let mut expected_idx = 0usize;
                loop {
                    let next = read_typed_message(&mut socket).await?;
                    match next.tag {
                        b'P' => {
                            match expected_idx {
                                0 => {
                                    handle_extended_query(
                                        &mut socket,
                                        next,
                                        "begin",
                                        None,
                                        None,
                                        b"BEGIN",
                                    ).await?;
                                }
                                1 => {
                                    handle_extended_query(
                                        &mut socket,
                                        next,
                                        "select $1::text as value",
                                        Some("hello"),
                                        Some((b"value".as_slice(), vec![b"hello".as_slice()])),
                                        b"SELECT 1",
                                    ).await?;
                                }
                                2 => {
                                    handle_extended_query(
                                        &mut socket,
                                        next,
                                        "commit",
                                        None,
                                        None,
                                        b"COMMIT",
                                    ).await?;
                                }
                                _ => anyhow::bail!("unexpected extra Parse message in transaction flow"),
                            }
                            expected_idx += 1;
                        }
                        b'C' => {
                            let sync = read_typed_message(&mut socket).await?;
                            if sync.tag != b'S' {
                                anyhow::bail!("expected Sync after Close, got {:?}", sync.tag as char);
                            }
                            write_message(&mut socket, b'3', |_| {}).await?;
                            write_ready_for_query(&mut socket).await?;
                        }
                        b'X' => {
                            if expected_idx != 3 {
                                anyhow::bail!("transaction server terminated before all expected queries were received");
                            }
                            break Ok(());
                        }
                        other => anyhow::bail!("unexpected postgres transaction message: {:?}", other as char),
                    }
                }
            }
        }
    });

    Ok((port, shutdown_tx, task))
}

async fn spawn_mock_postgres_numeric_result_server() -> Result<(u16, oneshot::Sender<()>, tokio::task::JoinHandle<Result<()>>)> {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .context("failed to bind mock postgres numeric listener")?;
    let port = listener
        .local_addr()
        .context("failed to get mock postgres numeric addr")?
        .port();
    let (shutdown_tx, mut shutdown_rx) = oneshot::channel();

    let task = tokio::spawn(async move {
        tokio::select! {
            _ = &mut shutdown_rx => Ok(()),
            accepted = listener.accept() => {
                let (mut socket, _) = accepted.context("failed to accept postgres numeric client")?;

                let _startup = read_startup_message(&mut socket).await?;
                write_authentication_ok(&mut socket).await?;
                write_parameter_status(&mut socket, b"client_encoding", b"UTF8").await?;
                write_parameter_status(&mut socket, b"server_version", b"16.0").await?;
                write_backend_key_data(&mut socket).await?;
                write_ready_for_query(&mut socket).await?;

                let parse = read_typed_message(&mut socket).await?;
                if parse.tag != b'P' {
                    anyhow::bail!("expected Parse message, got {:?}", parse.tag as char);
                }

                handle_extended_query_columns(
                    &mut socket,
                    parse,
                    "select 12.3400::numeric as amount",
                    &[],
                    Some((&[(b"amount".as_slice(), 1700)], vec![Some(b"12.3400".as_slice())])),
                    b"SELECT 1",
                ).await?;

                let next = read_typed_message(&mut socket).await?;
                if next.tag == b'C' {
                    let sync = read_typed_message(&mut socket).await?;
                    if sync.tag != b'S' {
                        anyhow::bail!("expected Sync after Close, got {:?}", sync.tag as char);
                    }
                    write_message(&mut socket, b'3', |_| {}).await?;
                    write_ready_for_query(&mut socket).await?;

                    let terminate = read_typed_message(&mut socket).await?;
                    if terminate.tag != b'X' {
                        anyhow::bail!("expected Terminate message, got {:?}", terminate.tag as char);
                    }
                } else if next.tag != b'X' {
                    anyhow::bail!("expected Close or Terminate message, got {:?}", next.tag as char);
                }

                Ok(())
            }
        }
    });

    Ok((port, shutdown_tx, task))
}

async fn spawn_mock_postgres_json_array_result_server() -> Result<(u16, oneshot::Sender<()>, tokio::task::JoinHandle<Result<()>>)> {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .context("failed to bind mock postgres json-array listener")?;
    let port = listener
        .local_addr()
        .context("failed to get mock postgres json-array addr")?
        .port();
    let (shutdown_tx, mut shutdown_rx) = oneshot::channel();

    let task = tokio::spawn(async move {
        tokio::select! {
            _ = &mut shutdown_rx => Ok(()),
            accepted = listener.accept() => {
                let (mut socket, _) = accepted.context("failed to accept postgres json-array client")?;

                let _startup = read_startup_message(&mut socket).await?;
                write_authentication_ok(&mut socket).await?;
                write_parameter_status(&mut socket, b"client_encoding", b"UTF8").await?;
                write_parameter_status(&mut socket, b"server_version", b"16.0").await?;
                write_backend_key_data(&mut socket).await?;
                write_ready_for_query(&mut socket).await?;

                let parse = read_typed_message(&mut socket).await?;
                if parse.tag != b'P' {
                    anyhow::bail!("expected Parse message, got {:?}", parse.tag as char);
                }

                handle_extended_query_columns(
                    &mut socket,
                    parse,
                    "select '{\"ok\":true,\"label\":\"json\"}'::jsonb as payload, '{1,2,3}'::int8[] as ids, '{\"alpha\",\"beta\"}'::text[] as tags",
                    &[],
                    Some((&[(b"payload".as_slice(), 3802), (b"ids".as_slice(), 1016), (b"tags".as_slice(), 1009)], vec![Some(b"{\"ok\":true,\"label\":\"json\"}".as_slice()), Some(b"{1,2,3}".as_slice()), Some(b"{\"alpha\",\"beta\"}".as_slice())])),
                    b"SELECT 1",
                ).await?;

                let next = read_typed_message(&mut socket).await?;
                if next.tag == b'C' {
                    let sync = read_typed_message(&mut socket).await?;
                    if sync.tag != b'S' {
                        anyhow::bail!("expected Sync after Close, got {:?}", sync.tag as char);
                    }
                    write_message(&mut socket, b'3', |_| {}).await?;
                    write_ready_for_query(&mut socket).await?;

                    let terminate = read_typed_message(&mut socket).await?;
                    if terminate.tag != b'X' {
                        anyhow::bail!("expected Terminate message, got {:?}", terminate.tag as char);
                    }
                } else if next.tag != b'X' {
                    anyhow::bail!("expected Close or Terminate message, got {:?}", next.tag as char);
                }

                Ok(())
            }
        }
    });

    Ok((port, shutdown_tx, task))
}

async fn spawn_mock_postgres_temporal_result_server() -> Result<(u16, oneshot::Sender<()>, tokio::task::JoinHandle<Result<()>>)> {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .context("failed to bind mock postgres temporal listener")?;
    let port = listener
        .local_addr()
        .context("failed to get mock postgres temporal addr")?
        .port();
    let (shutdown_tx, mut shutdown_rx) = oneshot::channel();

    let task = tokio::spawn(async move {
        tokio::select! {
            _ = &mut shutdown_rx => Ok(()),
            accepted = listener.accept() => {
                let (mut socket, _) = accepted.context("failed to accept postgres temporal client")?;

                let _startup = read_startup_message(&mut socket).await?;
                write_authentication_ok(&mut socket).await?;
                write_parameter_status(&mut socket, b"client_encoding", b"UTF8").await?;
                write_parameter_status(&mut socket, b"server_version", b"16.0").await?;
                write_backend_key_data(&mut socket).await?;
                write_ready_for_query(&mut socket).await?;

                let parse = read_typed_message(&mut socket).await?;
                if parse.tag != b'P' {
                    anyhow::bail!("expected Parse message, got {:?}", parse.tag as char);
                }

                handle_extended_query_columns(
                    &mut socket,
                    parse,
                    "select '2026-03-17'::date as created_on, '12:34:56'::time as starts_at, '12:34:56+00'::timetz as starts_at_tz, '2026-03-17 12:34:56'::timestamp as created_at, '2026-03-17 12:34:56+00'::timestamptz as created_at_utc, '2 days 03:04:05'::interval as elapsed, '123e4567-e89b-12d3-a456-426614174000'::uuid as id",
                    &[],
                    Some((&[
                        (b"created_on".as_slice(), 1082),
                        (b"starts_at".as_slice(), 1083),
                        (b"starts_at_tz".as_slice(), 1266),
                        (b"created_at".as_slice(), 1114),
                        (b"created_at_utc".as_slice(), 1184),
                        (b"elapsed".as_slice(), 1186),
                        (b"id".as_slice(), 2950),
                    ], vec![
                        Some(b"2026-03-17".as_slice()),
                        Some(b"12:34:56".as_slice()),
                        Some(b"12:34:56+00".as_slice()),
                        Some(b"2026-03-17 12:34:56".as_slice()),
                        Some(b"2026-03-17 12:34:56+00".as_slice()),
                        Some(b"2 days 03:04:05".as_slice()),
                        Some(b"123e4567-e89b-12d3-a456-426614174000".as_slice()),
                    ])),
                    b"SELECT 1",
                ).await?;

                let next = read_typed_message(&mut socket).await?;
                if next.tag == b'C' {
                    let sync = read_typed_message(&mut socket).await?;
                    if sync.tag != b'S' {
                        anyhow::bail!("expected Sync after Close, got {:?}", sync.tag as char);
                    }
                    write_message(&mut socket, b'3', |_| {}).await?;
                    write_ready_for_query(&mut socket).await?;

                    let terminate = read_typed_message(&mut socket).await?;
                    if terminate.tag != b'X' {
                        anyhow::bail!("expected Terminate message, got {:?}", terminate.tag as char);
                    }
                } else if next.tag != b'X' {
                    anyhow::bail!("expected Close or Terminate message, got {:?}", next.tag as char);
                }

                Ok(())
            }
        }
    });

    Ok((port, shutdown_tx, task))
}

async fn spawn_mock_postgres_bytea_oid_result_server() -> Result<(u16, oneshot::Sender<()>, tokio::task::JoinHandle<Result<()>>)> {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .context("failed to bind mock postgres bytea/oid listener")?;
    let port = listener
        .local_addr()
        .context("failed to get mock postgres bytea/oid addr")?
        .port();
    let (shutdown_tx, mut shutdown_rx) = oneshot::channel();

    let task = tokio::spawn(async move {
        tokio::select! {
            _ = &mut shutdown_rx => Ok(()),
            accepted = listener.accept() => {
                let (mut socket, _) = accepted.context("failed to accept postgres bytea/oid client")?;

                let _startup = read_startup_message(&mut socket).await?;
                write_authentication_ok(&mut socket).await?;
                write_parameter_status(&mut socket, b"client_encoding", b"UTF8").await?;
                write_parameter_status(&mut socket, b"server_version", b"16.0").await?;
                write_backend_key_data(&mut socket).await?;
                write_ready_for_query(&mut socket).await?;

                let parse = read_typed_message(&mut socket).await?;
                if parse.tag != b'P' {
                    anyhow::bail!("expected Parse message, got {:?}", parse.tag as char);
                }

                handle_extended_query_columns(
                    &mut socket,
                    parse,
                    "select '\\x6869'::bytea as payload, 42::oid as object_id",
                    &[],
                    Some((&[
                        (b"payload".as_slice(), 17),
                        (b"object_id".as_slice(), 26),
                    ], vec![
                        Some(b"\\x6869".as_slice()),
                        Some(b"42".as_slice()),
                    ])),
                    b"SELECT 1",
                ).await?;

                let next = read_typed_message(&mut socket).await?;
                if next.tag == b'C' {
                    let sync = read_typed_message(&mut socket).await?;
                    if sync.tag != b'S' {
                        anyhow::bail!("expected Sync after Close, got {:?}", sync.tag as char);
                    }
                    write_message(&mut socket, b'3', |_| {}).await?;
                    write_ready_for_query(&mut socket).await?;

                    let terminate = read_typed_message(&mut socket).await?;
                    if terminate.tag != b'X' {
                        anyhow::bail!("expected Terminate message, got {:?}", terminate.tag as char);
                    }
                } else if next.tag != b'X' {
                    anyhow::bail!("expected Close or Terminate message, got {:?}", next.tag as char);
                }

                Ok(())
            }
        }
    });

    Ok((port, shutdown_tx, task))
}

async fn spawn_mock_postgres_mixed_types_server() -> Result<(u16, oneshot::Sender<()>, tokio::task::JoinHandle<Result<()>>)> {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .context("failed to bind mock postgres mixed-types listener")?;
    let port = listener
        .local_addr()
        .context("failed to get mock postgres mixed-types addr")?
        .port();
    let (shutdown_tx, mut shutdown_rx) = oneshot::channel();

    let task = tokio::spawn(async move {
        tokio::select! {
            _ = &mut shutdown_rx => Ok(()),
            accepted = listener.accept() => {
                let (mut socket, _) = accepted.context("failed to accept postgres mixed-types client")?;

                let _startup = read_startup_message(&mut socket).await?;
                write_authentication_ok(&mut socket).await?;
                write_parameter_status(&mut socket, b"client_encoding", b"UTF8").await?;
                write_parameter_status(&mut socket, b"server_version", b"16.0").await?;
                write_backend_key_data(&mut socket).await?;
                write_ready_for_query(&mut socket).await?;

                let parse = read_typed_message(&mut socket).await?;
                if parse.tag != b'P' {
                    anyhow::bail!("expected Parse message, got {:?}", parse.tag as char);
                }
                let sql = parse_parse_sql(&parse.payload)?;
                if sql != "select ($1::int8 is not null) as has_n, $2::boolean as flag, ($3::float8 is not null) as has_ratio, $4::text as note, null::int8 as empty" {
                    anyhow::bail!("unexpected mixed-types SQL: {sql}");
                }

                let describe = read_typed_message(&mut socket).await?;
                if describe.tag != b'D' {
                    anyhow::bail!("expected Describe message, got {:?}", describe.tag as char);
                }
                let sync = read_typed_message(&mut socket).await?;
                if sync.tag != b'S' {
                    anyhow::bail!("expected Sync after Parse/Describe, got {:?}", sync.tag as char);
                }

                write_message(&mut socket, b'1', |_| {}).await?;
                write_parameter_description(&mut socket, &[20, 16, 701, 25]).await?;
                write_row_description_columns(
                    &mut socket,
                    &[(b"has_n", 16), (b"flag", 16), (b"has_ratio", 16), (b"note", 25), (b"empty", 20)],
                ).await?;
                write_ready_for_query(&mut socket).await?;

                let bind = read_typed_message(&mut socket).await?;
                if bind.tag != b'B' {
                    anyhow::bail!("expected Bind message, got {:?}", bind.tag as char);
                }
                let params = parse_bind_params_for_types(&bind.payload, &["int8", "bool", "float8", "text"])?;
                assert_eq!(params, vec![Some("42".to_string()), Some("t".to_string()), Some("3.5".to_string()), Some("hello".to_string())]);

                let execute = read_typed_message(&mut socket).await?;
                if execute.tag != b'E' {
                    anyhow::bail!("expected Execute message, got {:?}", execute.tag as char);
                }
                let sync = read_typed_message(&mut socket).await?;
                if sync.tag != b'S' {
                    anyhow::bail!("expected Sync after Execute, got {:?}", sync.tag as char);
                }

                write_message(&mut socket, b'2', |_| {}).await?;
                write_data_row_opt(&mut socket, &[Some(b"t"), Some(b"t"), Some(b"t"), Some(b"hello"), None]).await?;
                write_command_complete(&mut socket, b"SELECT 1").await?;
                write_ready_for_query(&mut socket).await?;

                let next = read_typed_message(&mut socket).await?;
                if next.tag == b'C' {
                    let sync = read_typed_message(&mut socket).await?;
                    if sync.tag != b'S' {
                        anyhow::bail!("expected Sync after Close, got {:?}", sync.tag as char);
                    }
                    write_message(&mut socket, b'3', |_| {}).await?;
                    write_ready_for_query(&mut socket).await?;

                    let terminate = read_typed_message(&mut socket).await?;
                    if terminate.tag != b'X' {
                        anyhow::bail!("expected Terminate message, got {:?}", terminate.tag as char);
                    }
                } else if next.tag != b'X' {
                    anyhow::bail!("expected Close or Terminate message, got {:?}", next.tag as char);
                }
                Ok(())
            }
        }
    });

    Ok((port, shutdown_tx, task))
}

async fn spawn_mock_postgres_tls_server(
    acceptor: TlsAcceptor,
) -> Result<(u16, oneshot::Sender<()>, tokio::task::JoinHandle<Result<()>>)> {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .context("failed to bind mock postgres TLS listener")?;
    let port = listener
        .local_addr()
        .context("failed to get mock postgres TLS addr")?
        .port();
    let (shutdown_tx, mut shutdown_rx) = oneshot::channel();

    let task = tokio::spawn(async move {
        tokio::select! {
            _ = &mut shutdown_rx => Ok(()),
            accepted = listener.accept() => {
                let (mut socket, _) = accepted.context("failed to accept postgres TLS client")?;

                let ssl_request = read_startup_message(&mut socket).await?;
                if ssl_request.len() != 4 || i32::from_be_bytes([ssl_request[0], ssl_request[1], ssl_request[2], ssl_request[3]]) != 80_877_103 {
                    anyhow::bail!("expected SSLRequest before TLS handshake");
                }
                socket.write_all(b"S").await.context("failed to accept postgres TLS request")?;

                let mut tls_stream = acceptor.accept(socket).await.context("failed to complete postgres TLS handshake")?;
                let _startup = read_startup_message(&mut tls_stream).await?;
                write_authentication_ok(&mut tls_stream).await?;
                write_parameter_status(&mut tls_stream, b"client_encoding", b"UTF8").await?;
                write_parameter_status(&mut tls_stream, b"server_version", b"16.0").await?;
                write_backend_key_data(&mut tls_stream).await?;
                write_ready_for_query(&mut tls_stream).await?;

                let query = read_typed_message(&mut tls_stream).await?;
                if query.tag != b'Q' {
                    anyhow::bail!("expected Query message, got {:?}", query.tag as char);
                }
                let sql = String::from_utf8(query.payload[..query.payload.len().saturating_sub(1)].to_vec())
                    .context("invalid TLS query payload")?;
                if sql != "select 1 as value" {
                    anyhow::bail!("unexpected TLS SQL: {sql}");
                }

                write_row_description(&mut tls_stream, b"value").await?;
                write_data_row(&mut tls_stream, &[b"1"]).await?;
                write_command_complete(&mut tls_stream, b"SELECT 1").await?;
                write_ready_for_query(&mut tls_stream).await?;

                let terminate = read_typed_message(&mut tls_stream).await?;
                if terminate.tag != b'X' {
                    anyhow::bail!("expected Terminate message, got {:?}", terminate.tag as char);
                }
                Ok(())
            }
        }
    });

    Ok((port, shutdown_tx, task))
}

struct TypedMessage {
    tag: u8,
    payload: Vec<u8>,
}

async fn read_startup_message<S>(socket: &mut S) -> Result<Vec<u8>>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let length = socket.read_i32().await.context("failed to read startup length")? as usize;
    let mut payload = vec![0; length.saturating_sub(4)];
    socket.read_exact(&mut payload).await.context("failed to read startup payload")?;
    Ok(payload)
}

async fn read_typed_message<S>(socket: &mut S) -> Result<TypedMessage>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let tag = socket.read_u8().await.context("failed to read message tag")?;
    let length = socket.read_i32().await.context("failed to read message length")? as usize;
    let mut payload = vec![0; length.saturating_sub(4)];
    socket.read_exact(&mut payload).await.context("failed to read message payload")?;
    Ok(TypedMessage { tag, payload })
}

async fn write_authentication_ok<S>(socket: &mut S) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    write_message(socket, b'R', |buf| buf.extend_from_slice(&0u32.to_be_bytes())).await
}

async fn write_parameter_status<S>(socket: &mut S, key: &[u8], value: &[u8]) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    write_message(socket, b'S', |buf| {
        buf.extend_from_slice(key);
        buf.push(0);
        buf.extend_from_slice(value);
        buf.push(0);
    })
    .await
}

async fn write_backend_key_data<S>(socket: &mut S) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    write_message(socket, b'K', |buf| {
        buf.extend_from_slice(&1u32.to_be_bytes());
        buf.extend_from_slice(&2u32.to_be_bytes());
    })
    .await
}

async fn write_parameter_description<S>(socket: &mut S, oids: &[u32]) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    write_message(socket, b't', |buf| {
        buf.extend_from_slice(&(oids.len() as u16).to_be_bytes());
        for oid in oids {
            buf.extend_from_slice(&oid.to_be_bytes());
        }
    })
    .await
}

async fn write_ready_for_query<S>(socket: &mut S) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    write_message(socket, b'Z', |buf| buf.push(b'I')).await
}

async fn write_row_description<S>(socket: &mut S, column_name: &[u8]) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    write_row_description_columns(socket, &[(column_name, 25)]).await
}

async fn write_row_description_columns<S>(socket: &mut S, columns: &[(&[u8], u32)]) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    write_message(socket, b'T', |buf| {
        buf.extend_from_slice(&(columns.len() as u16).to_be_bytes());
        for (column_name, oid) in columns {
            let type_size = postgres_type_size(*oid);
            buf.extend_from_slice(column_name);
            buf.push(0);
            buf.extend_from_slice(&0u32.to_be_bytes());
            buf.extend_from_slice(&0u16.to_be_bytes());
            buf.extend_from_slice(&oid.to_be_bytes());
            buf.extend_from_slice(&type_size.to_be_bytes());
            buf.extend_from_slice(&(-1i32).to_be_bytes());
            buf.extend_from_slice(&0u16.to_be_bytes());
        }
    })
    .await
}

fn postgres_type_size(oid: u32) -> i16 {
    match oid {
        16 => 1,
        20 => 8,
        21 => 2,
        23 => 4,
        700 => 4,
        701 => 8,
        _ => -1,
    }
}

async fn write_data_row<S>(socket: &mut S, values: &[&[u8]]) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    write_data_row_opt(socket, &values.iter().map(|value| Some(*value)).collect::<Vec<_>>()).await
}

async fn write_data_row_opt<S>(socket: &mut S, values: &[Option<&[u8]>]) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    write_message(socket, b'D', |buf| {
        buf.extend_from_slice(&(values.len() as u16).to_be_bytes());
        for value in values {
            match value {
                Some(value) => {
                    buf.extend_from_slice(&(value.len() as i32).to_be_bytes());
                    buf.extend_from_slice(value);
                }
                None => buf.extend_from_slice(&(-1i32).to_be_bytes()),
            }
        }
    })
    .await
}

async fn write_command_complete<S>(socket: &mut S, tag: &[u8]) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    write_message(socket, b'C', |buf| {
        buf.extend_from_slice(tag);
        buf.push(0);
    })
    .await
}

fn parse_bind_first_text_param(payload: &[u8]) -> Result<String> {
    let mut idx = 0usize;
    idx = skip_c_string(payload, idx)?;
    idx = skip_c_string(payload, idx)?;

    let format_count = read_u16(payload, &mut idx)? as usize;
    idx = idx.saturating_add(format_count * 2);

    let param_count = read_u16(payload, &mut idx)? as usize;
    if param_count == 0 {
        anyhow::bail!("bind payload contained no parameters");
    }

    let param_len = read_i32(payload, &mut idx)?;
    if param_len < 0 {
        anyhow::bail!("first bind parameter was null");
    }
    let len = param_len as usize;
    let end = idx.saturating_add(len);
    if end > payload.len() {
        anyhow::bail!("bind payload parameter truncated");
    }
    let value = String::from_utf8(payload[idx..end].to_vec()).context("invalid bind parameter utf8")?;
    Ok(value)
}

fn parse_bind_first_text_param_opt(payload: &[u8]) -> Result<Option<String>> {
    let params = parse_bind_text_params(payload)?;
    Ok(params.into_iter().next().unwrap_or(None))
}

fn parse_bind_text_params(payload: &[u8]) -> Result<Vec<Option<String>>> {
    parse_bind_params_for_types(payload, &[])
}

fn parse_bind_params_for_types(payload: &[u8], expected_types: &[&str]) -> Result<Vec<Option<String>>> {
    let mut idx = 0usize;
    idx = skip_c_string(payload, idx)?;
    idx = skip_c_string(payload, idx)?;

    let format_count = read_u16(payload, &mut idx)? as usize;
    let mut format_codes = Vec::with_capacity(format_count);
    for _ in 0..format_count {
        format_codes.push(read_u16(payload, &mut idx)?);
    }

    let param_count = read_u16(payload, &mut idx)? as usize;
    let mut params = Vec::with_capacity(param_count);
    for _ in 0..param_count {
        let param_len = read_i32(payload, &mut idx)?;
        if param_len < 0 {
            params.push(None);
            continue;
        }
        let len = param_len as usize;
        let end = idx.saturating_add(len);
        if end > payload.len() {
            anyhow::bail!("bind payload parameter truncated");
        }
        let param_index = params.len();
        let format_code = if format_codes.is_empty() {
            0
        } else if format_codes.len() == 1 {
            format_codes[0]
        } else {
            *format_codes.get(param_index).ok_or_else(|| anyhow::anyhow!("missing bind format code for parameter"))?
        };
        let bytes = &payload[idx..end];
        let value = decode_bind_param_to_string(bytes, format_code, expected_types.get(param_index).copied())?;
        idx = end;
        params.push(value);
    }
    Ok(params)
}

fn decode_bind_param_to_string(bytes: &[u8], format_code: u16, expected_type: Option<&str>) -> Result<Option<String>> {
    if format_code == 0 {
        return Ok(Some(String::from_utf8(bytes.to_vec()).context("invalid bind parameter utf8")?));
    }

    let value = match expected_type {
        Some("bool") => {
            if bytes.len() != 1 {
                anyhow::bail!("invalid binary bool bind length: {}", bytes.len());
            }
            if bytes[0] == 0 { "f".to_string() } else { "t".to_string() }
        }
        Some("int8") => {
            if bytes.len() != 8 {
                anyhow::bail!("invalid binary int8 bind length: {}", bytes.len());
            }
            i64::from_be_bytes(bytes.try_into().expect("checked int8 width")).to_string()
        }
        Some("float8") => {
            if bytes.len() != 8 {
                anyhow::bail!("invalid binary float8 bind length: {}", bytes.len());
            }
            f64::from_bits(u64::from_be_bytes(bytes.try_into().expect("checked float8 width"))).to_string()
        }
        Some("text") | None => String::from_utf8(bytes.to_vec()).context("invalid binary text bind utf8")?,
        Some(other) => anyhow::bail!("unsupported expected bind type: {other}"),
    };

    Ok(Some(value))
}

fn parse_parse_sql(payload: &[u8]) -> Result<String> {
    let mut idx = 0usize;
    idx = skip_c_string(payload, idx)?;
    let sql_start = idx;
    idx = skip_c_string(payload, idx)?;
    if idx == 0 || sql_start >= idx {
        anyhow::bail!("parse payload missing SQL");
    }
    String::from_utf8(payload[sql_start..idx - 1].to_vec()).context("invalid parse SQL utf8")
}

async fn handle_extended_query<S>(
    socket: &mut S,
    parse: TypedMessage,
    expected_sql: &str,
    expected_param: Option<&str>,
    result_row: Option<(&[u8], Vec<&[u8]>)>,
    command_complete: &[u8],
) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let sql = parse_parse_sql(&parse.payload)?;
    if sql != expected_sql {
        anyhow::bail!("unexpected extended-query SQL: {sql}");
    }

    let describe = read_typed_message(socket).await?;
    if describe.tag != b'D' {
        anyhow::bail!("expected Describe message, got {:?}", describe.tag as char);
    }
    let sync = read_typed_message(socket).await?;
    if sync.tag != b'S' {
        anyhow::bail!("expected Sync after Parse/Describe, got {:?}", sync.tag as char);
    }

    write_message(socket, b'1', |_| {}).await?;
    write_parameter_description(socket, if expected_param.is_some() { &[25] } else { &[] }).await?;
    if let Some((column_name, _)) = &result_row {
        write_row_description(socket, column_name).await?;
    } else {
        write_message(socket, b'n', |_| {}).await?;
    }
    write_ready_for_query(socket).await?;

    let bind = read_typed_message(socket).await?;
    if bind.tag != b'B' {
        anyhow::bail!("expected Bind message, got {:?}", bind.tag as char);
    }
    let actual_param = parse_bind_first_text_param_opt(&bind.payload)?;
    match (expected_param, actual_param.as_deref()) {
        (Some(expected), Some(actual)) if expected == actual => {}
        (None, None) => {}
        (expected, actual) => anyhow::bail!("unexpected bound parameter: expected {:?}, got {:?}", expected, actual),
    }

    let execute = read_typed_message(socket).await?;
    if execute.tag != b'E' {
        anyhow::bail!("expected Execute message, got {:?}", execute.tag as char);
    }
    let sync = read_typed_message(socket).await?;
    if sync.tag != b'S' {
        anyhow::bail!("expected Sync after Execute, got {:?}", sync.tag as char);
    }

    write_message(socket, b'2', |_| {}).await?;
    if let Some((_, values)) = result_row {
        write_data_row(socket, &values).await?;
    }
    write_command_complete(socket, command_complete).await?;
    write_ready_for_query(socket).await?;
    Ok(())
}

async fn handle_extended_query_columns<S>(
    socket: &mut S,
    parse: TypedMessage,
    expected_sql: &str,
    parameter_oids: &[u32],
    result_row: Option<(&[(&[u8], u32)], Vec<Option<&[u8]>>)>,
    command_complete: &[u8],
) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let sql = parse_parse_sql(&parse.payload)?;
    if sql != expected_sql {
        anyhow::bail!("unexpected extended-query SQL: {sql}");
    }

    let describe = read_typed_message(socket).await?;
    if describe.tag != b'D' {
        anyhow::bail!("expected Describe message, got {:?}", describe.tag as char);
    }
    let sync = read_typed_message(socket).await?;
    if sync.tag != b'S' {
        anyhow::bail!("expected Sync after Parse/Describe, got {:?}", sync.tag as char);
    }

    write_message(socket, b'1', |_| {}).await?;
    write_parameter_description(socket, parameter_oids).await?;
    if let Some((columns, _)) = &result_row {
        write_row_description_columns(socket, columns).await?;
    } else {
        write_message(socket, b'n', |_| {}).await?;
    }
    write_ready_for_query(socket).await?;

    let bind = read_typed_message(socket).await?;
    if bind.tag != b'B' {
        anyhow::bail!("expected Bind message, got {:?}", bind.tag as char);
    }
    let actual_params = parse_bind_text_params(&bind.payload)?;
    if !actual_params.is_empty() {
        anyhow::bail!("unexpected bound parameters: {:?}", actual_params);
    }

    let execute = read_typed_message(socket).await?;
    if execute.tag != b'E' {
        anyhow::bail!("expected Execute message, got {:?}", execute.tag as char);
    }
    let sync = read_typed_message(socket).await?;
    if sync.tag != b'S' {
        anyhow::bail!("expected Sync after Execute, got {:?}", sync.tag as char);
    }

    write_message(socket, b'2', |_| {}).await?;
    if let Some((_, values)) = result_row {
        write_data_row_opt(socket, &values).await?;
    }
    write_command_complete(socket, command_complete).await?;
    write_ready_for_query(socket).await?;
    Ok(())
}

fn skip_c_string(payload: &[u8], mut idx: usize) -> Result<usize> {
    while idx < payload.len() {
        if payload[idx] == 0 {
            return Ok(idx + 1);
        }
        idx += 1;
    }
    anyhow::bail!("unterminated c-string in postgres payload")
}

fn read_u16(payload: &[u8], idx: &mut usize) -> Result<u16> {
    let end = idx.saturating_add(2);
    if end > payload.len() {
        anyhow::bail!("short postgres payload reading u16");
    }
    let value = u16::from_be_bytes([payload[*idx], payload[*idx + 1]]);
    *idx = end;
    Ok(value)
}

fn read_i32(payload: &[u8], idx: &mut usize) -> Result<i32> {
    let end = idx.saturating_add(4);
    if end > payload.len() {
        anyhow::bail!("short postgres payload reading i32");
    }
    let value = i32::from_be_bytes([
        payload[*idx],
        payload[*idx + 1],
        payload[*idx + 2],
        payload[*idx + 3],
    ]);
    *idx = end;
    Ok(value)
}

async fn write_message<S, F>(socket: &mut S, tag: u8, build: F) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin,
    F: FnOnce(&mut Vec<u8>),
{
    let mut payload = Vec::new();
    build(&mut payload);
    socket.write_u8(tag).await.context("failed to write message tag")?;
    socket
        .write_i32((payload.len() as i32) + 4)
        .await
        .context("failed to write message length")?;
    socket.write_all(&payload).await.context("failed to write message payload")?;
    Ok(())
}

fn postgres_test_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn ensure_rustls_provider() {
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
    });
}

struct EnvVarGuard {
    key: &'static str,
    previous: Option<String>,
}

impl EnvVarGuard {
    fn set(key: &'static str, value: &str) -> Self {
        let previous = std::env::var(key).ok();
        unsafe {
            std::env::set_var(key, value);
        }
        Self { key, previous }
    }
}

impl Drop for EnvVarGuard {
    fn drop(&mut self) {
        match &self.previous {
            Some(value) => unsafe {
                std::env::set_var(self.key, value);
            },
            None => unsafe {
                std::env::remove_var(self.key);
            },
        }
    }
}
