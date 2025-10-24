extern crate proc_macro;
use proc_macro::TokenStream;

mod endpoint_async;
mod service;
mod service_mux_struct;

/// The `#[service]` macro is applied to an `impl` block to automatically generate the `ServiceTrait` implementation for the type.
///
/// - When applied to an inherent `impl` block, methods intended as service methods should be marked with `#[method]`.
/// - When applied to a trait `impl` block, all methods defined in the trait will be registered as service methods.
/// - All service methods must return `Result<T, RpcError<E>>`, where `E` is a user-defined error type that implements `RpcErrCodec`.
///
/// The service method recognizes:
/// - `fn` (which is considered non-blocking)
/// - `async fn`
/// - `impl Future`
/// - trait methods wrapped by `async_trait`
#[proc_macro_attribute]
pub fn service(_attr: TokenStream, item: TokenStream) -> TokenStream {
    service::service(_attr, item)
}

/// A marker attribute for methods in an inherent `impl` block that should be exposed as RPC methods.
/// This is not needed when using a trait-based implementation.
#[proc_macro_attribute]
pub fn method(_attr: TokenStream, item: TokenStream) -> TokenStream {
    item
}

/// The `#[service_mux_struct]` macro is applied to a **struct** to implement `ServiceTrait` on it.
/// It acts as a dispatcher, routing `serve()` calls to the correct service based on the `req.service` field.
///
/// Each field in the struct must hold a service that implements `ServiceTrait` (e.g., wrapped in an `Arc`).
/// The macro generates a `serve` implementation that matches `req.service` against the field names of the struct.
#[proc_macro_attribute]
pub fn service_mux_struct(_attr: TokenStream, item: TokenStream) -> TokenStream {
    service_mux_struct::service_mux_struct(_attr, item)
}

/// The `#[endpoint_async]` macro applies to a trait to generate a client for remote api calls
/// for async context.
///
/// NOTE: the generated struct and interface will appear in your `cargo doc`.
///
/// Rules:
///     - trait can be wrapped async_trait, optionally.
///     - If trait is not async_trait, then the method should use the signature `impl Future + Send`
///     - No fn is allowed.
///     - All method should have one and only argument, and return type should be `Result<Resp, RpcError<E>>`, where E impls [RpcErrCodec]
/// Usage:
/// ```rust
/// use occams_rpc_api_macros::endpoint_async;
/// use occams_rpc_core::error::RpcError;
/// use nix::errno::Errno;
/// #[endpoint_async(DemoClient)]
/// pub trait DemoService {
///     async fn foo(&self, _empty: ()) -> Result<(), RpcError<Errno>>;
/// }
/// ```
///
/// This will generate a `DemoClient` struct that implements the `DemoService` trait,
/// providing async methods that call the RPC endpoint. The DemoClient type will have a generate param
/// `<C: ClientCaller<Factory: ClientFact<Task = APIClientReq>>>`,
/// you can initialize it with `DemoClient::new(client_caller)`
#[proc_macro_attribute]
pub fn endpoint_async(attr: TokenStream, item: TokenStream) -> TokenStream {
    endpoint_async::endpoint_async(attr, item)
}
