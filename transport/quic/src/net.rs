//! abstract the common interface for Quic
use futures::future::poll_fn;
use occams_rpc_core::io::{AsyncListener, AsyncRead, AsyncWrite};
use occams_rpc_core::runtime::{AsyncFdTrait, AsyncIO};
use ring::rand::SecureRandom;
use std::cell::UnsafeCell;
use std::collections::HashMap;
use std::io::{self, ErrorKind};
use std::net::{SocketAddr, ToSocketAddrs, UdpSocket as StdUdpSocket};
use std::ops::Deref;
use std::sync::Arc;
use std::task::{Context, Poll, Waker};
use url::Url;

/// A runtime-independent async UDP socket.
pub struct UdpSocket<IO: AsyncIO> {
    io: IO::AsyncFd<StdUdpSocket>,
}

unsafe impl<IO: AsyncIO> Sync for UdpSocket<IO> {}

impl<IO: AsyncIO> UdpSocket<IO> {
    pub fn bind(addr: impl ToSocketAddrs) -> io::Result<Self> {
        let sock = StdUdpSocket::bind(addr)?;
        sock.set_nonblocking(true)?;
        Ok(Self { io: IO::to_async_fd_rd(sock)? })
    }

    pub async fn recv_from(&self, buf: &mut [u8]) -> io::Result<(usize, SocketAddr)> {
        self.io.async_read(|s| s.recv_from(buf)).await
    }

    pub async fn send_to(&self, buf: &[u8], target: &SocketAddr) -> io::Result<usize> {
        self.io.async_write(|s| s.send_to(buf, target)).await
    }

    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        self.io.deref().local_addr()
    }
}

struct ConnState {
    pub conn: quiche::Connection,
    pub read_wakers: HashMap<u64, Waker>,
    pub write_wakers: HashMap<u64, Waker>,
}

impl std::fmt::Debug for ConnState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ConnState")
    }
}

pub struct ConnectionHandler {
    pub state: Arc<UnsafeCell<ConnState>>,
}

unsafe impl Sync for ConnectionHandler {}

impl ConnectionHandler {
    pub fn new(conn: quiche::Connection) -> Self {
        Self {
            state: Arc::new(UnsafeCell::new(ConnState {
                conn,
                read_wakers: HashMap::new(),
                write_wakers: HashMap::new(),
            })),
        }
    }

    #[inline(always)]
    pub fn get_state_mut(&self) -> &mut ConnState {
        unsafe { &mut *self.state.get() }
    }
}

pub struct QuicStream {
    conn_handler: ConnectionHandler,
    stream_id: u64,
}

unsafe impl Sync for QuicStream {}

impl QuicStream {
    pub fn new(conn_handler: ConnectionHandler, stream_id: u64) -> Self {
        Self { conn_handler, stream_id }
    }
}

impl AsyncRead for QuicStream {
    async fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        poll_fn(|cx| {
            let state = self.conn_handler.get_state_mut();
            let stream_id = self.stream_id;

            match state.conn.stream_recv(stream_id, buf) {
                Ok((len, fin)) => {
                    if fin && len == 0 {
                        return Poll::Ready(Ok(0));
                    }
                    Poll::Ready(Ok(len))
                }
                Err(quiche::Error::Done) => {
                    state.read_wakers.insert(stream_id, cx.waker().clone());
                    Poll::Pending
                }
                Err(e) => Poll::Ready(Err(io::Error::new(ErrorKind::Other, e))),
            }
        })
        .await
    }
}

impl AsyncWrite for QuicStream {
    async fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        poll_fn(|cx| {
            let state = self.conn_handler.get_state_mut();
            let stream_id = self.stream_id;

            match state.conn.stream_send(stream_id, buf, false) {
                Ok(len) => Poll::Ready(Ok(len)),
                Err(quiche::Error::Done) => {
                    state.write_wakers.insert(stream_id, cx.waker().clone());
                    Poll::Pending
                }
                Err(e) => Poll::Ready(Err(io::Error::new(ErrorKind::Other, e))),
            }
        })
        .await
    }
}

pub struct QuicListener<IO: AsyncIO> {
    sock: UdpSocket<IO>,
    config: quiche::Config,
    clients: HashMap<quiche::ConnectionId<'static>, (ConnectionHandler, SocketAddr)>,
    buf: Vec<u8>,
    out_buf: Vec<u8>,
    rng: ring::rand::SystemRandom,
}

impl<IO: AsyncIO> std::fmt::Debug for QuicListener<IO> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "QuicListener")
    }
}

unsafe impl<IO: AsyncIO> Sync for QuicListener<IO> {}

impl<IO: AsyncIO + std::fmt::Debug> AsyncListener for QuicListener<IO> {
    type Conn = QuicStream;

    fn bind(addr: &str) -> io::Result<Self> {
        let url = Url::parse(addr).map_err(|e| io::Error::new(ErrorKind::InvalidInput, e))?;
        let host = url
            .host_str()
            .ok_or_else(|| io::Error::new(ErrorKind::InvalidInput, "missing host"))?;
        let port =
            url.port().ok_or_else(|| io::Error::new(ErrorKind::InvalidInput, "missing port"))?;
        let query_pairs: HashMap<_, _> = url.query_pairs().into_owned().collect();
        let cert_path = query_pairs
            .get("cert")
            .ok_or_else(|| io::Error::new(ErrorKind::InvalidInput, "missing cert path"))?;
        let key_path = query_pairs
            .get("key")
            .ok_or_else(|| io::Error::new(ErrorKind::InvalidInput, "missing key path"))?;

        let sock_addr = format!("{}:{}", host, port);
        let sock = UdpSocket::<IO>::bind(&sock_addr)?;

        let mut config = quiche::Config::new(quiche::PROTOCOL_VERSION)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;

        config
            .load_cert_chain_from_pem_file(cert_path)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
        config
            .load_priv_key_from_pem_file(key_path)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;

        config
            .set_application_protos(&[b"occams-rpc"])
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;

        config.set_max_idle_timeout(5000);
        config.set_max_recv_udp_payload_size(1350);
        config.set_max_send_udp_payload_size(1200);
        config.set_initial_max_data(10_000_000);
        config.set_initial_max_stream_data_bidi_local(1_000_000);
        config.set_initial_max_stream_data_bidi_remote(1_000_000);
        config.set_initial_max_streams_bidi(100);
        config.set_initial_max_streams_uni(100);
        config.verify_peer(false);

        Ok(Self {
            sock,
            config,
            clients: HashMap::new(),
            buf: vec![0; 65535],
            out_buf: vec![0; 65535],
            rng: ring::rand::SystemRandom::new(),
        })
    }

    async fn accept(&mut self) -> io::Result<QuicStream> {
        loop {
            let (len, from) = self.sock.recv_from(&mut self.buf).await?;
            let pkt_buf = &mut self.buf[..len];
            let hdr = match quiche::Header::from_slice(pkt_buf, quiche::MAX_CONN_ID_LEN) {
                Ok(v) => v,
                Err(_) => continue,
            };

            let conn_id = hdr.dcid.clone();
            if !self.clients.contains_key(&conn_id) {
                if hdr.ty != quiche::Type::Initial {
                    continue;
                }

                let mut scid = [0u8; 16];
                self.rng.fill(&mut scid[..]).unwrap();
                let scid = quiche::ConnectionId::from_ref(&scid);

                let conn = quiche::accept(
                    &hdr.dcid,
                    Some(&scid),
                    self.sock.local_addr().unwrap(),
                    from,
                    &mut self.config,
                )
                .unwrap();
                let conn_handler = ConnectionHandler::new(conn);
                self.clients.insert(conn_id.clone(), (conn_handler, from));
            }

            let (conn_handler, peer_addr) = self.clients.get_mut(&conn_id).unwrap();
            let local_addr = self.sock.local_addr().unwrap();

            let readable_stream = {
                let state = conn_handler.get_state_mut();
                let _ =
                    state.conn.recv(pkt_buf, quiche::RecvInfo { from: *peer_addr, to: local_addr });

                if state.conn.is_established() { state.conn.readable().next() } else { None }
            };

            if let Some(stream_id) = readable_stream {
                return Ok(QuicStream::new(conn_handler.clone(), stream_id));
            }

            let handlers: Vec<_> = self.clients.values().map(|(h, _)| h.clone()).collect();
            for conn_handler in handlers {
                let state = conn_handler.get_state_mut();
                loop {
                    let (write, send_info) = match state.conn.send(&mut self.out_buf) {
                        Ok(v) => v,
                        Err(quiche::Error::Done) => break,
                        Err(_) => {
                            state.conn.close(false, 0x1, b"fail").ok();
                            break;
                        }
                    };

                    self.sock.send_to(&self.out_buf[..write], &send_info.to).await?;
                }
            }
        }
    }

    fn local_addr(&self) -> io::Result<String> {
        Ok(self.sock.local_addr()?.to_string())
    }

    unsafe fn try_from_raw_fd(_addr: &str, _raw_fd: std::os::unix::io::RawFd) -> io::Result<Self> {
        Err(io::Error::new(
            ErrorKind::Unsupported,
            "try_from_raw_fd is not supported for QUIC transport",
        ))
    }
}
