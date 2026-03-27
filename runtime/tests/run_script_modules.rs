use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use runtime::isolate_pool::ExecutionContext;
use runtime::JsIsolate;
use serde_json::json;

struct TempDirGuard {
    path: PathBuf,
}

impl TempDirGuard {
    fn new(prefix: &str) -> Self {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("{prefix}-{unique}"));
        fs::create_dir_all(&path).expect("failed to create temp dir");
        Self { path }
    }

    fn write(&self, relative: &str, content: &str) -> PathBuf {
        let path = self.path.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("failed to create parent dir");
        }
        fs::write(&path, content).expect("failed to write fixture");
        path
    }
}

impl Drop for TempDirGuard {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

#[tokio::test]
async fn run_script_supports_relative_ts_imports_and_dynamic_imports() {
    let temp = TempDirGuard::new("flux-run-ts-modules");
    let entry = temp.write(
        "main.ts",
        r#"
import { double } from "./helper.ts";

export default async function(input: { value: number }) {
  const dynamic = await import("./triple.ts");
  return {
    doubled: double(input.value),
    tripled: dynamic.triple(input.value),
  };
}
"#,
    );
    temp.write(
        "helper.ts",
        r#"
export function double(value: number) {
  return value * 2;
}
"#,
    );
    temp.write(
        "triple.ts",
        r#"
export function triple(value: number) {
  return value * 3;
}
"#,
    );

    let output = async {
        let mut isolate = JsIsolate::new_for_run_entry(&entry).await?;
        let output = isolate
            .run_script(
                json!({ "value": 7 }),
                ExecutionContext::new("run-script-ts-modules"),
            )
            .await?;
        Ok::<_, anyhow::Error>(output.output)
    }
    .await
    .expect("script mode should succeed");
    assert_eq!(output, json!({ "doubled": 14, "tripled": 21 }));
}

#[tokio::test]
async fn run_script_supports_mjs_entry_with_top_level_await() {
    let temp = TempDirGuard::new("flux-run-mjs-modules");
    let entry = temp.write(
        "main.mjs",
        r#"
import { increment } from "./helper.js";

const base = await Promise.resolve(40);

export default function() {
  return { value: increment(base + 1) };
}
"#,
    );
    temp.write(
        "helper.js",
        r#"
export function increment(value) {
  return value + 1;
}
"#,
    );

    let output = async {
        let mut isolate = JsIsolate::new_for_run_entry(&entry).await?;
        let output = isolate
            .run_script(json!({}), ExecutionContext::new("run-script-mjs-modules"))
            .await?;
        Ok::<_, anyhow::Error>(output.output)
    }
    .await
    .expect("mjs script mode should succeed");
    assert_eq!(output, json!({ "value": 42 }));
}
