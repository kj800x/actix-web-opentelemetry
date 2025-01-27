use actix_web::{web, App, HttpRequest, HttpServer};
use actix_web_opentelemetry::{PrometheusMetricsHandler, RequestMetrics, RequestTracing};
use opentelemetry::{global, KeyValue};
use opentelemetry_sdk::{
    metrics::{Aggregation, Instrument, MeterProvider, Stream},
    propagation::TraceContextPropagator,
    runtime::TokioCurrentThread,
    Resource,
};

async fn index(_req: HttpRequest, _path: actix_web::web::Path<String>) -> &'static str {
    "Hello world!"
}

#[actix_web::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Start a new jaeger trace pipeline
    global::set_text_map_propagator(TraceContextPropagator::new());
    let _tracer = opentelemetry_jaeger::new_agent_pipeline()
        .with_service_name("actix_server")
        .install_batch(TokioCurrentThread)
        .expect("pipeline install error");

    // Start a new prometheus metrics pipeline if --features metrics-prometheus is used
    #[cfg(feature = "metrics-prometheus")]
    let (metrics_handler, meter_provider) = {
        let registry = prometheus::Registry::new();
        let exporter = opentelemetry_prometheus::exporter()
            .with_registry(registry.clone())
            .build()?;
        let provider = MeterProvider::builder()
            .with_reader(exporter)
            .with_resource(Resource::new([KeyValue::new("service.name", "my_app")]))
            .with_view(
                opentelemetry_sdk::metrics::new_view(
                    Instrument::new().name("http.server.duration"),
                    Stream::new().aggregation(Aggregation::ExplicitBucketHistogram {
                        boundaries: vec![
                            0.0, 0.005, 0.01, 0.025, 0.05, 0.075, 0.1, 0.25, 0.5, 0.75, 1.0, 2.5,
                            5.0, 7.5, 10.0,
                        ],
                        record_min_max: true,
                    }),
                )
                .unwrap(),
            )
            .build();
        global::set_meter_provider(provider.clone());

        (PrometheusMetricsHandler::new(registry), provider)
    };

    HttpServer::new(move || {
        let app = App::new()
            .wrap(RequestTracing::new())
            .wrap(RequestMetrics::default())
            .service(web::resource("/users/{id}").to(index));

        #[cfg(feature = "metrics-prometheus")]
        let app = app.route("/metrics", web::get().to(metrics_handler.clone()));

        app
    })
    .bind("127.0.0.1:8080")?
    .run()
    .await?;

    // Ensure all spans have been reported
    global::shutdown_tracer_provider();

    #[cfg(feature = "metrics-prometheus")]
    meter_provider.shutdown()?;

    Ok(())
}
