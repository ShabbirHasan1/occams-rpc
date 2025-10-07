use proc_macro::TokenStream;
use quote::{quote, ToTokens};
use syn::{parse_macro_input, Data, DeriveInput, Fields, Ident, Lit, Meta, NestedMeta, Variant};

struct ServerTaskEnumAttrs {
    req: bool,
    resp: bool,
    resp_type: Option<syn::Type>,
}

impl syn::parse::Parse for ServerTaskEnumAttrs {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        let mut req = false;
        let mut resp = false;
        let mut resp_type = None;

        while !input.is_empty() {
            let lookahead = input.lookahead1();
            if lookahead.peek(syn::Ident) {
                let ident: syn::Ident = input.parse()?;
                if ident == "req" {
                    req = true;
                } else if ident == "resp" {
                    resp = true;
                } else if ident == "resp_type" {
                    input.parse::<syn::Token![=]>()?;
                    resp_type = Some(input.parse()?);
                } else {
                    return Err(input.error(format!("unexpected attribute: {}", ident)));
                }
            } else if lookahead.peek(syn::Token![,]) {
                input.parse::<syn::Token![,]>()?;
            } else {
                return Err(input.error("unexpected token"));
            }
        }

        Ok(ServerTaskEnumAttrs { req, resp, resp_type })
    }
}

pub fn server_task_enum_impl(attrs: TokenStream, input: TokenStream) -> TokenStream {
    let mut ast = parse_macro_input!(input as DeriveInput);
    let enum_name = &ast.ident;

    let variants = if let Data::Enum(ref mut data) = ast.data {
        &mut data.variants
    } else {
        panic!("#[server_task_enum] can only be applied to enums");
    };

    let mut from_impls = Vec::new();
    let mut decode_arms = Vec::new();
    let mut get_action_arms = Vec::new();
    let mut encode_arms = Vec::new();
    let mut set_result_arms = Vec::new();
    let mut where_clauses_for_decode = Vec::new();

    let macro_attrs = parse_macro_input!(attrs as ServerTaskEnumAttrs);
    let has_req = macro_attrs.req;
    let has_resp = macro_attrs.resp;
    let resp_type_param = macro_attrs.resp_type;

    let resp_type = if has_resp {
        if resp_type_param.is_some() {
            panic!("Cannot specify 'resp_type' when 'resp' is present. Response type is Self.");
        }
        quote! { #enum_name }
    } else {
        let r_type =
            resp_type_param.expect("resp_type must be specified when 'resp' is not present");
        quote! { #r_type }
    };

    for variant in variants.iter_mut() {
        let variant_name = &variant.ident;
        let inner_type = match &variant.fields {
            Fields::Unnamed(fields) if fields.unnamed.len() == 1 => {
                &fields.unnamed.first().unwrap().ty
            }
            _ => panic!("Enum variants must be tuple-style with a single field"),
        };

        let action_value = if has_req {
            let action = get_action_attribute(variant);
            match action {
                Lit::Int(i) => quote! { occams_rpc::stream::RpcAction::Num(#i) },
                Lit::Str(s) => quote! { occams_rpc::stream::RpcAction::Str(#s) },
                _ => panic!("Unsupported action literal type"),
            }
        } else {
            quote! {} // No action value needed if no req
        };

        from_impls.push(quote! {
            impl From<#inner_type> for #enum_name {
                fn from(task: #inner_type) -> Self {
                    #enum_name::#variant_name(task)
                }
            }
        });

        if has_req {
            decode_arms.push(quote! {
                #action_value => {
                    let task = <#inner_type as occams_rpc::stream::server::ServerTaskDecode<#resp_type>>::decode_req::<C>(codec, action, seq, req, blob, noti)?;
                    Ok(#enum_name::#variant_name(task))
                }
            });

            where_clauses_for_decode.push(quote! {
                #inner_type: occams_rpc::stream::server::ServerTaskDecode<#resp_type>
            });

            get_action_arms.push(quote! {
                #enum_name::#variant_name(_) => #action_value,
            });
        }

        if has_resp {
            encode_arms.push(quote! {
                #enum_name::#variant_name(task) => task.encode_resp(codec),
            });

            set_result_arms.push(quote! {
                #enum_name::#variant_name(ref mut task) => task.set_result(res),
            });
        }
    }

    let req_impl = if has_req {
        quote! {
            impl<R: Send + Unpin + 'static> occams_rpc::stream::server::RpcServerTaskReq<R> for #enum_name {}

            impl<R: Send + Unpin + 'static> occams_rpc::stream::server::ServerTaskDecode<R> for #enum_name
            where
                #(#where_clauses_for_decode),*
            {
                fn decode_req<'a, C: occams_rpc::codec::Codec>(
                    codec: &'a C,
                    action: occams_rpc::stream::RpcAction<'a>,
                    seq: u64,
                    req: &'a [u8],
                    blob: Option<io_buffer::Buffer>,
                    noti: occams_rpc::stream::server::RpcRespNoti<R>,
                ) -> Result<Self, ()> {
                    match action {
                        #(#decode_arms)*
                        _ => {
                            log::error!("Unknown action: {:?}", action);
                            Err(())
                        }
                    }
                }
            }
        }
    } else {
        quote! {}
    };

    let resp_impl = if has_resp {
        quote! {
            impl occams_rpc::stream::server::RpcServerTaskResp for #enum_name {}

            impl occams_rpc::stream::server::ServerTaskEncode for #enum_name {
                fn encode_resp<'a, C: occams_rpc::codec::Codec>(
                    &'a self,
                    codec: &'a C,
                ) -> (u64, Result<(Vec<u8>, Option<&'a io_buffer::Buffer>), &'a occams_rpc::error::RpcError>) {
                    match self {
                        #(#encode_arms)*
                    }
                }
            }

            impl occams_rpc::stream::server::ServerTaskDone<#enum_name> for #enum_name {
                fn set_result(&mut self, res: Result<(), occams_rpc::error::RpcError>) -> occams_rpc::stream::server::RpcRespNoti<#enum_name> {
                    match self {
                        #(#set_result_arms)*
                    }
                }
            }

            impl #enum_name {
                pub fn set_result_done(mut self, res: Result<(), occams_rpc::error::RpcError>) {
                    let noti = self.set_result(res);
                    noti.done(self);
                }
            }
        }
    } else {
        quote! {}
    };

    let get_action_impl = if has_req {
        quote! {
            impl #enum_name {
                pub fn get_action(&self) -> occams_rpc::stream::RpcAction {
                    match self {
                        #(#get_action_arms)*
                    }
                }
            }
        }
    } else {
        quote! {}
    };

    let expanded = quote! {
        #ast

        #(#from_impls)*

        #req_impl

        #resp_impl

        #get_action_impl
    };

    TokenStream::from(expanded)
}

fn get_action_attribute(variant: &Variant) -> Lit {
    for attr in &variant.attrs {
        if attr.path.is_ident("action") {
            if let Ok(Meta::List(meta_list)) = attr.parse_meta() {
                for nested in meta_list.nested.iter() {
                    if let NestedMeta::Meta(Meta::NameValue(name_value)) = nested {
                        if name_value.path.is_ident("action") {
                            return name_value.lit.clone();
                        }
                    }
                }
            }
        }
    }

    // A fallback for a simple #[action = "..."] syntax
    for attr in &variant.attrs {
        if attr.path.is_ident("action") {
            if let Ok(Meta::NameValue(name_value)) = attr.parse_meta() {
                return name_value.lit.clone();
            }
        }
    }

    panic!("Variant {} is missing #[action(...)] attribute", variant.ident);
}
