// SPDX-License-Identifier: GPL-3.0-or-later
//! Throwaway spike: does a plain HTTPS client get usage JSON from claude.ai?
//!
//! Run in your own terminal: `cargo run --example spike`
//! Paste the value of the `sessionKey` cookie (claude.ai → DevTools → Application → Cookies).
//! The key is never printed or saved. Response bodies are written, redacted, to tests/fixtures/.

use serde_json::Value;
use std::io::{self, BufRead, Write};
use std::time::Duration;

const UA: &str = concat!("ClaudeUsageWidget/", env!("CARGO_PKG_VERSION"));

fn main() {
    print!("Paste sessionKey cookie value: ");
    io::stdout().flush().unwrap();
    let mut key = String::new();
    io::stdin().lock().read_line(&mut key).unwrap();
    let key = key.trim().to_string();
    if key.is_empty() {
        eprintln!("No key entered.");
        return;
    }

    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(15)))
        .http_status_as_error(false)
        .build()
        .into();

    let get = |url: &str| -> (u16, String, String) {
        match agent
            .get(url)
            .header("Cookie", &format!("sessionKey={key}"))
            .header("User-Agent", UA)
            .header("Accept", "application/json")
            .call()
        {
            Ok(mut r) => {
                let status = r.status().as_u16();
                let ct = r
                    .headers()
                    .get("content-type")
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("")
                    .to_string();
                let body = r.body_mut().read_to_string().unwrap_or_default();
                (status, ct, body)
            }
            Err(e) => (0, String::new(), format!("transport error: {e}")),
        }
    };

    let report = |name: &str, (status, ct, body): &(u16, String, String)| {
        let challenge = body.contains("cf-chl") || body.contains("Just a moment");
        println!("\n== {name}\nstatus: {status}\ncontent-type: {ct}\ncloudflare challenge: {challenge}");
    };

    let orgs = get("https://claude.ai/api/organizations");
    report("GET /api/organizations", &orgs);
    let Ok(orgs_json) = serde_json::from_str::<Value>(&orgs.2) else {
        println!("Body is not JSON (first 200 chars): {}", &orgs.2.chars().take(200).collect::<String>());
        return;
    };
    save("tests/fixtures/orgs.json", &orgs_json);

    let Some(list) = orgs_json.as_array() else { return };
    println!("organizations: {}", list.len());
    for (i, org) in list.iter().enumerate() {
        let caps = org.get("capabilities").cloned().unwrap_or(Value::Null);
        println!("  org #{i} capabilities: {caps}");
        let Some(uuid) = org.get("uuid").and_then(|v| v.as_str()) else { continue };
        let usage = get(&format!("https://claude.ai/api/organizations/{uuid}/usage"));
        report(&format!("GET usage for org #{i}"), &usage);
        match serde_json::from_str::<Value>(&usage.2) {
            Ok(v) => {
                println!("usage JSON (redacted):\n{}", serde_json::to_string_pretty(&redact(v.clone())).unwrap());
                save(&format!("tests/fixtures/usage_org{i}.json"), &v);
            }
            Err(_) => println!("Body is not JSON (first 200 chars): {}", usage.2.chars().take(200).collect::<String>()),
        }
    }
}

fn save(path: &str, v: &Value) {
    std::fs::write(path, serde_json::to_string_pretty(&redact(v.clone())).unwrap()).unwrap();
    println!("saved {path}");
}

/// Replace identifying strings (ids, names, emails, uuids) with placeholders.
fn redact(v: Value) -> Value {
    match v {
        Value::Object(m) => Value::Object(
            m.into_iter()
                .map(|(k, v)| {
                    let kl = k.to_lowercase();
                    let sensitive = ["uuid", "id", "name", "email", "billing", "phone"]
                        .iter()
                        .any(|s| kl == *s || kl.ends_with(&format!("_{s}")) || kl.starts_with(&format!("{s}_")));
                    let v = if sensitive && v.is_string() {
                        Value::String(format!("REDACTED_{}", k.to_uppercase()))
                    } else {
                        redact(v)
                    };
                    (k, v)
                })
                .collect(),
        ),
        Value::Array(a) => Value::Array(a.into_iter().map(redact).collect()),
        Value::String(s) if looks_like_uuid(&s) => Value::String("REDACTED_UUID".into()),
        Value::String(s) if s.contains('@') => Value::String("REDACTED_EMAIL".into()),
        other => other,
    }
}

fn looks_like_uuid(s: &str) -> bool {
    s.len() == 36 && s.chars().enumerate().all(|(i, c)| if [8, 13, 18, 23].contains(&i) { c == '-' } else { c.is_ascii_hexdigit() })
}
