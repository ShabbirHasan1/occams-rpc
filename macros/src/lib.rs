mod client_task;
mod client_task_enum;
mod server_task_enum;

#[proc_macro_attribute]
pub fn client_task(
    attrs: proc_macro::TokenStream, input: proc_macro::TokenStream,
) -> proc_macro::TokenStream {
    client_task::client_task_impl(attrs, input)
}

#[proc_macro_attribute]
pub fn client_task_enum(
    attrs: proc_macro::TokenStream, input: proc_macro::TokenStream,
) -> proc_macro::TokenStream {
    client_task_enum::client_task_enum_impl(attrs, input)
}

#[proc_macro_attribute]
pub fn server_task_enum(
    attrs: proc_macro::TokenStream, input: proc_macro::TokenStream,
) -> proc_macro::TokenStream {
    server_task_enum::server_task_enum_impl(attrs, input)
}
