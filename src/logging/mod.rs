mod consoleexporter;

// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

use opentelemetry::{KeyValue, trace::TracerProvider};
use opentelemetry_appender_tracing::layer::OpenTelemetryTracingBridge;
use opentelemetry_otlp::{LogExporter as OtlpLogExporter, Protocol, WithExportConfig};
use opentelemetry_semantic_conventions::attribute::{SERVICE_NAME, SERVICE_VERSION};
use std::time::Duration;
use tracing::info;
use tracing_subscriber::{Layer, layer::SubscriberExt, util::SubscriberInitExt};

use opentelemetry_sdk::{
    Resource,
    trace::{Sampler, SdkTracerProvider},
};
pub fn init_tracing(
    enable_otel_logs: bool,
    debug: bool,
) -> Result<SdkTracerProvider, Box<dyn std::error::Error>> {
    let filter = if debug { "debug" } else { "info" };
    let filter = tracing_subscriber::EnvFilter::new(format!("opentelemetry_sdk=info,{filter}"));
    let otlp_span_exporter = opentelemetry_otlp::SpanExporter::builder()
        .with_tonic()
        .with_protocol(Protocol::HttpBinary)
        .with_timeout(Duration::from_secs(5))
        .build()
        .map_err(|err| err.to_string())?;

    let resource = Resource::builder()
        .with_attributes([
            KeyValue::new(SERVICE_NAME, "markdown-wrangler"),
            KeyValue::new(SERVICE_VERSION, env!("CARGO_PKG_VERSION")),
        ])
        .build();

    let trace_provider = SdkTracerProvider::builder()
        .with_batch_exporter(otlp_span_exporter)
        // we want *everything!*
        .with_sampler(Sampler::AlwaysOn)
        // .with_max_events_per_span(MAX_EVENTS_PER_SPAN)
        // .with_max_attributes_per_span(MAX_ATTRIBUTES_PER_SPAN)
        .with_resource(resource.clone())
        .build();
    let res = tracing_subscriber::registry().with(filter.clone()).with(
        tracing_opentelemetry::layer().with_tracer(trace_provider.tracer("markdown-wrangler")),
    );

    if enable_otel_logs {
        let exporter_name = match std::env::var("OTEL_LOGS_EXPORTER") {
            Ok(val) => val,
            Err(_) => "otlp".to_string(),
        }
        .to_lowercase();

        let log_provider =
            opentelemetry_sdk::logs::SdkLoggerProvider::builder().with_resource(resource);

        let log_provider = if exporter_name == "console" {
            log_provider
                .with_simple_exporter(consoleexporter::OurLogExporter::default())
                .build()
        } else if exporter_name == "otlp" {
            let endpoint = std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT")
                .unwrap_or("http://localhost:4318".to_string());

            log_provider
                .with_batch_exporter(
                    OtlpLogExporter::builder()
                        .with_tonic()
                        .with_endpoint(endpoint)
                        .build()
                        .map_err(|err| err.to_string())?,
                )
                .build()
        } else {
            return Err(format!("Unsupported OTEL_LOGS_EXPORTER value: {}", exporter_name).into());
        };

        let otel_log_layer =
            OpenTelemetryTracingBridge::new(&log_provider).with_filter(filter.clone());
        res.with(otel_log_layer).init();
    } else {
        res.with(tracing_subscriber::fmt::layer()).init();
    };
    Ok(trace_provider)
}

pub fn log_startup(debug: bool) {
    info!("Starting markdown-wrangler");
    if debug {
        info!("Debug mode enabled");
    }
}
