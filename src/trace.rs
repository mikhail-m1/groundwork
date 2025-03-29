use log::{Metadata, Record};
use serde::Serialize;
use std::io::Write;
use std::{
    sync::{Arc, Mutex},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use thiserror::Error;

pub struct Buffer<const SIZE: usize>(circular_buffer::CircularBuffer<SIZE, u8>);

pub struct SpyLogger<const SIZE: usize, T: log::Log> {
    buffer: Arc<Mutex<Buffer<SIZE>>>,
    logger: T,
}
pub const DEFAULT_BUFFER_SIZE: usize = 64 * 1024;

pub type SpyLoggerDefault<T> = SpyLogger<DEFAULT_BUFFER_SIZE, T>;

impl<const SIZE: usize, T: log::Log> SpyLogger<SIZE, T> {
    pub fn new(logger: T) -> Self {
        Self {
            buffer: Arc::new(Mutex::new(Buffer::new())),
            logger,
        }
    }

    pub fn buffer(&self) -> Arc<Mutex<Buffer<SIZE>>> {
        self.buffer.clone()
    }
}

#[derive(Serialize, Debug)]
pub struct LogLine {
    timestamp: Duration,
    level: u8,
    message: String,
}

#[derive(Error, Debug)]
pub enum LogError {
    #[error("Found unexpected character on timestamp place")]
    UnexpectedTimestampValue,
    #[error("Unexpected end")]
    ValueExpected,
    #[error("Cannot restore message from bytes")]
    Utf8(#[from] std::string::FromUtf8Error),
}

impl<const SIZE: usize> Default for Buffer<SIZE> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const SIZE: usize> Buffer<SIZE> {
    pub fn new() -> Self {
        Self(circular_buffer::CircularBuffer::new())
    }

    pub fn get_logs(&mut self) -> Result<Vec<LogLine>, LogError> {
        let mut it = self.0.iter();
        let mut result = vec![];
        let mut add_result = |timestamp, level, message| -> Result<(), LogError> {
            result.push(LogLine {
                timestamp: Duration::from_secs(timestamp),
                level,
                message: String::from_utf8(message).map_err(LogError::Utf8)?,
            });
            Ok(())
        };
        if it.any(|&v| v == 0) {
            'top: loop {
                let mut timestamp = 0u64;
                for _ in 0..16 {
                    timestamp =
                        (timestamp << 4) | read_hex(*it.next().ok_or(LogError::ValueExpected)?)?;
                }
                let level = read_hex(*it.next().ok_or(LogError::ValueExpected)?)? as u8;
                let mut message = vec![];
                for &c in it.by_ref() {
                    if c == 0 {
                        add_result(timestamp, level, message)?;
                        continue 'top;
                    }
                    message.push(c);
                }
                add_result(timestamp, level, message)?;
                break;
            }
        }
        Ok(result)
    }

    pub fn get_traces(&mut self) -> Result<Vec<String>, LogError> {
        let mut it = self.0.iter();
        let mut result = vec![];
        if it.any(|&v| v == 0) {
            'top: loop {
                let mut message = vec![];
                for &v in it.by_ref() {
                    if v != 0 {
                        message.push(v);
                    } else {
                        result.push(String::from_utf8(message).map_err(LogError::Utf8)?);
                        continue 'top;
                    }
                }
                result.push(String::from_utf8(message).map_err(LogError::Utf8)?);
                break;
            }
        }
        Ok(result)
    }

    fn write_log(&mut self, level: log::Level, message: &str) -> std::fmt::Result {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| std::fmt::Error)?
            .as_secs();
        self.0
            .write_all(format!("\0{timestamp:016X}{}", level as u8).as_bytes())
            .map_err(|_| std::fmt::Error)?;
        self.0
            .write_all(message.as_bytes())
            .map_err(|_| std::fmt::Error)
    }

    fn write_trace(&mut self, message: &str) -> std::io::Result<()> {
        use std::io::Write;
        self.0.write_all(&[0u8])?;
        self.0.write_all(message.as_bytes())
    }
}

fn read_hex(v: u8) -> Result<u64, LogError> {
    Ok((match v {
        b'0'..=b'9' => v - b'0',
        b'A'..=b'F' => v - b'A' + 10,
        _ => Err(LogError::UnexpectedTimestampValue)?,
    }) as u64)
}

impl<const SIZE: usize, T: log::Log> log::Log for SpyLogger<SIZE, T> {
    fn enabled(&self, metadata: &Metadata) -> bool {
        self.logger.enabled(metadata)
    }

    fn log(&self, record: &Record) {
        if self.enabled(record.metadata()) {
            let mut m = self.buffer.lock().expect("can lock buffer mutex");
            _ = m.write_log(record.level(), &format!("{}", record.args()));
            self.logger.log(record);
        }
    }
    fn flush(&self) {
        self.logger.flush();
    }
}

pub struct StdoutTraceWriterMaker<const SIZE: usize> {
    buffer: Arc<Mutex<Buffer<SIZE>>>,
}

pub struct TraceWriter<const SIZE: usize> {
    buffer: Arc<Mutex<Buffer<SIZE>>>,
}

impl<const SIZE: usize> StdoutTraceWriterMaker<SIZE> {
    pub fn new(buffer: Arc<Mutex<Buffer<SIZE>>>) -> Self {
        Self { buffer }
    }
}

impl<'a, const SIZE: usize> tracing_subscriber::fmt::MakeWriter<'a>
    for StdoutTraceWriterMaker<SIZE>
{
    type Writer = TraceWriter<SIZE>;

    fn make_writer(&'a self) -> Self::Writer {
        TraceWriter {
            buffer: self.buffer.clone(),
        }
    }
}

impl<const SIZE: usize> std::io::Write for TraceWriter<SIZE> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.buffer
            .lock()
            .unwrap()
            .write_trace(&String::from_utf8_lossy(buf))?;
        std::io::stdout().write(buf)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

pub struct TraceWriterWrapperMaker<
    const SIZE: usize,
    T: for<'a> tracing_subscriber::fmt::MakeWriter<'a>,
> {
    buffer: Arc<Mutex<Buffer<SIZE>>>,
    maker: T,
}

pub struct TraceWriterWrapper<const SIZE: usize, T: std::io::Write> {
    buffer: Arc<Mutex<Buffer<SIZE>>>,
    writer: T,
}

impl<const SIZE: usize, T: for<'a> tracing_subscriber::fmt::MakeWriter<'a>>
    TraceWriterWrapperMaker<SIZE, T>
{
    pub fn new(buffer: Arc<Mutex<Buffer<SIZE>>>, maker: T) -> Self {
        Self { buffer, maker }
    }
}

impl<'a, const SIZE: usize, T: for<'b> tracing_subscriber::fmt::MakeWriter<'b>>
    tracing_subscriber::fmt::MakeWriter<'a> for TraceWriterWrapperMaker<SIZE, T>
{
    type Writer = TraceWriterWrapper<SIZE, <T as tracing_subscriber::fmt::MakeWriter<'a>>::Writer>;

    fn make_writer(&'a self) -> Self::Writer {
        Self::Writer {
            buffer: self.buffer.clone(),
            writer: self.maker.make_writer(),
        }
    }
}

impl<const SIZE: usize, T: std::io::Write> std::io::Write for TraceWriterWrapper<SIZE, T> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.buffer
            .lock()
            .unwrap()
            .write_trace(&String::from_utf8_lossy(buf))?;
        self.writer.write(buf)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
