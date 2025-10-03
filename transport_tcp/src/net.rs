use nix::errno::Errno;
use std::os::unix::net::{UnixListener, UnixStream};
use std::str::FromStr;
use std::{
    fmt, fs, io,
    net::{AddrParseError, IpAddr, SocketAddr, TcpListener, TcpStream, ToSocketAddrs},
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    str,
};

use log::*;
use occams_rpc::io::{AsyncRead, AsyncWrite};
use occams_rpc::runtime::{AsyncFdTrait, AsyncIO};

/// Unify behavior of tcp & unix ddr
pub enum UnifyAddr {
    Socket(SocketAddr),
    Path(PathBuf),
}

/// Unify behavior of tcp & unix stream
pub enum UnifyStream<IO: AsyncIO> {
    Tcp(IO::AsyncFd<TcpStream>),
    Unix(IO::AsyncFd<UnixStream>),
}

/// Unify behavior of tcp & unix socket listener
pub enum UnifyListener<IO: AsyncIO> {
    Tcp(IO::AsyncFd<TcpListener>),
    Unix(IO::AsyncFd<UnixListener>),
}

impl std::fmt::Display for UnifyAddr {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::Socket(s) => write!(f, "{}", s),
            Self::Path(p) => write!(f, "{}", p.display()),
        }
    }
}
impl std::fmt::Debug for UnifyAddr {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::Socket(s) => write!(f, "{}", s),
            Self::Path(p) => write!(f, "{}", p.display()),
        }
    }
}
impl std::clone::Clone for UnifyAddr {
    fn clone(&self) -> Self {
        match self {
            Self::Socket(s) => UnifyAddr::Socket(s.clone()),
            Self::Path(p) => UnifyAddr::Path(p.clone()),
        }
    }
}

impl std::str::FromStr for UnifyAddr {
    type Err = AddrParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.as_bytes()[0] as char == '/' {
            return Ok(Self::Path(PathBuf::from(s)));
        }
        match s.parse::<SocketAddr>() {
            Ok(a) => Ok(Self::Socket(a)),
            // Can't directly resolve the IP, try to resolve it through the domain name.
            // If multiple IP addresses are resolved, only the first result is taken
            Err(e) => match s.to_socket_addrs() {
                Ok(mut _v) => match _v.next() {
                    Some(a) => Ok(Self::Socket(a)),
                    None => Err(e),
                },
                Err(_) => Err(e),
            },
        }
    }
}

impl std::cmp::PartialEq<str> for UnifyAddr {
    fn eq(&self, other: &str) -> bool {
        match self {
            Self::Socket(s) => {
                match other.parse::<SocketAddr>() {
                    Ok(addr) => *s == addr,
                    Err(_) => {
                        // compatibility case: ‘other’ is IpAddr
                        match other.parse::<IpAddr>() {
                            Ok(addr) => s.ip() == addr,
                            Err(_) => false,
                        }
                    }
                }
            }
            Self::Path(p) => p == Path::new(other),
        }
    }
}

impl<IO: AsyncIO> UnifyListener<IO> {
    pub fn bind(addr: &UnifyAddr) -> io::Result<Self> {
        match addr {
            UnifyAddr::Socket(_addr) => match TcpListener::bind(_addr) {
                Ok(l) => {
                    l.set_nonblocking(true).expect("non_blocking");
                    return Ok(UnifyListener::Tcp(IO::to_async_fd_rd(l)?));
                }
                Err(e) => Err(e),
            },
            UnifyAddr::Path(path) => {
                if path.exists() {
                    fs::remove_file(path)?;
                }
                let path_dup_str = format!("{}{}", path.to_str().unwrap(), "_dup");
                let path_dup = Path::new(&path_dup_str);
                if path_dup.exists() {
                    fs::remove_file(&path_dup)?;
                }
                match UnixListener::bind(&path_dup) {
                    Ok(l) => {
                        if let Err(e) = fs::hard_link(path_dup, path) {
                            error!(
                                "hard_link {:?}->{:?} error: {:?}",
                                path_dup.to_str(),
                                path.to_str(),
                                e
                            );
                            return Err(e);
                        }
                        if let Err(e) = fs::set_permissions(path, fs::Permissions::from_mode(0o666))
                        {
                            error!("cannot get metadata of {:?}: {:?}", path.to_str(), e);
                            return Err(e);
                        }
                        l.set_nonblocking(true).expect("non_blocking");
                        return Ok(UnifyListener::Unix(IO::to_async_fd_rd(l)?));
                    }
                    Err(e) => Err(e),
                }
            }
        }
    }

    #[inline]
    pub async fn accept(&mut self) -> io::Result<UnifyStream<IO>> {
        match self {
            UnifyListener::Tcp(l) => match l.async_read(|_l| _l.accept()).await {
                Ok((stream, _)) => {
                    stream.set_nonblocking(true).expect("non_blocking");
                    return Ok(UnifyStream::Tcp(IO::to_async_fd_rw(stream)?));
                }
                Err(e) => return Err(e),
            },
            UnifyListener::Unix(l) => match l.async_read(|_l| _l.accept()).await {
                Ok((stream, _)) => {
                    stream.set_nonblocking(true).expect("non_blocking");
                    return Ok(UnifyStream::Unix(IO::to_async_fd_rw(stream)?));
                }
                Err(e) => return Err(e),
            },
        }
    }
}

impl<IO: AsyncIO> std::fmt::Display for UnifyListener<IO> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::Tcp(l) => match l.local_addr() {
                Ok(addr) => {
                    return write!(f, "listener {}", addr);
                }
                Err(_) => {
                    return write!(f, "tcp listener unknown");
                }
            },
            Self::Unix(l) => match l.local_addr() {
                Ok(addr) => {
                    return write!(f, "listener {}", addr.as_pathname().unwrap().display());
                }
                Err(_) => {
                    return write!(f, "unix listener unknown");
                }
            },
        }
    }
}

impl<IO: AsyncIO> UnifyStream<IO> {
    pub async fn shutdown_write(&mut self) -> io::Result<()> {
        match self {
            UnifyStream::Tcp(l) => l.async_write(|_l| _l.shutdown(std::net::Shutdown::Write)).await,
            UnifyStream::Unix(l) => {
                l.async_write(|_l| _l.shutdown(std::net::Shutdown::Write)).await
            }
        }
    }
}

impl<IO: AsyncIO> std::fmt::Display for UnifyStream<IO> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::Tcp(l) => match l.local_addr() {
                Ok(addr) => {
                    return write!(f, "listener {}", addr);
                }
                Err(_) => {
                    return write!(f, "tcp listener unknown");
                }
            },
            Self::Unix(l) => match l.local_addr() {
                Ok(addr) => {
                    return write!(f, "listener {}", addr.as_pathname().unwrap().display());
                }
                Err(_) => {
                    return write!(f, "unix listener unknown");
                }
            },
        }
    }
}

impl<IO: AsyncIO> AsyncRead for UnifyStream<IO> {
    async fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        use std::io::Read;
        match self {
            UnifyStream::Tcp(s) => s.async_read(|mut stream| stream.read(buf)).await,
            UnifyStream::Unix(s) => s.async_read(|mut stream| stream.read(buf)).await,
        }
    }
}

impl<IO: AsyncIO> AsyncWrite for UnifyStream<IO> {
    async fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        use std::io::Write;
        match self {
            UnifyStream::Tcp(s) => s.async_write(|mut stream| stream.write(buf)).await,
            UnifyStream::Unix(s) => s.async_write(|mut stream| stream.write(buf)).await,
        }
    }
}

pub fn listen_on_addr<IO: AsyncIO>(addr: &str) -> io::Result<UnifyListener<IO>> {
    match UnifyAddr::from_str(addr) {
        Err(_) => {
            error!("Fail to parse addr {:?}", addr);
            return Err(Errno::EFAULT.into());
        }
        Ok(listen_addr) => match UnifyListener::bind(&listen_addr) {
            Ok(listener) => {
                info!("listen on {:?}", addr);
                return Ok(listener);
            }
            Err(e) => {
                error!("Fail to bind on addr {:?}: {:?}", listen_addr, e);
                return Err(e);
            }
        },
    }
}

#[cfg(test)]
mod tests {

    #[allow(unused_imports)]
    use std::str::FromStr;

    use super::*;

    #[test]
    fn test_addr_compare() {
        let uaddr_unify = UnifyAddr::from_str("127.0.0.1:18555").expect("parse address error");
        let uaddr_with_port = "127.0.0.1:18555";
        let uaddr_without_port = "127.0.0.1";
        let uaddr_wrong_ip_without_port = "127.0.0.2";
        let uaddr_wrong_ip_with_port = "127.0.0.2:18555";
        let uaddr_wrong_port = "127.0.0.1:18888";

        assert!(uaddr_unify == *uaddr_with_port);
        assert!(uaddr_unify == *uaddr_without_port);
        assert!(uaddr_unify != *uaddr_wrong_ip_without_port);
        assert!(uaddr_unify != *uaddr_wrong_ip_with_port);
        assert!(uaddr_unify != *uaddr_wrong_port);
    }
}
