use tower::Layer;
use tower::Service;
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

#[derive(Clone)]
pub struct RetryLayer {
    max_retries: u32,
}

impl RetryLayer {
    pub fn new(max_retries: u32) -> Self {
        Self { max_retries }
    }
}

impl<S> Layer<S> for RetryLayer {
    type Service = RetryService<S>;

    fn layer(&self, service: S) -> Self::Service {
        RetryService {
            inner: service,
            max_retries: self.max_retries,
        }
    }
}

#[derive(Clone)]
pub struct RetryService<S> {
    inner: S,
    max_retries: u32,
}

impl<S, Request> Service<Request> for RetryService<S>
where
    S: Service<Request> + Clone + Send + 'static,
    S::Future: Send + 'static,
    S::Error: std::fmt::Debug,
    Request: Clone + Send + 'static,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, request: Request) -> Self::Future {
        let mut inner = self.inner.clone();
        let max_retries = self.max_retries;

        Box::pin(async move {
            let mut attempts = 0;
            loop {
                match inner.call(request.clone()).await {
                    Ok(response) => return Ok(response),
                    Err(e) => {
                        attempts += 1;
                        if attempts >= max_retries {
                            tracing::warn!("Request failed after {} attempts", attempts);
                            return Err(e);
                        }
                        tracing::debug!("Request failed, retrying (attempt {}/{})", attempts, max_retries);
                        tokio::time::sleep(tokio::time::Duration::from_millis(100 * attempts as u64)).await;
                    }
                }
            }
        })
    }
}