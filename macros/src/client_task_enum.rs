use proc_macro::TokenStream;
use quote::{quote, ToTokens};
use std::collections::HashMap;
use syn::{parse_macro_input, Data, DeriveInput, Fields, Lit, Meta, NestedMeta, Variant};

fn get_action_attribute(variant: &Variant) -> Option<Meta> {
    for attr in &variant.attrs {
        if attr.path.is_ident("action") {
            if let Ok(Meta::List(meta_list)) = attr.parse_meta() {
                let nested = meta_list.nested.first().cloned();
                if let Some(NestedMeta::Lit(lit)) = nested {
                    if meta_list.nested.len() > 1 {
                        panic!("Only one action is allowed per variant");
                    }
                    return Some(Meta::NameValue(syn::MetaNameValue {
                        path: syn::Path::from(syn::Ident::new(
                            "action",
                            proc_macro2::Span::call_site(),
                        )),
                        eq_token: syn::token::Eq::default(),
                        lit,
                    }));
                } else if let Some(NestedMeta::Meta(Meta::Path(path))) = nested {
                    // Handle enum variant like Action::Open
                    return Some(Meta::Path(path));
                }
            }
        }
    }
    None
}

pub fn client_task_enum_impl(_attr: TokenStream, input: TokenStream) -> TokenStream {
    let mut ast = parse_macro_input!(input as DeriveInput);
    let enum_name = &ast.ident;

    let variants = if let Data::Enum(data) = &mut ast.data {
        &mut data.variants
    } else {
        panic!("#[client_task_enum] can only be applied to enums");
    };

    let mut from_impls = Vec::new();
    let mut encode_req_arms = Vec::new();
    let mut get_req_blob_arms = Vec::new();
    let mut decode_resp_arms = Vec::new();
    let mut reserve_resp_blob_arms = Vec::new();
    let mut get_action_arms = Vec::new();
    let mut set_result_arms = Vec::new();
    let mut deref_arms = Vec::new();
    let mut deref_mut_arms = Vec::new();

    let mut inner_type_counts: HashMap<String, usize> = HashMap::new();
    for variant in variants.iter() {
        if let Fields::Unnamed(fields) = &variant.fields {
            if fields.unnamed.len() == 1 {
                let inner_type = &fields.unnamed.first().unwrap().ty;
                let inner_type_str = quote! {#inner_type}.to_string();
                *inner_type_counts.entry(inner_type_str).or_insert(0) += 1;
            }
        }
    }

    for variant in variants.iter_mut() {
        let variant_name = &variant.ident;
        let inner_type = match &variant.fields {
            Fields::Unnamed(fields) if fields.unnamed.len() == 1 => {
                &fields.unnamed.first().unwrap().ty
            }
            _ => panic!("Enum variants must be tuple-style with a single field"),
        };

        let inner_type_str = quote! {#inner_type}.to_string();
        if *inner_type_counts.get(&inner_type_str).unwrap_or(&0) == 1 {
            from_impls.push(quote! {
                impl From<#inner_type> for #enum_name {
                    fn from(task: #inner_type) -> Self {
                        #enum_name::#variant_name(task)
                    }
                }
            });
        }

        let action_meta = get_action_attribute(variant);
        variant.attrs.retain(|attr| !attr.path.is_ident("action"));

        let action_arm = if let Some(meta) = action_meta {
            match meta {
                Meta::NameValue(nv) => match nv.lit {
                    Lit::Int(val) => {
                        quote! { #enum_name::#variant_name(..) => occams_rpc::stream::RpcAction::Num(#val), }
                    }
                    Lit::Str(val) => {
                        quote! { #enum_name::#variant_name(..) => occams_rpc::stream::RpcAction::Str(#val), }
                    }
                    _ => panic!("Unsupported action type"),
                },
                Meta::Path(val) => {
                    quote! { #enum_name::#variant_name(..) => occams_rpc::stream::RpcAction::Num(#val as i32), }
                }
                _ => panic!("Unsupported action type"),
            }
        } else {
            quote! { #enum_name::#variant_name(inner) => occams_rpc::stream::client::ClientTaskAction::get_action(inner), }
        };

        get_action_arms.push(action_arm);

        encode_req_arms.push(quote! {
            #enum_name::#variant_name(inner) => occams_rpc::stream::client::ClientTaskEncode::encode_req(inner, codec),
        });

        get_req_blob_arms.push(quote! {
            #enum_name::#variant_name(inner) => occams_rpc::stream::client::ClientTaskEncode::get_req_blob(inner),
        });

        decode_resp_arms.push(quote! {
            #enum_name::#variant_name(inner) => occams_rpc::stream::client::ClientTaskDecode::decode_resp(inner, codec, buffer),
        });

        reserve_resp_blob_arms.push(quote! {
            #enum_name::#variant_name(inner) => occams_rpc::stream::client::ClientTaskDecode::reserve_resp_blob(inner, size),
        });

        set_result_arms.push(quote! {
            #enum_name::#variant_name(inner) => occams_rpc::stream::client::ClientTaskDone::set_result(inner, res),
        });

        deref_arms.push(quote! {
            #enum_name::#variant_name(inner) => inner,
        });

        deref_mut_arms.push(quote! {
            #enum_name::#variant_name(inner) => inner,
        });
    }

    let expanded = quote! {
        #ast

        #(#from_impls)*

        impl std::ops::Deref for #enum_name {
            type Target = occams_rpc::stream::client::ClientTaskCommon;
            fn deref(&self) -> &Self::Target {
                match self {
                    #(#deref_arms)*
                }
            }
        }

        impl std::ops::DerefMut for #enum_name {
            fn deref_mut(&mut self) -> &mut Self::Target {
                match self {
                    #(#deref_mut_arms)*
                }
            }
        }

        impl occams_rpc::stream::client::ClientTaskEncode for #enum_name {
            fn encode_req<C: occams_rpc::codec::Codec>(&self, codec: &C) -> Result<Vec<u8>, ()> {
                match self {
                    #(#encode_req_arms)*
                }
            }

            fn get_req_blob(&self) -> Option<&[u8]> {
                match self {
                    #(#get_req_blob_arms)*
                }
            }
        }

        impl occams_rpc::stream::client::ClientTaskDecode for #enum_name {
            fn decode_resp<C: occams_rpc::codec::Codec>(&mut self, codec: &C, buffer: &[u8]) -> Result<(), ()> {
                match self {
                    #(#decode_resp_arms)*
                }
            }

            fn reserve_resp_blob(&mut self, size: i32) -> Option<&mut [u8]> {
                match self {
                    #(#reserve_resp_blob_arms)*
                }
            }
        }

        impl occams_rpc::stream::client::ClientTaskAction for #enum_name {
            fn get_action<'a>(&'a self) -> occams_rpc::stream::RpcAction<'a> {
                match self {
                    #(#get_action_arms)*
                }
            }
        }

        impl occams_rpc::stream::client::ClientTaskDone for #enum_name {
            fn set_result(self, res: Result<(), occams_rpc::error::RpcError>) {
                match self {
                    #(#set_result_arms)*
                }
            }
        }

        impl occams_rpc::stream::client::ClientTask for #enum_name {}
    };

    TokenStream::from(expanded)
}
