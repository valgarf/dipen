// Taken from: https://pyo3.rs/v0.23.3/async-await.html
// in the future there might be a standard solution included in pyo3
use pyo3::prelude::*;
use std::{
    future::Future,
    pin::{pin, Pin},
    task::{Context, Poll},
};

pub struct AllowThreads<F>(F);

impl<F> Future for AllowThreads<F>
where
    F: Future + Unpin + Send,
    F::Output: Send,
{
    type Output = F::Output;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let waker = cx.waker();
        Python::with_gil(|gil| {
            gil.allow_threads(|| pin!(&mut self.0).poll(&mut Context::from_waker(waker)))
        })
    }
}
