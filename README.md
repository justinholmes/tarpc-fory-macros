# tarpc-fory-macros

Proc-macro companion for [`tarpc-fory`](https://crates.io/crates/tarpc-fory).

Provides the `#[fory_service]` attribute that generates Apache Fory serialization impls for tarpc service types.

## Usage

```rust
#[tarpc::service]
#[tarpc_fory::fory_service]
trait Hello {
    async fn hello(name: String) -> String;

    #[streaming(server)]
    async fn list_items(prefix: String) -> Item;
}
```

The `#[fory_service]` attribute:
- Parses the tarpc-generated `HelloRequest` / `HelloResponse` enums
- Emits manual fory `Serializer` impls (EXT path — no TYPE_ID_COUNTER collision)
- Emits `pub struct HelloService;` marker struct
- Emits `impl ServiceWireSchema for HelloService` with auto-registration
- Parses `#[streaming(server|client|bidirectional)]` attributes and emits metadata

## License

Dual-licensed under MIT or Apache-2.0.
