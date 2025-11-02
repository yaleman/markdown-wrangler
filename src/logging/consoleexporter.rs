//! An OpenTelemetry exporter that writes Logs to stdout on export, based on `opentelemetry-stdout` crate.
//!
use opentelemetry::logs::AnyValue;
use opentelemetry_sdk::error::{OTelSdkError, OTelSdkResult};
use opentelemetry_sdk::logs::LogBatch;
use opentelemetry_semantic_conventions::SCHEMA_URL;
use opentelemetry_semantic_conventions::attribute::OTEL_STATUS_CODE;
use opentelemetry_semantic_conventions::resource::{OTEL_SCOPE_NAME, OTEL_SCOPE_VERSION};
use serde_json::{Value, json};

use super::*;
use chrono::{DateTime, Utc};
use std::collections::HashMap;
use std::sync::atomic::{self, Ordering};
use std::{fmt, time};

pub struct OurLogExporter {
    resource: Resource,
    is_shutdown: atomic::AtomicBool,
    #[allow(dead_code)]
    resource_emitted: atomic::AtomicBool,
}

impl Default for OurLogExporter {
    fn default() -> Self {
        OurLogExporter {
            resource: Resource::builder()
                .with_service_name("markdown_wrangler")
                .build(),
            is_shutdown: atomic::AtomicBool::new(false),
            resource_emitted: atomic::AtomicBool::new(false),
        }
    }
}

impl fmt::Debug for OurLogExporter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("LogExporter")
    }
}

impl opentelemetry_sdk::logs::LogExporter for OurLogExporter {
    /// Export logs to stdout
    async fn export(&self, batch: LogBatch<'_>) -> OTelSdkResult {
        if self.is_shutdown.load(atomic::Ordering::SeqCst) {
            Err(OTelSdkError::AlreadyShutdown)
        } else {
            if self
                .resource_emitted
                .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
            {
                //     if let Some(schema_url) = self.resource.schema_url() {
                //     println!("\t Resource SchemaUrl: {schema_url:?}");
                // }
                // self.resource.iter().for_each(|(k, v)| {
                //     println!("\t ->  {k}={v:?}");
                // });
            }
            print_logs(batch);

            Ok(())
        }
    }

    fn shutdown_with_timeout(&self, _timeout: time::Duration) -> OTelSdkResult {
        self.is_shutdown.store(true, atomic::Ordering::SeqCst);
        Ok(())
    }

    fn set_resource(&mut self, res: &opentelemetry_sdk::Resource) {
        self.resource = res.clone();
    }
}

fn print_logs(batch: LogBatch<'_>) {
    for (i, log) in batch.iter().enumerate() {
        let (record, library) = log;

        let mut event: HashMap<&str, Value> = HashMap::from_iter([("event.index", i.into())]);
        let mut scope: HashMap<&str, Value> = HashMap::new();
        if !library.name().is_empty() {
            scope.insert(OTEL_SCOPE_NAME, Value::String(library.name().to_string()));
        }
        if let Some(version) = library.version() {
            scope.insert(OTEL_SCOPE_VERSION, Value::String(version.to_string()));
        }
        if let Some(schema_url) = library.schema_url() {
            scope.insert(SCHEMA_URL, Value::String(schema_url.to_string()));
        }
        if library.attributes().count() > 0 {
            scope.insert(
                "attributes",
                Value::Object(
                    library
                        .attributes()
                        .map(|kv| {
                            (
                                kv.key.as_str().to_string(),
                                Value::String(format!("{:?}", kv.value)),
                            )
                        })
                        .collect(),
                ),
            );
        }

        if !scope.is_empty() {
            event.insert("log.instrumentation_scope", json!(scope));
        }

        if let Some(event_name) = record.event_name() {
            event.insert("log.event_name", Value::String(event_name.to_string()));
        }
        if let Some(target) = record.target() {
            event.insert("log.scope", Value::String(target.to_string()));
        }
        if let Some(trace_context) = record.trace_context() {
            event.insert(
                "trace.id",
                Value::String(format!("{:?}", trace_context.trace_id)),
            );
            event.insert("span.id", Value::String(trace_context.span_id.to_string()));
            if let Some(trace_flags) = trace_context.trace_flags {
                event.insert("trace.flags", Value::String(format!("{:?}", trace_flags)));
            }
        }
        if let Some(timestamp) = record.timestamp() {
            let datetime: DateTime<Utc> = timestamp.into();
            event.insert(
                "timestamp",
                Value::String(datetime.format("%Y-%m-%dT%H:%M:%S%.6fZ").to_string()),
            );
        }
        if let Some(timestamp) = record.observed_timestamp() {
            let datetime: DateTime<Utc> = timestamp.into();
            event.insert(
                "observed_timestamp",
                Value::String(datetime.format("%Y-%m-%dT%H:%M:%S%.6fZ").to_string()),
            );
        }
        if let Some(sevnum) = record.severity_number().map(|n| n as isize) {
            if sevnum > 16 {
                event.insert(OTEL_STATUS_CODE, Value::String("Error".to_string()));
            } else {
                event.insert(OTEL_STATUS_CODE, Value::String("Ok".to_string()));
            }
        }
        if let Some(severity) = record.severity_text() {
            event.insert("severity.text", Value::String(severity.to_string()));
        }
        if let Some(body) = record.body() {
            match body {
                AnyValue::Int(int_value) => event.insert("msg", Value::Number((*int_value).into())),
                AnyValue::Double(double_value) => {
                    event.insert("msg", Value::Number((*double_value as usize).into()))
                }
                AnyValue::String(string_value) => {
                    event.insert("msg", Value::String(string_value.to_string()))
                }
                AnyValue::Boolean(bool_value) => event.insert("msg", Value::Bool(*bool_value)),
                AnyValue::Bytes(items) => event.insert(
                    "msg",
                    Value::String(format!("Bytes({}): {:?}", items.len(), items)),
                ),
                AnyValue::ListAny(any_values) => {
                    let values: Vec<String> =
                        (*any_values).iter().map(|v| format!("{v:?}")).collect();
                    event.insert(
                        "msg",
                        Value::String(format!("ListAny({}): {:?}", values.len(), values)),
                    )
                }
                AnyValue::Map(hash_map) => event.insert(
                    "msg",
                    Value::Object(
                        (*hash_map)
                            .iter()
                            .map(|(k, v)| (k.to_string(), Value::String(format!("{v:?}"))))
                            .collect(),
                    ),
                ),
                _ => todo!(),
            };
        }

        if record.attributes_iter().count() > 0 {
            event.insert(
                "attributes",
                Value::Object(
                    record
                        .attributes_iter()
                        .map(|(k, v)| (k.as_str().to_string(), Value::String(format!("{v:?}"))))
                        .collect(),
                ),
            );
        }
        println!(
            "{}",
            serde_json::to_string(&event)
                .unwrap_or(format!("Failed to serialize event: {:?}", event))
        )
    }
}
