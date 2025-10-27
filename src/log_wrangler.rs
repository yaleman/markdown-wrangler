// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

use std::time::Duration;

use opentelemetry::{KeyValue, trace::TracerProvider};
use opentelemetry_otlp::{Protocol, WithExportConfig};
use opentelemetry_semantic_conventions::attribute::{SERVICE_NAME, SERVICE_VERSION};
use tracing::info;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use opentelemetry_sdk::{
    Resource,
    trace::{Sampler, SdkTracerProvider},
};
pub fn init_tracing(debug: bool) -> Result<SdkTracerProvider, Box<dyn std::error::Error>> {
    let filter = if debug { "debug" } else { "info" };

    let otlp_exporter = opentelemetry_otlp::SpanExporter::builder()
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

    let provider = SdkTracerProvider::builder()
        .with_batch_exporter(otlp_exporter)
        // we want *everything!*
        .with_sampler(Sampler::AlwaysOn)
        // .with_max_events_per_span(MAX_EVENTS_PER_SPAN)
        // .with_max_attributes_per_span(MAX_ATTRIBUTES_PER_SPAN)
        .with_resource(resource)
        .build();
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new(filter))
        .with(tracing_subscriber::fmt::layer())
        .with(tracing_opentelemetry::layer().with_tracer(provider.tracer("markdown-wrangler")))
        .init();

    Ok(provider)
}

pub fn log_startup(debug: bool) {
    info!("Starting markdown-wrangler");
    if debug {
        info!("Debug mode enabled");
    }
}
