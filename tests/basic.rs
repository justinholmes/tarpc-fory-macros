// Integration test: verify that #[fory_service] emits working fory Serializer impls.
//
// Strategy: apply #[fory_service] to a trait WITHOUT #[tarpc::service], and manually
// declare the matching HelloRequest/HelloResponse enums. This tests the macro in
// isolation without involving #[tarpc::service], avoiding duplicate-impl conflicts
// with the fork's fory code.
//
// Full integration (fory_service + tarpc::service + real TCP transport) is in
// tarpc-fory's test suite (Task 9).

use tarpc_fory_macros::fory_service;

// -----------------------------------------------------------------------
// Manually declare the enums that #[tarpc::service] would generate.
// These must match the structure the macro expects (named variants for
// request, unnamed single-field variants for response).
// -----------------------------------------------------------------------

#[derive(Debug)]
pub enum HelloRequest {
    Hello { name: String },
    Add { a: i32, b: i32 },
}

#[derive(Debug)]
pub enum HelloResponse {
    Hello(String),
    Add(i32),
}

// -----------------------------------------------------------------------
// Apply #[fory_service] to the trait. The macro generates:
//   - impl ForyDefault for HelloRequest
//   - impl Serializer for HelloRequest
//   - impl ForyDefault for HelloResponse
//   - impl Serializer for HelloResponse
//   - pub struct HelloService;
//   - impl ::tarpc_fory::ServiceWireSchema for HelloService
// -----------------------------------------------------------------------

#[fory_service]
trait Hello {
    async fn hello(name: String) -> String;
    async fn add(a: i32, b: i32) -> i32;
}

// -----------------------------------------------------------------------
// Verify the marker struct was emitted.
// -----------------------------------------------------------------------

#[test]
fn marker_struct_exists() {
    // HelloService should be a zero-sized struct in scope.
    let _: HelloService = HelloService;
}

// -----------------------------------------------------------------------
// Verify fory Serializer round-trips for HelloRequest.
// -----------------------------------------------------------------------

#[test]
fn request_fory_round_trip_hello() {
    let mut fory = fory::Fory::default();
    // register_serializer uses the EXT path — no ForyObject derive needed.
    fory.register_serializer::<HelloRequest>(100).unwrap();

    let req = HelloRequest::Hello { name: "world".to_string() };
    let bytes = fory.serialize(&req).unwrap();
    let decoded: HelloRequest = fory.deserialize(&bytes).unwrap();
    match decoded {
        HelloRequest::Hello { name } => assert_eq!(name, "world"),
        _ => panic!("unexpected variant"),
    }
}

#[test]
fn request_fory_round_trip_add() {
    let mut fory = fory::Fory::default();
    fory.register_serializer::<HelloRequest>(100).unwrap();

    let req = HelloRequest::Add { a: 3, b: 4 };
    let bytes = fory.serialize(&req).unwrap();
    let decoded: HelloRequest = fory.deserialize(&bytes).unwrap();
    match decoded {
        HelloRequest::Add { a, b } => {
            assert_eq!(a, 3);
            assert_eq!(b, 4);
        }
        _ => panic!("unexpected variant"),
    }
}

// -----------------------------------------------------------------------
// Verify fory Serializer round-trips for HelloResponse.
// -----------------------------------------------------------------------

#[test]
fn response_fory_round_trip_string() {
    let mut fory = fory::Fory::default();
    fory.register_serializer::<HelloResponse>(101).unwrap();

    let resp = HelloResponse::Hello("hello, world".to_string());
    let bytes = fory.serialize(&resp).unwrap();
    let decoded: HelloResponse = fory.deserialize(&bytes).unwrap();
    match decoded {
        HelloResponse::Hello(s) => assert_eq!(s, "hello, world"),
        _ => panic!("unexpected variant"),
    }
}

#[test]
fn response_fory_round_trip_i32() {
    let mut fory = fory::Fory::default();
    fory.register_serializer::<HelloResponse>(101).unwrap();

    let resp = HelloResponse::Add(7);
    let bytes = fory.serialize(&resp).unwrap();
    let decoded: HelloResponse = fory.deserialize(&bytes).unwrap();
    match decoded {
        HelloResponse::Add(n) => assert_eq!(n, 7),
        _ => panic!("unexpected variant"),
    }
}

// -----------------------------------------------------------------------
// Verify ForyDefault is implemented (required by register_serializer).
// -----------------------------------------------------------------------

#[test]
fn request_fory_default() {
    let req: HelloRequest = fory::ForyDefault::fory_default();
    // First variant (Hello) with all fields defaulted (String::default() = "").
    match req {
        HelloRequest::Hello { name } => assert_eq!(name, ""),
        _ => panic!("unexpected variant"),
    }
}

#[test]
fn response_fory_default() {
    let resp: HelloResponse = fory::ForyDefault::fory_default();
    // First variant (Hello) with defaulted inner (String::default() = "").
    match resp {
        HelloResponse::Hello(s) => assert_eq!(s, ""),
        _ => panic!("unexpected variant"),
    }
}

// -----------------------------------------------------------------------
// Verify ServiceWireSchema impl can call register() (smoke test).
// -----------------------------------------------------------------------

#[test]
fn service_wire_schema_register_compiles() {
    use tarpc_fory::ServiceWireSchema;
    let mut fory = fory::Fory::default();
    // Full registration chain including envelope stubs.
    HelloService::register(&mut fory).unwrap();
}
