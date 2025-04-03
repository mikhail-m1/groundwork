# Groundwork

Groundwork is a library that provides a status page for your Rust process.

The status page displays information about the process, including:

* Memory usage
* Allocator usage
* CPU usage
* File descriptors
* Sockets
* Logs / tracing output
* API Calls information

Currently, only [Poem](https://github.com/poem-web/poem) is supported. However, adding support for Axum and other web frameworks should be straightforward. If your service doesn't integrate with any web frameworks, integrating Poem is relatively simple.

**Note:** Only Linux and macOS are supported at this time.

## Getting Started

To begin, add next dependencies to your `Cargo.toml` file:
```
groundwork = "0.1"
alloc-metrics = "0.1"
```
And copy the necessary lines from the [hello_world example](https://github.com/mikhail-m1/groundwork/blob/main/examples/hello_world.rs).

## Screenshots

<img width="884" alt="Image" src="https://github.com/user-attachments/assets/abf3fc3a-4bb4-415a-9765-cee5a92c13b6" />

<img width="882" alt="Image" src="https://github.com/user-attachments/assets/b88aac28-524e-4639-8b7f-330aac542e0f" />

<img width="883" alt="Image" src="https://github.com/user-attachments/assets/624fab23-4d0c-4006-838d-2a2978c08c30" />

<img width="884" alt="Image" src="https://github.com/user-attachments/assets/e47b6249-2188-44b1-9da5-6e2d5c05ff90" />
