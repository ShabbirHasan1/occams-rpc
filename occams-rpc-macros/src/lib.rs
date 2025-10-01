use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, DeriveInput, Fields, Ident, Meta, NestedMeta, Type};

fn check_option_inner_type(ty: &syn::Type) -> bool {
    if let syn::Type::Path(syn::TypePath { qself: None, path }) = ty {
        if path.leading_colon.is_none()
            && path.segments.len() == 1
            && path.segments[0].ident == "Option"
        {
            if let syn::PathArguments::AngleBracketed(args) = &path.segments[0].arguments {
                if args.args.len() == 1 {
                    if let syn::GenericArgument::Type(_inner_ty) = &args.args[0] {
                        return true;
                    }
                }
            }
        }
    }
    false
}

#[proc_macro_attribute]
pub fn client_task(
    _attrs: proc_macro::TokenStream, input: proc_macro::TokenStream,
) -> proc_macro::TokenStream {
    let mut ast = parse_macro_input!(input as DeriveInput);
    let struct_name = &ast.ident;
    let mut common_field: Option<(Ident, Type)> = None;
    let mut req_field: Option<Ident> = None;
    let mut resp_field: Option<(Ident, Type)> = None;
    let mut req_blob_field: Option<Ident> = None; // New field for req_blob
    let mut resp_blob_field: Option<(Ident, Type)> = None; // New field for resp_blob

    if let syn::Data::Struct(syn::DataStruct { fields: Fields::Named(fields), .. }) = &mut ast.data
    {
        for field in fields.named.iter_mut() {
            let mut new_attrs = Vec::new();
            for attr in &field.attrs {
                if attr.path.is_ident("field") {
                    if let Ok(Meta::List(meta_list)) = attr.parse_meta() {
                        for nested in meta_list.nested.iter() {
                            if let NestedMeta::Meta(Meta::Path(p)) = nested {
                                if let Some(ident) = p.get_ident() {
                                    let f_name = field.ident.as_ref().unwrap().clone();
                                    let f_type = field.ty.clone();
                                    match ident.to_string().as_str() {
                                        "common" => common_field = Some((f_name, f_type)),
                                        "req" => req_field = Some(f_name),
                                        "resp" => resp_field = Some((f_name, f_type)),
                                        "req_blob" => req_blob_field = Some(f_name), // Handle req_blob
                                        "resp_blob" => resp_blob_field = Some((f_name, f_type)), // Handle resp_blob
                                        _ => {}
                                    }
                                }
                            }
                        }
                    }
                } else {
                    new_attrs.push(attr.clone());
                }
            }
            field.attrs = new_attrs;
        }
    }

    let (common_field_name, common_field_type) =
        common_field.expect("common field must be tagged with #[field(common)]");
    let req_field_name = req_field.expect("req field must be tagged with #[field(req)]");
    let (resp_field_name, resp_field_type) =
        resp_field.expect("resp field must be tagged with #[field(resp)]");

    if !check_option_inner_type(&resp_field_type) {
        panic!("`{}::{}` resp field must be of type Option<T>", struct_name, resp_field_name);
    }

    let get_req_blob_body = if let Some(req_blob_field_name) = req_blob_field {
        quote! {
            fn get_req_blob(&self) -> Option<&[u8]> {
                Some(self.#req_blob_field_name.as_ref())
            }
        }
    } else {
        quote! {}
    };

    let get_resp_blob_mut_body = if let Some((resp_blob_field_name, _)) = &resp_blob_field {
        quote! {
            fn get_resp_blob_mut(&mut self) -> Option<&mut impl occams_rpc::stream::client_task::AllocateBuf> {
                Some(&mut self.#resp_blob_field_name)
            }
        }
    } else {
        quote! {}
    };

    let expanded = quote! {
        #ast

        impl std::ops::Deref for #struct_name {
            type Target = #common_field_type;
            fn deref(&self) -> &Self::Target {
                &self.#common_field_name
            }
        }

        impl std::ops::DerefMut for #struct_name {
            fn deref_mut(&mut self) -> &mut Self::Target {
                &mut self.#common_field_name
            }
        }

        impl ClientTaskEncode for #struct_name {
            fn encode_req<C: occams_rpc::codec::Codec>(&self, codec: &C) -> Result<Vec<u8>, ()> {
                codec.encode(&self.#req_field_name)
            }

            #get_req_blob_body
        }

        impl ClientTaskDecode for #struct_name {
            fn decode_resp<C: occams_rpc::codec::Codec>(&mut self, codec: &C, buffer: &[u8]) -> Result<(), ()> {
                let resp = codec.decode(buffer)?;
                self.#resp_field_name = Some(resp);
                Ok(())
            }

            #get_resp_blob_mut_body
        }
    };
    TokenStream::from(expanded)
}

/// ```compile_fail
/// use occams_rpc_macros::*;
/// #[client_task]
/// pub struct FileTaskWrongResp {
///     #[field(common)]
///     common: occams_rpc::stream::client_task::ClientTaskCommon,
///     #[field(req)]
///     req: (),
///     #[field(resp)]
///     resp: (),
/// }
/// ```
#[doc(hidden)]
#[allow(dead_code)]
fn test_resp_not_option() {}

/// ```compile_fail
/// use occams_rpc_macros::*;
/// #[client_task]
/// pub struct FileTaskNoReq {
///     #[field(common)]
///     common: occams_rpc::stream::client_task::ClientTaskCommon,
///     #[field(resp)]
///     resp: Option<()>,
/// }
/// ```
#[doc(hidden)]
#[allow(dead_code)]
fn test_missing_req() {}

/// ```compile_fail
/// use occams_rpc_macros::*;
/// #[client_task]
/// pub struct FileTaskNoResp {
///     #[field(common)]
///     common: occams_rpc::stream::client_task::ClientTaskCommon,
///     #[field(req)]
///     req: (),
/// }
/// ```
#[doc(hidden)]
#[allow(dead_code)]
fn test_missing_resp() {}
