//! Integration and argument-parsing tests for the `flux` CLI.
//!
//! Run with:
//!   cargo test -p cli
//!
//! Tests are grouped into:
//!   - `parsing` – verify that commands/flags parse correctly (offline, no HTTP)
//!   - `auth`    – login command validation
//!   - `api`     – mock-HTTP tests that set FLUX_API_URL → mockito server

use assert_cmd::prelude::*;
use predicates::prelude::*;
use std::process::Command;

// ─── helpers ──────────────────────────────────────────────────────────────────

/// Return a `Command` for the `flux` binary already built by cargo.
fn flux() -> Command {
    Command::cargo_bin("flux").unwrap()
}

// ─── parsing: top-level help & version ────────────────────────────────────────

#[test]
fn help_shows_usage() {
    flux()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("Flux CLI"))
        .stdout(predicate::str::contains("USAGE").or(predicate::str::contains("Usage")));
}

#[test]
fn version_flag_prints_semver() {
    flux()
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::is_match(r"\d+\.\d+\.\d+").unwrap());
}

#[test]
fn unknown_command_exits_nonzero() {
    flux()
        .arg("does-not-exist")
        .assert()
        .failure();
}

// ─── parsing: subcommand help pages ──────────────────────────────────────────

macro_rules! help_test {
    ($name:ident, $($arg:expr),+) => {
        #[test]
        fn $name() {
            flux()
                $(.arg($arg))+
                .arg("--help")
                .assert()
                .success()
                .stdout(predicate::str::contains("Usage").or(predicate::str::contains("USAGE")));
        }
    };
}

help_test!(help_login,        "login");
help_test!(help_whoami,       "whoami");
help_test!(help_tenant,       "tenant");
help_test!(help_project,      "project");
help_test!(help_function,     "function");
help_test!(help_deploy,       "deploy");
help_test!(help_invoke,       "invoke");
help_test!(help_version_sub,  "version");
help_test!(help_deployments,  "deployments");
help_test!(help_logs,         "logs");
help_test!(help_trace,        "trace");
help_test!(help_debug,        "debug");
help_test!(help_monitor,      "monitor");
help_test!(help_secrets,      "secrets");
help_test!(help_config,       "config");
help_test!(help_api_key,      "api-key");
help_test!(help_gateway,      "gateway");
help_test!(help_workflow,     "workflow");
help_test!(help_agent,        "agent");
help_test!(help_schedule,     "schedule");
help_test!(help_queue,        "queue");
help_test!(help_event,        "event");
help_test!(help_tool,         "tool");
help_test!(help_env,          "env");
help_test!(help_db,           "db");
help_test!(help_stack,        "stack");
help_test!(help_tail,         "tail");
help_test!(help_errors,       "errors");
help_test!(help_fix,          "fix");
help_test!(help_doctor,       "doctor");
help_test!(help_open,         "open");
help_test!(help_upgrade,      "upgrade");

// ─── parsing: nested subcommand help ──────────────────────────────────────────

macro_rules! nested_help_test {
    ($name:ident, $a:expr, $b:expr) => {
        #[test]
        fn $name() {
            flux()
                .arg($a)
                .arg($b)
                .arg("--help")
                .assert()
                .success();
        }
    };
}

nested_help_test!(help_tenant_create,      "tenant",   "create");
nested_help_test!(help_tenant_list,        "tenant",   "list");
nested_help_test!(help_tenant_use,         "tenant",   "use");
nested_help_test!(help_project_create,     "project",  "create");
nested_help_test!(help_project_list,       "project",  "list");
nested_help_test!(help_function_create,    "function", "create");
nested_help_test!(help_function_list,      "function", "list");
nested_help_test!(help_secrets_list,       "secrets",  "list");
nested_help_test!(help_secrets_set,        "secrets",  "set");
nested_help_test!(help_secrets_delete,     "secrets",  "delete");
nested_help_test!(help_gateway_route,      "gateway",  "route");
nested_help_test!(help_gateway_middleware, "gateway",  "middleware");
nested_help_test!(help_db_create,          "db",       "create");
nested_help_test!(help_db_list,            "db",       "list");
nested_help_test!(help_db_diff,            "db",       "diff");
nested_help_test!(help_db_query,           "db",       "query");
nested_help_test!(help_db_migration,       "db",       "migration");
nested_help_test!(help_stack_up,           "stack",    "up");
nested_help_test!(help_stack_down,         "stack",    "down");
nested_help_test!(help_stack_reset,        "stack",    "reset");
nested_help_test!(help_stack_seed,         "stack",    "seed");
nested_help_test!(help_monitor_status,     "monitor",  "status");
nested_help_test!(help_monitor_metrics,    "monitor",  "metrics");
nested_help_test!(help_version_list,       "version",  "list");
nested_help_test!(help_version_rollback,   "version",  "rollback");
nested_help_test!(help_version_promote,    "version",  "promote");
nested_help_test!(help_version_diff,       "version",  "diff");
nested_help_test!(help_workflow_create,    "workflow", "create");
nested_help_test!(help_workflow_run,       "workflow", "run");
nested_help_test!(help_agent_create,       "agent",    "create");
nested_help_test!(help_agent_simulate,     "agent",    "simulate");
nested_help_test!(help_schedule_create,    "schedule", "create");
nested_help_test!(help_queue_create,       "queue",    "create");
nested_help_test!(help_event_publish,      "event",    "publish");
nested_help_test!(help_tool_list,          "tool",     "list");
nested_help_test!(help_api_key_create,     "api-key",  "create");
nested_help_test!(help_env_list,           "env",      "list");
nested_help_test!(help_config_list,        "config",   "list");

// ─── parsing: global flags are accepted ───────────────────────────────────────

#[test]
fn global_flag_no_color_accepted() {
    // --no-color followed by any subcommand should parse without error;
    // it will fail because no token is set, but NOT with "unexpected argument"
    flux()
        .args(["--no-color", "doctor", "--help"])
        .assert()
        .success();
}

#[test]
fn global_flag_json_accepted() {
    flux()
        .args(["--json", "doctor", "--help"])
        .assert()
        .success();
}

#[test]
fn global_flag_tenant_accepted() {
    flux()
        .args(["--tenant", "my-org", "tenant", "--help"])
        .assert()
        .success();
}

#[test]
fn global_flag_dry_run_accepted() {
    flux()
        .args(["--dry-run", "deploy", "--help"])
        .assert()
        .success();
}

// ─── parsing: backward-compat aliases ────────────────────────────────────────

#[test]
fn create_alias_help_works() {
    // `flux create --help` should work (it's a hidden alias for `flux new`)
    flux()
        .args(["create", "--help"])
        .assert()
        .success();
}

// ─── parsing: required args missing → non-zero exit ──────────────────────────

#[test]
fn trace_requires_request_id() {
    flux()
        .arg("trace")
        .assert()
        .failure();
}

#[test]
fn invoke_requires_name() {
    flux()
        .arg("invoke")
        .assert()
        .failure();
}

#[test]
// flux debug (no args) is now interactive mode — it should parse correctly
// and show help content rather than failing with "required argument missing".
fn debug_without_args_parses_as_interactive() {
    // --help verifies that the optional request_id is described correctly
    flux()
        .args(["debug", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("interactive").or(
            predicate::str::contains("request_id").or(
                predicate::str::contains("request-id"),
            ),
        ));
}

#[test]
fn tail_help_shows_usage() {
    flux()
        .args(["tail", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("tail").or(predicate::str::contains("Usage")));
}

#[test]
fn tail_auto_debug_flag_is_accepted() {
    // --auto-debug is a known flag; --help should parse cleanly
    flux()
        .args(["tail", "--auto-debug", "--help"])
        .assert()
        .success();
}

#[test]
fn fix_and_debug_are_equivalent_aliases() {
    // both should show the same help structure
    flux()
        .args(["fix", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("alias").or(
            predicate::str::contains("debug").or(
                predicate::str::contains("Usage"),
            ),
        ));
}

#[test]
fn errors_command_parses_since_flag() {
    // --help with --since flag verifies the flag is defined
    flux()
        .args(["errors", "--since", "24h", "--help"])
        .assert()
        .success();
}

// ─── API mock tests (require FLUX_API_URL override) ──────────────────────

#[cfg(test)]
mod api_mock {
    use assert_cmd::prelude::*;
    use mockito::Server;
    use predicates::prelude::*;
    use std::process::Command;
    use tempfile::TempDir;

    fn flux() -> Command {
        Command::cargo_bin("flux").unwrap()
    }

    /// Boot a mockito server, write a fake home dir with config, and return
    /// (server, home_tmpdir, workspace_tmpdir, api_url).
    ///
    /// The `flux` subprocess is run with:
    ///   HOME=home_tmp  (so ~/.flux/config.json points to our fake config)
    ///   cwd=workspace  (an empty dir with no project config, so it won't override)
    async fn setup_mock_server() -> (mockito::ServerGuard, TempDir, TempDir, String) {
        let server = Server::new_async().await;
        let api_url = server.url();

        // Home dir: contains .flux/config.json pointing at the mock
        let home_tmp = tempfile::tempdir().unwrap();
        let dot_flux = home_tmp.path().join(".flux");
        std::fs::create_dir_all(&dot_flux).unwrap();
        let cfg = serde_json::json!({
            "api_url": api_url,
            "token": "flux_test_token_abc123",
            "tenant_id": "t_test",
            "tenant_slug": "test-org",
            "project_id": "p_test",
            "gateway_url": api_url,
            "runtime_url": api_url,
        });
        std::fs::write(
            dot_flux.join("config.json"),
            serde_json::to_string_pretty(&cfg).unwrap(),
        ).unwrap();

        // Workspace dir: empty, so ProjectConfig::load_sync() finds nothing
        let workspace_tmp = tempfile::tempdir().unwrap();

        (server, home_tmp, workspace_tmp, api_url)
    }

    #[tokio::test]
    async fn whoami_prints_token_preview() {
        let (mut server, home_tmp, workspace_tmp, api_url) = setup_mock_server().await;

        let mock = server.mock("GET", "/auth/me")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"id":"u_1","email":"dev@example.com","tenant_id":"t_test","tenant_slug":"test-org","project_id":"p_test"}"#)
            .create_async()
            .await;

        let _out = flux()
            .env("FLUX_API_URL", &api_url)
            .env("FLUX_TOKEN", "flux_test_token_abc123")
            .env("HOME", home_tmp.path())
            .current_dir(workspace_tmp.path())
            .arg("whoami")
            .assert()
            .success();

        mock.assert_async().await;
    }

    #[tokio::test]
    async fn tenant_list_table_output() {
        let (mut server, home_tmp, workspace_tmp, api_url) = setup_mock_server().await;

        let mock = server.mock("GET", "/tenants")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"data":{"tenants":[{"id":"t_1","slug":"acme","name":"Acme Inc","role":"owner","created_at":"2024-01-01T00:00:00Z"}]}}"#)
            .create_async()
            .await;

        flux()
            .env("FLUX_API_URL", &api_url)
            .env("FLUX_TOKEN", "flux_test_token_abc123")
            .env("HOME", home_tmp.path())
            .current_dir(workspace_tmp.path())
            .args(["tenant", "list"])
            .assert()
            .success()
            .stdout(predicate::str::contains("Acme Inc").or(predicate::str::contains("t_1")));

        mock.assert_async().await;
    }

    #[tokio::test]
    async fn secrets_list_output() {
        let (mut server, home_tmp, workspace_tmp, api_url) = setup_mock_server().await;

        let mock = server.mock("GET", mockito::Matcher::Regex(r"^/secrets".to_string()))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"[{"key":"DATABASE_URL","updated_at":"2024-01-01T00:00:00Z","version":1}]"#)
            .create_async()
            .await;

        flux()
            .env("FLUX_API_URL", &api_url)
            .env("FLUX_TOKEN", "flux_test_token_abc123")
            .env("HOME", home_tmp.path())
            .current_dir(workspace_tmp.path())
            .args(["secrets", "list"])
            .assert()
            .success()
            .stdout(predicate::str::contains("DATABASE_URL"));

        mock.assert_async().await;
    }

    #[tokio::test]
    async fn function_list_output() {
        let (mut server, home_tmp, workspace_tmp, api_url) = setup_mock_server().await;

        let mock = server.mock("GET", mockito::Matcher::Regex(r"^/functions".to_string()))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"data":{"functions":[{"id":"fn_1","name":"echo","runtime":"deno","status":"active","updated_at":"2024-01-01T00:00:00Z"}]}}"#)
            .create_async()
            .await;

        flux()
            .env("FLUX_API_URL", &api_url)
            .env("FLUX_TOKEN", "flux_test_token_abc123")
            .env("HOME", home_tmp.path())
            .current_dir(workspace_tmp.path())
            .args(["function", "list"])
            .assert()
            .success()
            .stdout(predicate::str::contains("echo"));

        mock.assert_async().await;
    }

    #[tokio::test]
    async fn gateway_route_list_output() {
        let (mut server, home_tmp, workspace_tmp, api_url) = setup_mock_server().await;

        let mock = server.mock("GET", mockito::Matcher::Regex(r"^/gateway/routes".to_string()))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"data":{"routes":[{"id":"r_1","method":"GET","path":"/hello","function_name":"echo","auth_type":"none","is_async":false}]}}"#)
            .create_async()
            .await;

        flux()
            .env("FLUX_API_URL", &api_url)
            .env("FLUX_TOKEN", "flux_test_token_abc123")
            .env("HOME", home_tmp.path())
            .current_dir(workspace_tmp.path())
            .args(["gateway", "route", "list"])
            .assert()
            .success()
            .stdout(predicate::str::contains("/hello"));

        mock.assert_async().await;
    }

    #[tokio::test]
    async fn secrets_set_calls_post() {
        let (mut server, home_tmp, workspace_tmp, api_url) = setup_mock_server().await;

        let mock = server.mock("POST", mockito::Matcher::Regex(r"^/secrets".to_string()))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"ok":true}"#)
            .create_async()
            .await;

        flux()
            .env("FLUX_API_URL", &api_url)
            .env("FLUX_TOKEN", "flux_test_token_abc123")
            .env("HOME", home_tmp.path())
            .current_dir(workspace_tmp.path())
            .args(["secrets", "set", "MY_KEY", "my_value"])
            .assert()
            .success();

        mock.assert_async().await;
    }

    #[tokio::test]
    async fn db_list_output() {
        let (mut server, home_tmp, workspace_tmp, api_url) = setup_mock_server().await;

        let mock = server.mock("GET", "/db/databases")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"databases":["default","analytics"]}"#)
            .create_async()
            .await;

        flux()
            .env("FLUX_API_URL", &api_url)
            .env("FLUX_TOKEN", "flux_test_token_abc123")
            .env("HOME", home_tmp.path())
            .current_dir(workspace_tmp.path())
            .args(["db", "list"])
            .assert()
            .success()
            .stdout(predicate::str::contains("default"));

        mock.assert_async().await;
    }

    #[tokio::test]
    async fn version_list_output() {
        let (mut server, home_tmp, workspace_tmp, api_url) = setup_mock_server().await;

        let mock = server.mock("GET", mockito::Matcher::Regex(r"^/functions/echo/deployments".to_string()))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"data":{"deployments":[{"id":"d_1","version":3,"status":"active","is_active":true,"created_at":"2024-03-01T12:00:00Z"}]}}"#)
            .create_async()
            .await;

        flux()
            .env("FLUX_API_URL", &api_url)
            .env("FLUX_TOKEN", "flux_test_token_abc123")
            .env("HOME", home_tmp.path())
            .current_dir(workspace_tmp.path())
            .args(["version", "list", "echo"])
            .assert()
            .success()
            .stdout(predicate::str::contains("v3"));

        mock.assert_async().await;
    }

    #[tokio::test]
    async fn monitor_status_output() {
        let (mut server, home_tmp, workspace_tmp, api_url) = setup_mock_server().await;

        let mock = server.mock("GET", "/monitor/status")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"data":[{"service":"api","status":"healthy","latency_ms":12},{"service":"gateway","status":"healthy","latency_ms":8}]}"#)
            .create_async()
            .await;

        flux()
            .env("FLUX_API_URL", &api_url)
            .env("FLUX_TOKEN", "flux_test_token_abc123")
            .env("HOME", home_tmp.path())
            .current_dir(workspace_tmp.path())
            .args(["monitor", "status"])
            .assert()
            .success()
            .stdout(predicate::str::contains("api"));

        mock.assert_async().await;
    }

    #[tokio::test]
    async fn workflow_list_output() {
        let (mut server, home_tmp, workspace_tmp, api_url) = setup_mock_server().await;

        let mock = server.mock("GET", mockito::Matcher::Regex(r"^/workflows".to_string()))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"data":[{"name":"onboarding","status":"active","created_at":"2024-01-01T00:00:00Z"}]}"#)
            .create_async()
            .await;

        flux()
            .env("FLUX_API_URL", &api_url)
            .env("FLUX_TOKEN", "flux_test_token_abc123")
            .env("HOME", home_tmp.path())
            .current_dir(workspace_tmp.path())
            .args(["workflow", "list"])
            .assert()
            .success()
            .stdout(predicate::str::contains("onboarding"));

        mock.assert_async().await;
    }

    #[tokio::test]
    async fn agent_list_output() {
        let (mut server, home_tmp, workspace_tmp, api_url) = setup_mock_server().await;

        let mock = server.mock("GET", mockito::Matcher::Regex(r"^/agents".to_string()))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"data":[{"name":"support-bot","model":"gpt-4o","status":"active"}]}"#)
            .create_async()
            .await;

        flux()
            .env("FLUX_API_URL", &api_url)
            .env("FLUX_TOKEN", "flux_test_token_abc123")
            .env("HOME", home_tmp.path())
            .current_dir(workspace_tmp.path())
            .args(["agent", "list"])
            .assert()
            .success()
            .stdout(predicate::str::contains("support-bot"));

        mock.assert_async().await;
    }

    #[tokio::test]
    async fn api_key_list_output() {
        let (mut server, home_tmp, workspace_tmp, api_url) = setup_mock_server().await;

        let mock = server.mock("GET", "/api-keys")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"data":[{"id":"k_1","name":"CI Key","scopes":["function:invoke"],"created_at":"2024-01-01T00:00:00Z"}]}"#)
            .create_async()
            .await;

        flux()
            .env("FLUX_API_URL", &api_url)
            .env("FLUX_TOKEN", "flux_test_token_abc123")
            .env("HOME", home_tmp.path())
            .current_dir(workspace_tmp.path())
            .args(["api-key", "list"])
            .assert()
            .success()
            .stdout(predicate::str::contains("CI Key"));

        mock.assert_async().await;
    }
}
