// low level of rpc

pub mod client;
pub mod client_impl;
pub mod client_timer;
pub mod proto;
pub mod server;
pub mod server_impl;
pub mod throttler;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RpcAction<'a> {
    Str(&'a str),
    Num(i32),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum RpcActionOwned {
    Str(String),
    Num(i32),
}

impl<'a> From<RpcAction<'a>> for RpcActionOwned {
    fn from(action: RpcAction<'a>) -> Self {
        match action {
            RpcAction::Str(s) => RpcActionOwned::Str(s.to_string()),
            RpcAction::Num(n) => RpcActionOwned::Num(n),
        }
    }
}

impl RpcActionOwned {
    pub fn to_action<'a>(&'a self) -> RpcAction<'a> {
        match self {
            RpcActionOwned::Str(s) => RpcAction::Str(s.as_str()),
            RpcActionOwned::Num(n) => RpcAction::Num(*n),
        }
    }
}
