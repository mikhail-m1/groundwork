use std::sync::{Arc, Mutex};

use poem::error::InternalServerError;
use poem::middleware::AddData;
use poem::web::{Data, Html, WithContentType};
use poem::{EndpointExt, IntoResponse};
use poem::{Result, handler};
use poem::{Route, get};
use trace::Buffer;
use tracing_subscriber::fmt::MakeWriter;
pub mod call;
pub mod descriptors;
pub mod stat;
pub mod trace;

pub type DefaultGroundwork = Groundwork<{ trace::DEFAULT_BUFFER_SIZE }, 100>;

pub struct Groundwork<const LOG_SIZE: usize, const CALL_SIZE: usize> {
    stats_data: Arc<stat::StatsData>,
    logs: Arc<Mutex<Buffer<LOG_SIZE>>>,
    calls_middleware: call::CallMiddleware<CALL_SIZE>,
}

impl<const LOG_SIZE: usize, const CALL_SIZE: usize> Groundwork<LOG_SIZE, CALL_SIZE> {
    pub fn new(name: &str) -> Self {
        Self {
            stats_data: Arc::new(stat::StatsData::new(name)),
            logs: Arc::new(Mutex::new(Buffer::new())),
            calls_middleware: call::CallMiddleware::new(),
        }
    }

    pub fn register_handlers(&self, route: Route, page_path: &str) -> Route {
        route
            .at(
                "/groundwork/stats",
                get(stat::stats).with(AddData::new(self.stats_data.clone())),
            )
            .at(
                "/groundwork/logs",
                get(logs).with(AddData::new(self.logs.clone())),
            )
            .at(
                "/groundwork/calls",
                get(calls).with(AddData::new(self.calls_middleware().get())),
            )
            .at("/groundwork/descriptors", get(descriptors::descriptors))
            .at("/groundwork/w3.css", css)
            .at(page_path, index)
    }

    pub fn register_stdout_tracing_subscriber(&self) {
        tracing_subscriber::fmt::Subscriber::builder()
            .with_ansi(false)
            .with_writer(self.trace_writer_stdout())
            .init();
    }

    pub fn trace_writer_stdout(&self) -> impl for<'a> MakeWriter<'a> + 'static {
        trace::StdoutTraceWriterMaker::new(self.logs.clone())
    }

    pub fn trace_writer<W>(&self, writer: W) -> impl for<'a> MakeWriter<'a> + 'static
    where
        W: for<'a> MakeWriter<'a> + 'static,
    {
        trace::TraceWriterWrapperMaker::new(self.logs.clone(), writer)
    }

    pub fn calls_middleware(&self) -> call::CallMiddleware<CALL_SIZE> {
        self.calls_middleware.clone()
    }
}

#[handler]
fn logs(
    buffer: Data<&std::sync::Arc<std::sync::Mutex<trace::Buffer<{ trace::DEFAULT_BUFFER_SIZE }>>>>,
) -> Result<String> {
    serde_json::to_string(&buffer.lock().unwrap().get_traces().unwrap())
        .map_err(InternalServerError)
}

// FIXME convert to implementation of Endpoint
#[handler]
fn calls(buffer: Data<&call::BufferRef<100>>) -> Result<String> {
    serde_json::to_string(&buffer.lock().unwrap().iter().collect::<Vec<_>>())
        .map_err(InternalServerError)
}

#[handler]
fn css() -> WithContentType<&'static str> {
    include_str!("w3.css").with_content_type("text/css")
}

#[handler]
fn index() -> Html<&'static str> {
    Html(include_str!("index.html"))
}
