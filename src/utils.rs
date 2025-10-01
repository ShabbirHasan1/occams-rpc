use std::future::Future;
use std::pin::Pin;
use std::task::*;

pub struct CancellableRead<F, C> {
    read_future: F,
    cancel_future: C,
}

impl<F, C> CancellableRead<F, C> {
    pub fn new(read_future: F, cancel_future: C) -> Self {
        Self { read_future, cancel_future }
    }
}

impl<F: Future, C: Future> Future for CancellableRead<F, C> {
    type Output = Result<F::Output, ()>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = unsafe { self.get_unchecked_mut() };
        let read_future = unsafe { Pin::new_unchecked(&mut this.read_future) };
        if let Poll::Ready(output) = read_future.poll(cx) {
            return Poll::Ready(Ok(output));
        }
        let cancel_future = unsafe { Pin::new_unchecked(&mut this.cancel_future) };
        if let Poll::Ready(_) = cancel_future.poll(cx) {
            return Poll::Ready(Err(()));
        }
        return Poll::Pending;
    }
}
