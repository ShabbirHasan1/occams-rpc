use crate::net::{ConnectionHandler, QuicStream, UdpSocket};
use crossfire::MAsyncRx;
use occams_rpc_core::io::{AsyncBufStream, AsyncRead, AsyncWrite};
use occams_rpc_core::runtime::AsyncIO;
use occams_rpc_core::{ClientConfig, error::*};
use occams_rpc_stream::client::{ClientFactory, ClientTaskDecode, ClientTaskDone, ClientTransport};
use occams_rpc_stream::client_timer::ClientTaskTimer;
use occams_rpc_stream::proto;
use ring::rand::SecureRandom;
use std::cell::UnsafeCell;
use std::net::{SocketAddr, ToSocketAddrs};
use std::sync::Arc;
use std::time::Duration;
use std::{fmt, io, str};
use url::Url;

const CLIENT_DEFAULT_BUF_SIZE: usize = 8 * 1024;

pub struct QuicClient<F: ClientFactory> {
    stream: UnsafeCell<AsyncBufStream<QuicStream>>,
    resp_buf: UnsafeCell<Vec<u8>>,
    logger: F::Logger,
    server_id: u64,
    client_id: u64,
    read_timeout: Duration,
    write_timeout: Duration,
    sock: Arc<UdpSocket<F::IO>>,
    conn_handler: ConnectionHandler,
}

unsafe impl<F: ClientFactory> Sync for QuicClient<F> {}

unsafe impl<F: ClientFactory> Send for QuicClient<F> {}
unsafe impl<F: ClientFactory> Sync for QuicClient<F> {}

impl<F: ClientFactory> fmt::Debug for QuicClient<F> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "rpc quic client {}:{}", self.server_id, self.client_id)
    }
}

impl<F: ClientFactory> QuicClient<F> {
    #[inline(always)]
    fn get_stream_mut(&self) -> &mut AsyncBufStream<QuicStream> {
        unsafe { &mut *self.stream.get() }
    }

    #[inline(always)]
    fn get_resp_buf(&self, len: usize) -> &mut Vec<u8> {
        unsafe { &mut *self.resp_buf.get() }
    }

    async fn drive_connection(&self) -> io::Result<()> {
        let mut out = [0; 65535];
        let state = unsafe { &mut *self.conn_handler.state.get() };
        loop {
            let (write, send_info) = match state.conn.send(&mut out) {
                Ok(v) => v,
                Err(quiche::Error::Done) => break,
                Err(e) => return Err(io::Error::new(io::ErrorKind::Other, e)),
            };

            self.sock.send_to(&out[..write], &send_info.to).await?;
        }
        Ok(())
    }
}

impl<F: ClientFactory> ClientTransport<F> for QuicClient<F>
where
    <F as ClientFactory>::IO: std::fmt::Debug,
{
    async fn connect(
        addr: &str, config: &ClientConfig, client_id: u64, server_id: u64, logger: F::Logger,
    ) -> Result<Self, RpcError> {
        let url = Url::parse(addr).map_err(|_| RPC_ERR_CONNECT)?;
        let host = url.host_str().ok_or(RPC_ERR_CONNECT)?;
        let port = url.port().ok_or(RPC_ERR_CONNECT)?;
        let remote_addr: SocketAddr = format!("{}:{}", host, port)
            .to_socket_addrs()
            .map_err(|_| RPC_ERR_CONNECT)?
            .next()
            .ok_or(RPC_ERR_CONNECT)?;

        let bind_addr: SocketAddr = "0.0.0.0:0".parse().unwrap();
        let sock = Arc::new(UdpSocket::<F::IO>::bind(&bind_addr).map_err(|_| RPC_ERR_CONNECT)?);

        let mut quiche_config =
            quiche::Config::new(quiche::PROTOCOL_VERSION).map_err(|_| RPC_ERR_CONNECT)?;
        quiche_config.verify_peer(false);
        quiche_config.set_application_protos(&[b"occams-rpc"]).unwrap();

        let mut scid = [0u8; 16];
        let rng = ring::rand::SystemRandom::new();
        rng.fill(&mut scid[..]).unwrap();
        let scid = quiche::ConnectionId::from_ref(&scid);

        let conn = quiche::connect(
            Some(host),
            &scid,
            sock.local_addr().unwrap(),
            remote_addr,
            &mut quiche_config,
        )
        .map_err(|_| RPC_ERR_CONNECT)?;

        let conn_handler = ConnectionHandler::new(conn);
        let stream = QuicStream::new(conn_handler.clone(), 0);

        let mut buf_size = config.stream_buf_size;
        if buf_size == 0 {
            buf_size = CLIENT_DEFAULT_BUF_SIZE;
        }

        Ok(Self {
            stream: UnsafeCell::new(AsyncBufStream::new(stream, buf_size)),
            resp_buf: UnsafeCell::new(Vec::with_capacity(512)),
            server_id,
            client_id,
            write_timeout: config.write_timeout,
            read_timeout: config.read_timeout,
            logger,
            sock,
            conn_handler,
        })
    }

    #[inline(always)]
    fn get_logger(&self) -> &F::Logger {
        &self.logger
    }

    #[inline(always)]
    async fn close_conn(&self) {}

    #[inline(always)]
    async fn flush_req(&self) -> io::Result<()> {
        self.drive_connection().await
    }

    #[inline(always)]
    async fn write_req<'a>(
        &'a self, need_flush: bool, header: &'a [u8], action_str: Option<&'a [u8]>,
        msg_buf: &'a [u8], blob: Option<&'a [u8]>,
    ) -> io::Result<()> {
        let writer = self.get_stream_mut();

        writer.write_all(header).await?;
        if let Some(action_s) = action_str {
            writer.write_all(action_s).await?;
        }
        if msg_buf.len() > 0 {
            writer.write_all(msg_buf).await?;
        }
        if let Some(blob_buf) = blob {
            writer.write_all(blob_buf).await?;
        }
        if need_flush {
            self.flush_req().await?;
        }
        return Ok(());
    }

    #[inline]
    async fn read_resp(
        &self, _factory: &F, _codec: &F::Codec, _close_ch: Option<&MAsyncRx<()>>,
        _task_reg: &mut ClientTaskTimer<F>,
    ) -> Result<bool, RpcError> {
        let mut recv_buf = [0; 65535];
        loop {
            let mut resp_head_buf = [0u8; proto::RPC_RESP_HEADER_LEN];
            let reader = self.get_stream_mut();

            match F::IO::timeout(self.read_timeout, reader.read_exact(&mut resp_head_buf)).await {
                Ok(Ok(_)) => {
                    return Ok(true);
                }
                Ok(Err(e)) if e.kind() == io::ErrorKind::WouldBlock => {
                    // Not enough data, fall through to read from socket
                }
                Ok(Err(_)) => return Err(RPC_ERR_COMM),
                Err(_) => return Err(RPC_ERR_TIMEOUT), // Timeout
            }

            match self.sock.recv_from(&mut recv_buf).await {
                Ok((len, from)) => {
                    let state = self.conn_handler.get_state_mut();
                    let local_addr = self.sock.local_addr().unwrap();
                    let _ = state
                        .conn
                        .recv(&mut recv_buf[..len], quiche::RecvInfo { from, to: local_addr });
                    for stream_id in state.conn.readable() {
                        if let Some(waker) = state.read_wakers.remove(&stream_id) {
                            waker.wake();
                        }
                    }
                }
                Err(_) => {
                    return Err(RPC_ERR_COMM);
                }
            }
        }
    }
}
