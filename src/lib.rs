extern crate proc_macro;

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};
use syn::{
    FnArg, Ident, ItemTrait, Pat, PatType, ReturnType, Type,
    parse_macro_input,
};

/// Fory wire-schema attribute for tarpc services.
///
/// Apply AFTER `#[tarpc::service]` on a trait definition. Receives the
/// service trait (with method signatures) and emits:
///
/// - Manual `fory::Serializer` + `fory::ForyDefault` impls for `XxxRequest` / `XxxResponse`
///   enums (EXT path — avoids `fory_type_index` collision with user types)
/// - `pub struct XxxService;` marker
/// - `impl tarpc_fory::ServiceWireSchema for XxxService`
/// - User-type registration calls via `fory.register()` for types in method signatures
///
/// The macro receives the service trait definition (after `#[tarpc::service]` has
/// re-emitted it with attributes preserved). It extracts method names, argument types,
/// and return types from the trait to generate matching fory impls for the enums that
/// `#[tarpc::service]` emits.
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
    // The input is the service trait definition (tarpc::service re-emits attrs onto the trait).
    let service_trait: ItemTrait = parse_macro_input!(input as ItemTrait);

    let service_ident = &service_trait.ident;
    let request_ident = format_ident!("{}Request", service_ident);
    let response_ident = format_ident!("{}Response", service_ident);

    // Extract method information from the trait.
    let methods: Vec<RpcMethod> = collect_rpc_methods(&service_trait);

    if methods.is_empty() {
        // No methods — emit the trait unchanged with no additions.
        let ts: TokenStream2 = quote! { #service_trait };
        return ts.into();
    }

    // Generate snake_to_camel names (matching tarpc's naming convention).
    let camel_case_idents: Vec<Ident> = methods
        .iter()
        .map(|m| {
            let camel = snake_to_camel(&m.ident.to_string());
            Ident::new(&camel, m.ident.span())
        })
        .collect();

    // Generate fory impls for the request/response enums and the service marker.
    let fory_tokens = generate_fory_impls(
        service_ident,
        &request_ident,
        &response_ident,
        &methods,
        &camel_case_idents,
    );

    // Emit the service trait unchanged + fory additions.
    let mut output: TokenStream2 = quote! { #service_trait };
    output.extend(fory_tokens);
    output.into()
}

/// Information extracted from a single RPC method on the service trait.
struct RpcMethod {
    ident: Ident,
    /// Typed arguments (excluding `self` and `context`).
    args: Vec<PatType>,
    /// Return type (or `()` if none).
    output: Type,
}

/// Walk the trait items and collect async fn methods, skipping `serve`.
fn collect_rpc_methods(service_trait: &ItemTrait) -> Vec<RpcMethod> {
    let unit: Type = syn::parse_quote!(());
    let mut methods = Vec::new();

    for item in &service_trait.items {
        if let syn::TraitItem::Fn(method) = item {
            let ident = method.sig.ident.clone();
            // Skip non-async methods (e.g. the generated `serve` fn).
            if method.sig.asyncness.is_none() {
                continue;
            }
            // Collect typed arguments (skip self and context::Context).
            let args: Vec<PatType> = method
                .sig
                .inputs
                .iter()
                .filter_map(|arg| match arg {
                    FnArg::Typed(pt) => Some(pt.clone()),
                    FnArg::Receiver(_) => None,
                })
                .filter(|pt| {
                    // Skip context::Context arguments.
                    !is_context_type(&pt.ty)
                })
                .collect();

            let output: Type = match &method.sig.output {
                ReturnType::Type(_, ty) => *ty.clone(),
                ReturnType::Default => unit.clone(),
            };

            methods.push(RpcMethod { ident, args, output });
        }
    }

    methods
}

/// Returns true if the type looks like `context::Context` or `::tarpc::context::Context`.
fn is_context_type(ty: &Type) -> bool {
    if let Type::Path(tp) = ty {
        let segs: Vec<_> = tp.path.segments.iter().collect();
        // Matches: Context, context::Context, tarpc::context::Context, ::tarpc::context::Context
        if let Some(last) = segs.last() {
            if last.ident == "Context" {
                return true;
            }
        }
    }
    false
}

/// Generate all fory-related impls from the method list.
fn generate_fory_impls(
    service_ident: &Ident,
    request_ident: &Ident,
    response_ident: &Ident,
    methods: &[RpcMethod],
    camel_case_idents: &[Ident],
) -> TokenStream2 {
    // -----------------------------------------------------------------------
    // XxxRequest: build per-variant write/read arms.
    //
    // tarpc generates: enum XxxRequest { MethodA { arg1: T1, arg2: T2 }, MethodB {} }
    // Wire layout: u32 discriminant, then each field written inline.
    // -----------------------------------------------------------------------

    let mut req_write_arms = Vec::new();
    let mut req_read_arms = Vec::new();

    for (idx, (method, variant)) in methods.iter().zip(camel_case_idents.iter()).enumerate() {
        let disc = idx as u32;

        if method.args.is_empty() {
            // Unit-like struct variant with no fields: `VariantName {}`
            req_write_arms.push(quote! {
                #request_ident::#variant {} => {
                    (#disc as u32).fory_write(context, ::fory_core::types::RefMode::None, false, false)?;
                }
            });
            req_read_arms.push(quote! {
                #disc => {
                    Ok(#request_ident::#variant {})
                }
            });
        } else {
            let field_idents: Vec<&Pat> = method.args.iter().map(|a| a.pat.as_ref()).collect();
            let field_types: Vec<&Type> = method.args.iter().map(|a| a.ty.as_ref()).collect();

            let field_writes = field_idents.iter().map(|pat| {
                quote! { #pat.fory_write(context, ::fory_core::types::RefMode::None, false, false)?; }
            }).collect::<Vec<_>>();

            let field_reads = field_idents.iter().zip(field_types.iter()).map(|(pat, ty)| {
                quote! {
                    let #pat = <#ty>::fory_read(context, ::fory_core::types::RefMode::None, false)?;
                }
            }).collect::<Vec<_>>();

            req_write_arms.push(quote! {
                #request_ident::#variant { #( #field_idents ),* } => {
                    (#disc as u32).fory_write(context, ::fory_core::types::RefMode::None, false, false)?;
                    #( #field_writes )*
                }
            });

            req_read_arms.push(quote! {
                #disc => {
                    #( #field_reads )*
                    Ok(#request_ident::#variant { #( #field_idents ),* })
                }
            });
        }
    }

    // ForyDefault for XxxRequest: first variant with all fields defaulted.
    let req_default = {
        let first_method = &methods[0];
        let first_variant = &camel_case_idents[0];
        if first_method.args.is_empty() {
            quote! { #request_ident::#first_variant {} }
        } else {
            let defaults = first_method.args.iter().map(|arg| {
                let pat = &arg.pat;
                let ty = &arg.ty;
                quote! { #pat: <#ty as ::fory::ForyDefault>::fory_default() }
            }).collect::<Vec<_>>();
            quote! { #request_ident::#first_variant { #( #defaults ),* } }
        }
    };

    // -----------------------------------------------------------------------
    // XxxResponse: build per-variant write/read arms.
    //
    // tarpc generates: enum XxxResponse { MethodA(RetType1), MethodB(RetType2), ... }
    // Wire layout: u32 discriminant, then the single tuple field.
    // -----------------------------------------------------------------------

    let mut resp_write_arms = Vec::new();
    let mut resp_read_arms = Vec::new();

    for (idx, (method, variant)) in methods.iter().zip(camel_case_idents.iter()).enumerate() {
        let disc = idx as u32;
        let ret_ty = &method.output;

        resp_write_arms.push(quote! {
            #response_ident::#variant(__v) => {
                (#disc as u32).fory_write(context, ::fory_core::types::RefMode::None, false, false)?;
                __v.fory_write(context, ::fory_core::types::RefMode::None, false, false)?;
            }
        });

        resp_read_arms.push(quote! {
            #disc => {
                let __v = <#ret_ty>::fory_read(context, ::fory_core::types::RefMode::None, false)?;
                Ok(#response_ident::#variant(__v))
            }
        });
    }

    // ForyDefault for XxxResponse: first variant with defaulted inner value.
    let resp_default = {
        let first_method = &methods[0];
        let first_variant = &camel_case_idents[0];
        let first_ret = &first_method.output;
        quote! { #response_ident::#first_variant(<#first_ret as ::fory::ForyDefault>::fory_default()) }
    };

    // -----------------------------------------------------------------------
    // ServiceWireSchema impl
    // -----------------------------------------------------------------------

    let req_name = format!("{}Request", service_ident);
    let resp_name = format!("{}Response", service_ident);

    // FNV-1a hashes of the module-qualified type names, evaluated at compile
    // time in the user's crate (so module_path!() expands in the right scope).
    let req_id_expr = quote! {
        ::tarpc_fory::fory_wire_id(concat!(module_path!(), "::", #req_name))
    };
    let resp_id_expr = quote! {
        ::tarpc_fory::fory_wire_id(concat!(module_path!(), "::", #resp_name))
    };

    // Collect user-type registration calls from method signatures.
    let user_type_regs = collect_user_type_registrations(methods);

    let service_marker_ident = format_ident!("{}Service", service_ident);

    quote! {
        // -----------------------------------------------------------------------
        // ForyDefault + Serializer for XxxRequest  (EXT path)
        // -----------------------------------------------------------------------

        impl ::fory::ForyDefault for #request_ident {
            fn fory_default() -> Self {
                #req_default
            }
        }

        impl ::fory::Serializer for #request_ident {
            fn fory_write_data(
                &self,
                context: &mut ::fory::WriteContext,
            ) -> ::core::result::Result<(), ::fory::Error> {
                use ::fory::Serializer as _;
                match self {
                    #( #req_write_arms )*
                }
                Ok(())
            }

            fn fory_read_data(
                context: &mut ::fory::ReadContext,
            ) -> ::core::result::Result<Self, ::fory::Error>
            where
                Self: Sized + ::fory::ForyDefault,
            {
                use ::fory::Serializer as _;
                let __disc =
                    u32::fory_read(context, ::fory_core::types::RefMode::None, false)?;
                match __disc {
                    #( #req_read_arms )*
                    _ => Err(::fory::Error::invalid_data(format!(
                        "{}: unknown variant discriminant {}",
                        stringify!(#request_ident),
                        __disc,
                    ))),
                }
            }

            fn fory_type_id_dyn(
                &self,
                type_resolver: &::fory::TypeResolver,
            ) -> ::core::result::Result<::fory::TypeId, ::fory::Error> {
                Self::fory_get_type_id(type_resolver)
            }

            fn as_any(&self) -> &dyn ::std::any::Any {
                self
            }
        }

        // -----------------------------------------------------------------------
        // ForyDefault + Serializer for XxxResponse  (EXT path)
        // -----------------------------------------------------------------------

        impl ::fory::ForyDefault for #response_ident {
            fn fory_default() -> Self {
                #resp_default
            }
        }

        impl ::fory::Serializer for #response_ident {
            fn fory_write_data(
                &self,
                context: &mut ::fory::WriteContext,
            ) -> ::core::result::Result<(), ::fory::Error> {
                use ::fory::Serializer as _;
                match self {
                    #( #resp_write_arms )*
                }
                Ok(())
            }

            fn fory_read_data(
                context: &mut ::fory::ReadContext,
            ) -> ::core::result::Result<Self, ::fory::Error>
            where
                Self: Sized + ::fory::ForyDefault,
            {
                use ::fory::Serializer as _;
                let __disc =
                    u32::fory_read(context, ::fory_core::types::RefMode::None, false)?;
                match __disc {
                    #( #resp_read_arms )*
                    _ => Err(::fory::Error::invalid_data(format!(
                        "{}: unknown variant discriminant {}",
                        stringify!(#response_ident),
                        __disc,
                    ))),
                }
            }

            fn fory_type_id_dyn(
                &self,
                type_resolver: &::fory::TypeResolver,
            ) -> ::core::result::Result<::fory::TypeId, ::fory::Error> {
                Self::fory_get_type_id(type_resolver)
            }

            fn as_any(&self) -> &dyn ::std::any::Any {
                self
            }
        }

        // -----------------------------------------------------------------------
        // XxxService marker struct + ServiceWireSchema impl
        // -----------------------------------------------------------------------

        /// Marker struct for the service.
        ///
        /// Pass as the type parameter to `tarpc_fory::connect` and
        /// `tarpc_fory::listen` for zero-boilerplate fory transport.
        pub struct #service_marker_ident;

        impl ::tarpc_fory::ServiceWireSchema for #service_marker_ident {
            type Req = #request_ident;
            type Resp = #response_ident;

            fn register(fory: &mut ::fory::Fory) -> ::core::result::Result<(), ::fory::Error> {
                use ::tarpc_fory::envelope::{
                    ForyTraceContext, ForyServerError,
                    ForyRequest, ForyResponse, ForyClientMessage,
                };
                // Non-generic envelope types (shared across all requests/responses).
                fory.register_serializer::<ForyTraceContext>(2)?;
                fory.register_serializer::<ForyServerError>(3)?;
                // ID 4 intentionally unassigned (was ForyResult<T>, now removed).
                // Request-side parameterised envelope types.
                fory.register_serializer::<ForyRequest<#request_ident>>(5)?;
                fory.register_serializer::<ForyClientMessage<#request_ident>>(7)?;
                // Response-side parameterised envelope type.
                fory.register_serializer::<ForyResponse<#response_ident>>(6)?;
                // Generated request/response enums (EXT path — no type_id_index collision).
                fory.register_serializer::<#request_ident>(#req_id_expr)?;
                fory.register_serializer::<#response_ident>(#resp_id_expr)?;
                // User-defined types referenced in method signatures (STRUCT path via register).
                #( #user_type_regs )*
                Ok(())
            }
        }
    }
}

/// Collect fory `register` calls for user-defined types found in method signatures.
///
/// Walks argument types and return types. Skips built-in types; emits
/// `fory.register::<T>(fory_wire_id(type_name::<T>()))?;` for each user-defined type.
fn collect_user_type_registrations(methods: &[RpcMethod]) -> Vec<TokenStream2> {
    use std::collections::BTreeSet;
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut registrations = Vec::new();

    for method in methods {
        // Collect from arguments.
        for arg in &method.args {
            collect_type_registrations(&arg.ty, &mut seen, &mut registrations);
        }
        // Collect from return type.
        collect_type_registrations(&method.output, &mut seen, &mut registrations);
    }

    registrations
}

/// Walk a `syn::Type`, skip built-in/primitive types, and for each non-trivial
/// path type emit a `fory.register::<T>(fory_wire_id(type_name::<T>()))?;` call.
fn collect_type_registrations(
    ty: &Type,
    seen: &mut std::collections::BTreeSet<String>,
    out: &mut Vec<TokenStream2>,
) {
    match ty {
        Type::Path(type_path) => {
            let last_seg = type_path.path.segments.last();
            if let Some(seg) = last_seg {
                let name = seg.ident.to_string();
                if is_builtin_type_name(&name) {
                    // Recurse into generic args (e.g., Vec<UserData> → register UserData).
                    if let syn::PathArguments::AngleBracketed(ref args) = seg.arguments {
                        for arg in &args.args {
                            if let syn::GenericArgument::Type(inner_ty) = arg {
                                collect_type_registrations(inner_ty, seen, out);
                            }
                        }
                    }
                } else {
                    // User-defined type — emit a register call (STRUCT path via ForyObject).
                    let path = &type_path.path;
                    let key = quote! { #path }.to_string();
                    if seen.insert(key) {
                        out.push(quote! {
                            fory.register::<#path>(
                                ::tarpc_fory::fory_wire_id(::std::any::type_name::<#path>())
                            )?;
                        });
                    }
                }
            }
        }
        Type::Tuple(tuple) => {
            for elem in &tuple.elems {
                collect_type_registrations(elem, seen, out);
            }
        }
        Type::Reference(r) => collect_type_registrations(&r.elem, seen, out),
        Type::Paren(p) => collect_type_registrations(&p.elem, seen, out),
        Type::Group(g) => collect_type_registrations(&g.elem, seen, out),
        // Slices, raw pointers, trait objects, impl Trait — skip silently.
        _ => {}
    }
}

/// Returns true for type names that fory handles natively (primitives, std containers, etc.).
/// These do not require explicit registration via `fory.register()`.
fn is_builtin_type_name(name: &str) -> bool {
    matches!(
        name,
        "u8" | "u16"
            | "u32"
            | "u64"
            | "u128"
            | "usize"
            | "i8"
            | "i16"
            | "i32"
            | "i64"
            | "i128"
            | "isize"
            | "f32"
            | "f64"
            | "bool"
            | "char"
            | "String"
            | "str"
            | "Vec"
            | "VecDeque"
            | "LinkedList"
            | "Option"
            | "Result"
            | "HashMap"
            | "BTreeMap"
            | "IndexMap"
            | "HashSet"
            | "BTreeSet"
            | "IndexSet"
            | "Box"
            | "Arc"
            | "Rc"
            | "Cow"
            | "Duration"
            | "Instant"
            | "SystemTime"
            | "PathBuf"
            | "Path"
            | "Bytes"
            | "BytesMut"
    )
}

/// Convert snake_case to CamelCase (matching tarpc's convention for variant names).
fn snake_to_camel(ident_str: &str) -> String {
    let mut camel = String::with_capacity(ident_str.len());
    let mut capitalize_next = true;
    for c in ident_str.chars() {
        if c == '_' {
            capitalize_next = true;
        } else if capitalize_next {
            camel.extend(c.to_uppercase());
            capitalize_next = false;
        } else {
            camel.extend(c.to_lowercase());
        }
    }
    camel.shrink_to_fit();
    camel
}

#[test]
fn snake_to_camel_basic() {
    assert_eq!(snake_to_camel("abc_def"), "AbcDef");
}

#[test]
fn snake_to_camel_single() {
    assert_eq!(snake_to_camel("hello"), "Hello");
}

#[test]
fn snake_to_camel_multi() {
    assert_eq!(snake_to_camel("get_user_data"), "GetUserData");
}
