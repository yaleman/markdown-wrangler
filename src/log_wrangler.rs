// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

use opentelemetry::trace::TracerProvider;
use tracing::info;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

pub fn init_tracing(debug: bool) -> Result<(), Box<dyn std::error::Error>> {
    let filter = if debug { "debug" } else { "info" };

    if debug {
        let tracer = opentelemetry_sdk::trace::SdkTracerProvider::builder()
            .build()
            .tracer("markdown-wrangler");

        tracing_subscriber::registry()
            .with(tracing_subscriber::EnvFilter::new(filter))
            .with(tracing_subscriber::fmt::layer())
            .with(tracing_opentelemetry::layer().with_tracer(tracer))
            .init();
    } else {
        tracing_subscriber::registry()
            .with(tracing_subscriber::EnvFilter::new(filter))
            .with(tracing_subscriber::fmt::layer())
            .init();
    }

    Ok(())
}

pub fn log_startup(debug: bool) {
    info!("Starting markdown-wrangler");
    if debug {
        info!("Debug mode enabled");
    }
}
