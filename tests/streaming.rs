// Integration test: verify that #[fory_service] correctly parses
// #[streaming(server|client|bidirectional)] attributes and emits the
// streaming_methods() metadata on the service marker struct.
//
// Strategy: identical to basic.rs — apply #[fory_service] WITHOUT
// #[tarpc::service] and manually declare the matching XxxRequest / XxxResponse
// enums. This isolates the macro from the tarpc fork.

use tarpc_fory_macros::fory_service;

// -----------------------------------------------------------------------
// Stub types used in method signatures.
// -----------------------------------------------------------------------

#[derive(Debug, Clone, fory::ForyObject)]
pub struct Entry {
    name: String,
}

#[derive(Debug, Clone, fory::ForyObject)]
pub struct UploadResult {
    ok: bool,
}

#[derive(Debug, Clone, fory::ForyObject)]
pub struct Data {
    payload: Vec<u8>,
}

#[derive(Debug, Clone, fory::ForyObject)]
pub struct Ack {
    seq: u64,
}

// -----------------------------------------------------------------------
// Manually declare the enums that #[tarpc::service] would generate.
//
// tarpc::service names Request variants after snake_to_camel of method names:
//   unary_method  -> UnaryMethod
//   server_stream -> ServerStream
//   client_upload -> ClientUpload
//   bidi_sync     -> BidiSync
// -----------------------------------------------------------------------

#[derive(Debug)]
pub enum StreamingSvcRequest {
    UnaryMethod { x: u32 },
    ServerStream { prefix: String },
    ClientUpload { chunk: Vec<u8> },
    BidiSync { item: Data },
}

#[derive(Debug)]
pub enum StreamingSvcResponse {
    UnaryMethod(String),
    ServerStream(Entry),
    ClientUpload(UploadResult),
    BidiSync(Ack),
}

// -----------------------------------------------------------------------
// Apply #[fory_service] to the trait.
// -----------------------------------------------------------------------

#[fory_service]
#[allow(dead_code)]
trait StreamingSvc {
    async fn unary_method(x: u32) -> String;

    #[streaming(server)]
    async fn server_stream(prefix: String) -> Entry;

    #[streaming(client)]
    async fn client_upload(chunk: Vec<u8>) -> UploadResult;

    #[streaming(bidirectional)]
    async fn bidi_sync(item: Data) -> Ack;
}

// -----------------------------------------------------------------------
// Verify that the marker struct was emitted.
// -----------------------------------------------------------------------

#[test]
fn streaming_marker_struct_exists() {
    let _: StreamingSvcService = StreamingSvcService;
}

// -----------------------------------------------------------------------
// Streaming metadata tests (core S4 assertions).
// -----------------------------------------------------------------------

#[test]
fn streaming_methods_metadata() {
    let methods = StreamingSvcService::streaming_methods();
    assert_eq!(methods.len(), 3);
    assert_eq!(methods[0].0, "server_stream");
    assert_eq!(methods[1].0, "client_upload");
    assert_eq!(methods[2].0, "bidi_sync");
}

#[test]
fn streaming_modes_are_correct() {
    use tarpc_fory::StreamingMode;
    let methods = StreamingSvcService::streaming_methods();
    assert_eq!(methods[0].1, StreamingMode::Server);
    assert_eq!(methods[1].1, StreamingMode::Client);
    assert_eq!(methods[2].1, StreamingMode::Bidirectional);
}

#[test]
fn unary_not_in_streaming_methods() {
    let methods = StreamingSvcService::streaming_methods();
    assert!(!methods.iter().any(|(name, _)| *name == "unary_method"));
}

// -----------------------------------------------------------------------
// Fory round-trip smoke test for the request enum.
// -----------------------------------------------------------------------

#[test]
fn request_fory_round_trip_unary() {
    let mut fory = fory::Fory::default();
    fory.register_serializer::<StreamingSvcRequest>(200).unwrap();

    let req = StreamingSvcRequest::UnaryMethod { x: 42 };
    let bytes = fory.serialize(&req).unwrap();
    let decoded: StreamingSvcRequest = fory.deserialize(&bytes).unwrap();
    match decoded {
        StreamingSvcRequest::UnaryMethod { x } => assert_eq!(x, 42),
        _ => panic!("unexpected variant"),
    }
}
