use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::{parse_macro_input, FnArg, ImplItem, Item, ReturnType, Type};

fn get_result_type_from_future(ty: &syn::Type) -> Option<&syn::Type> {
    if let syn::Type::ImplTrait(type_impl) = ty {
        for bound in &type_impl.bounds {
            if let syn::TypeParamBound::Trait(trait_bound) = bound {
                if let Some(segment) = trait_bound.path.segments.last() {
                    if segment.ident == "Future" {
                        if let syn::PathArguments::AngleBracketed(args) = &segment.arguments {
                            for arg in &args.args {
                                if let syn::GenericArgument::Binding(binding) = arg {
                                    if binding.ident == "Output" {
                                        return Some(&binding.ty);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    None
}

pub fn service(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as Item);

    match input {
        Item::Impl(item_impl) => {
            let self_ty = &item_impl.self_ty;
            let service_name = if let Some((_, path, _)) = &item_impl.trait_ {
                path.segments.last().unwrap().ident.to_string()
            } else {
                let ty_path = if let Type::Path(type_path) = self_ty.as_ref() {
                    type_path
                } else {
                    panic!("Expected a path for self_ty");
                };
                ty_path.path.segments.last().unwrap().ident.to_string()
            };
            let service_name_pascal = service_name;

            let methods_data: Vec<_> = item_impl
                .items
                .iter()
                .filter_map(|item| {
                    if let ImplItem::Method(method) = item {
                        if item_impl.trait_.is_some()
                            || method.attrs.iter().any(|attr| attr.path.is_ident("method"))
                        {
                            let method_name = method.sig.ident.clone();
                            let arg_ty: Type = method
                                .sig
                                .inputs
                                .iter()
                                .filter_map(|arg| {
                                    if let FnArg::Typed(pat_type) = arg {
                                        Some((*pat_type.ty).clone())
                                    } else {
                                        None
                                    }
                                })
                                .nth(0)
                                .expect("Method should have one argument besides &self");

                            let is_async_method = method.sig.asyncness.is_some();

                            let returns_impl_future =
                                if let ReturnType::Type(_, ty) = &method.sig.output {
                                    get_result_type_from_future(ty).is_some()
                                } else {
                                    false
                                };

                            let returns_pin_box_future = if let ReturnType::Type(_, ty) =
                                &method.sig.output
                            {
                                if let Type::Path(type_path) = &**ty {
                                    type_path.path.segments.last().map_or(false, |segment| {
                                        segment.ident == "Pin" && type_path.path.segments.len() > 1
                                    })
                                } else {
                                    false
                                }
                            } else {
                                false
                            };

                            let should_await =
                                is_async_method || returns_pin_box_future || returns_impl_future;

                            if !should_await {
                                panic!(
                                    "Service methods must be `async fn`, return `impl Future`, or be wrapped by `async_trait`. Method `{}` is not.",
                                    method_name
                                );
                            }

                            Some((method_name, arg_ty))
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                })
                .collect();

            let handler_methods = methods_data.iter().map(|(method_name, arg_ty)| {
                let handler_name = format_ident!("__handle_{}", method_name);
                quote! {
                    async fn #handler_name<C: Codec>(&self, req: Request<C>) {
                        let arg = match req.req.as_ref() {
                            None => {
                                unreachable!();
                            }
                            Some(buf) => match req.codec.decode::<#arg_ty>(&buf) {
                                Ok(arg) => arg,
                                Err(_) => {
                                    req.set_rpc_error(occams_rpc_core::error::RpcIntErr::Decode);
                                    return;
                                }
                            },
                        };

                        let res = self.#method_name(arg).await;

                        match res {
                            Ok(resp) => {
                                req.set_result(resp);
                            }
                            Err(e) => {
                                req.set_error(e);
                            }
                        }
                    }
                }
            });

            let dispatch_arms = methods_data.iter().map(|(method_name, _)| {
                let method_name_str = method_name.to_string();
                let handler_name = format_ident!("__handle_{}", method_name);
                quote! {
                    #method_name_str => self.#handler_name(req).await,
                }
            });

            let (impl_generics, _ty_generics, where_clause) = item_impl.generics.split_for_impl();

            let mut service_trait_generics = item_impl.generics.clone();
            service_trait_generics.params.push(syn::parse_quote!(C: Codec));
            let (service_trait_impl_generics, _, service_trait_where_clause) =
                service_trait_generics.split_for_impl();

            let expanded = quote! {
                impl #impl_generics #self_ty #where_clause {
                    #(#handler_methods)*
                }

                impl #service_trait_impl_generics occams_rpc::service::ServiceStatic<C> for #self_ty #service_trait_where_clause {
                    const SERVICE_NAME: &'static str = #service_name_pascal;
                    fn serve(&self, req: Request<C>) -> impl std::future::Future<Output = ()> + Send {
                        async move {
                            match req.method.as_str() {
                                #(#dispatch_arms)*
                                _ => {
                                    req.set_rpc_error(occams_rpc_core::error::RpcIntErr::Method);
                                }
                            }
                        }
                    }
                }
            };

            let final_code = quote! {
                #item_impl
                #expanded
            };

            TokenStream::from(final_code)
        }
        _ => panic!("The `service` attribute can only be applied to impl blocks."),
    }
}

/// ```compile_fail
/// use occams_rpc_api_macros::{method, service};
/// use occams_rpc_core::error::RpcError;
/// use serde::{Deserialize, Serialize};
///
/// #[derive(Debug, Deserialize, Serialize, PartialEq)]
/// pub struct MyArg {
///     pub value: u32,
/// }
///
/// #[derive(Debug, Deserialize, Serialize, PartialEq)]
/// pub struct MyResp {
///     pub result: u32,
/// }
///
/// pub struct NonAsyncServiceImpl;
///
/// #[service]
/// impl NonAsyncServiceImpl {
///     #[method]
///     fn non_async_method(&self, arg: MyArg) -> Result<MyResp, RpcError<String>> {
///         Ok(MyResp { result: arg.value + 1 })
///     }
/// }
/// ```
#[doc(hidden)]
#[allow(dead_code)]
fn test_non_async_method_compile_fail() {}
