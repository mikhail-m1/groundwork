use std::{
    ops::DerefMut,
    sync::{Arc, Mutex},
    task::Poll,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use poem::{Body, Endpoint, IntoResponse, Middleware, Response};
use serde::Serialize;
use tokio::io::AsyncRead;

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Call {
    pub timestamp_ms: u64,
    pub duration_us: u64,
    pub path: String,
    pub response: CallResponse,
}

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub enum CallResponse {
    Ok { length: usize },
    Error { code: u16 },
}

pub type Buffer<const SIZE: usize> = circular_buffer::CircularBuffer<SIZE, Call>;

pub type BufferRef<const SIZE: usize> = Arc<Mutex<Buffer<SIZE>>>;

#[derive(Clone)]
pub struct CallMiddleware<const SIZE: usize> {
    calls: BufferRef<SIZE>,
}

impl<const SIZE: usize> Default for CallMiddleware<SIZE> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const SIZE: usize> CallMiddleware<SIZE> {
    pub fn new() -> Self {
        Self {
            calls: Arc::new(Mutex::new(Buffer::new())),
        }
    }

    pub fn get(&self) -> BufferRef<SIZE> {
        self.calls.clone()
    }
}

impl Call {
    pub fn successfull(
        timestamp_ms: u64,
        duration: Duration,
        path: String,
        reponse_length: usize,
    ) -> Self {
        Self {
            timestamp_ms,
            duration_us: duration.as_micros() as u64,
            path,
            response: CallResponse::Ok {
                length: reponse_length,
            },
        }
    }

    pub fn error(timestamp_ms: u64, duration: Duration, path: String, response_code: u16) -> Self {
        Self {
            timestamp_ms,
            duration_us: duration.as_micros() as u64,
            path,
            response: CallResponse::Error {
                code: response_code,
            },
        }
    }
}

pub struct CallMiddlewareImpl<const SIZE: usize, E: Endpoint> {
    endpoint: E,
    calls: BufferRef<SIZE>,
}

impl<const SIZE: usize, E: Endpoint> Middleware<E> for CallMiddleware<SIZE> {
    type Output = CallMiddlewareImpl<SIZE, E>;

    fn transform(&self, ep: E) -> Self::Output {
        CallMiddlewareImpl {
            endpoint: ep,
            calls: self.calls.clone(),
        }
    }
}

impl<const SIZE: usize, E: Endpoint> Endpoint for CallMiddlewareImpl<SIZE, E> {
    type Output = Response;

    async fn call(&self, request: poem::Request) -> poem::Result<Self::Output> {
        let path = request.original_uri().to_string();
        let timestamp_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .as_ref()
            .map(Duration::as_millis)
            .unwrap_or(0) as u64;
        let now = Instant::now();
        let res = self.endpoint.call(request).await;
        let duration = now.elapsed();
        match res {
            Ok(response) => {
                let r = response.into_response();
                let (parts, body) = r.into_parts();
                let async_read = body.into_async_read();
                Ok(Response::from_parts(
                    parts,
                    Body::from_async_read(BodyReader {
                        timestamp_ms,
                        duration,
                        path,
                        calls: self.calls.clone(),
                        wrapped: async_read,
                        length: 0,
                    }),
                ))
                // let mut guard = self.calls.lock().expect("can lock mutex");
                // guard.push_back(Call::successfull(timestamp_ms, duration, path, body.len()));
                // Ok(Response::from_parts(parts, Body::from_bytes(body)))
            }
            Err(err) => {
                let mut guard = self.calls.lock().expect("can lock mutex");
                guard.push_back(Call::error(
                    timestamp_ms,
                    duration,
                    path,
                    err.status().as_u16(),
                ));
                Err(err)
            }
        }
    }
}

struct BodyReader<const SIZE: usize, T: AsyncRead + Unpin> {
    wrapped: T,
    timestamp_ms: u64,
    duration: Duration,
    path: String,
    calls: BufferRef<SIZE>,
    length: usize,
}

impl<const SIZE: usize, T: AsyncRead + Unpin> AsyncRead for BodyReader<SIZE, T> {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        let initial = buf.filled().len();
        let r = unsafe { std::pin::Pin::new_unchecked(&mut self.deref_mut().wrapped) }
            .poll_read(cx, buf);
        if let Poll::Ready(Err(_)) = &r {
            let mut path = String::new();
            std::mem::swap(&mut path, &mut self.path);
            self.calls.lock().expect("can lock").push_back(Call::error(
                self.timestamp_ms,
                self.duration,
                path,
                u16::MAX,
            ));
            return r;
        }
        match buf.filled().len() - initial {
            0 => {
                let mut path = String::new();
                std::mem::swap(&mut path, &mut self.path);
                self.calls
                    .lock()
                    .expect("can lock")
                    .push_back(Call::successfull(
                        self.timestamp_ms,
                        self.duration,
                        path,
                        self.length,
                    ));
            }
            v => self.length += v,
        }
        r
    }
}
