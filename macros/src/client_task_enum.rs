use proc_macro::TokenStream;
use quote::quote;
use std::collections::HashMap;
use syn::{parse_macro_input, Data, DeriveInput, Fields};

pub fn client_task_enum_impl(_attr: TokenStream, input: TokenStream) -> TokenStream {
    let ast = parse_macro_input!(input as DeriveInput);
    let enum_name = &ast.ident;

    let variants = if let Data::Enum(data) = &ast.data {
        &data.variants
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
    for variant in variants {
        if let Fields::Unnamed(fields) = &variant.fields {
            if fields.unnamed.len() == 1 {
                let inner_type = &fields.unnamed.first().unwrap().ty;
                let inner_type_str = quote! {#inner_type}.to_string();
                *inner_type_counts.entry(inner_type_str).or_insert(0) += 1;
            }
        }
    }

    for variant in variants {
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

        encode_req_arms.push(quote! {
            #enum_name::#variant_name(inner) => inner.encode_req(codec),
        });

        get_req_blob_arms.push(quote! {
            #enum_name::#variant_name(inner) => inner.get_req_blob(),
        });

        decode_resp_arms.push(quote! {
            #enum_name::#variant_name(inner) => inner.decode_resp(codec, buffer),
        });

        reserve_resp_blob_arms.push(quote! {
            #enum_name::#variant_name(inner) => inner.reserve_resp_blob(size),
        });

        get_action_arms.push(quote! {
            #enum_name::#variant_name(inner) => inner.get_action(),
        });

        set_result_arms.push(quote! {
            #enum_name::#variant_name(inner) => inner.set_result(res),
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

        impl occams_rpc::stream::client::ClientTask for #enum_name {
            fn set_result(self, res: Result<(), occams_rpc::error::RpcError>) {
                match self {
                    #(#set_result_arms)*
                }
            }
        }
    };

    TokenStream::from(expanded)
}
