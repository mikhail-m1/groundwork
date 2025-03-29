use alloc_metrics::MetricAlloc;
use poem::{EndpointExt, Route, Server, listener::TcpListener, middleware::Tracing};
use poem_openapi::{OpenApi, OpenApiService, param::Query, payload::PlainText};

// Add this two lines to have allocator statistics
#[global_allocator]
static GLOBAL: MetricAlloc<std::alloc::System> = MetricAlloc::new(std::alloc::System);

#[tokio::main]
pub async fn main() -> Result<(), std::io::Error> {
    // The name is shown on the top of status page
    let groundwork = groundwork::DefaultGroundwork::new("Hello world");

    // You can register default groundwork tracer to stdout
    // groundwork.register_stdout_tracing_subscriber();

    // Or your own tracer with groundwork proxy
    tracing_subscriber::fmt::Subscriber::builder()
        .with_writer(groundwork.trace_writer(tracing_subscriber::fmt::TestWriter::new()))
        .with_ansi(false)
        .init();

    log::set_max_level(log::LevelFilter::Trace);

    let api_service =
        OpenApiService::new(Api, "Hello World", "1.0").server("http://localhost:8080/api");
    let ui = api_service.swagger_ui();

    let route = groundwork
        .register_handlers(Route::new(), "/status")
        .nest("/api", api_service.with(groundwork.calls_middleware())) // this is the way to trace only some calls
        .nest("/", ui)
        // .with(groundwork.calls_middleware()) // this line enable tracing for all calls
        .with(Tracing);

    Server::new(TcpListener::bind("127.0.0.1:8080"))
        .run(route)
        .await
}

struct Api;

#[OpenApi]
impl Api {
    #[oai(path = "/hello", method = "get")]
    async fn index(&self, name: Query<Option<String>>) -> PlainText<String> {
        match name.0 {
            Some(name) => PlainText(format!("hello, {name}!")),
            None => PlainText("hello!".to_string()),
        }
    }
}
