extern crate proc_macro;

use proc_macro::TokenStream;

/// Fory wire-schema attribute for tarpc services.
///
/// Apply AFTER `#[tarpc::service]` on a trait definition. Parses the
/// tarpc-generated `XxxRequest` / `XxxResponse` enums from the expanded
/// token stream and emits:
///
/// - Manual `fory::Serializer` + `fory::ForyDefault` impls (EXT path)
/// - `pub struct XxxService;` marker
/// - `impl tarpc_fory::ServiceWireSchema for XxxService`
/// - User-type registration calls via `fory.register()`
///
/// # Example
///
/// ```ignore
/// #[tarpc::service]
/// #[tarpc_fory::fory_service]
/// trait Hello {
///     async fn hello(name: String) -> String;
/// }
/// ```
#[proc_macro_attribute]
pub fn fory_service(_attr: TokenStream, input: TokenStream) -> TokenStream {
    // Phase: skeleton — pass-through. Task 2 fills in the real logic.
    input
}
