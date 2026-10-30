//! Cancellable background I/O shared by media discovery and comic workflows.
use anyhow::Result;
use std::sync::{
    Arc, OnceLock,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
use std::time::Duration;

fn runtime() -> Result<&'static tokio::runtime::Runtime> {
    static RUNTIME: OnceLock<Result<tokio::runtime::Runtime, String>> = OnceLock::new();
    RUNTIME
        .get_or_init(|| {
            tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
                .map_err(|error| error.to_string())
        })
        .as_ref()
        .map_err(|error| anyhow::anyhow!("{error}"))
}

pub struct Job<T> {
    result: crossbeam_channel::Receiver<Result<T>>,
    cancel: Arc<AtomicBool>,
    pub bytes: Arc<AtomicU64>,
}
impl<T: Send + 'static> Job<T> {
    pub fn spawn<F, Fut>(work: F) -> Self
    where
        F: FnOnce(Arc<AtomicU64>) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = Result<T>> + Send + 'static,
    {
        let (tx, result) = crossbeam_channel::bounded(1);
        let cancel = Arc::new(AtomicBool::new(false));
        let bytes = Arc::new(AtomicU64::new(0));
        let stop = cancel.clone();
        let progress = bytes.clone();
        match runtime() {
            Ok(rt) => {
                rt.spawn(async move {
                    let value = tokio::select! {
                        result = work(progress) => result,
                        _ = async {
                            while !stop.load(Ordering::Relaxed) {
                                tokio::time::sleep(Duration::from_millis(40)).await;
                            }
                        } => Err(anyhow::anyhow!("Cancelled")),
                    };
                    let _ = tx.send(value);
                });
            }
            Err(error) => {
                let _ = tx.send(Err(error));
            }
        }
        Self {
            result,
            cancel,
            bytes,
        }
    }
    pub fn poll(&self) -> Option<Result<T>> {
        match self.result.try_recv() {
            Ok(result) => Some(result),
            Err(crossbeam_channel::TryRecvError::Empty) => None,
            Err(_) => Some(Err(anyhow::anyhow!("Background worker stopped"))),
        }
    }
}
impl<T> Drop for Job<T> {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn successive_jobs_can_reuse_a_pooled_connection() {
        use axum::{extract::ConnectInfo, routing::get};
        use std::{collections::HashSet, net::SocketAddr, sync::Mutex};
        let peers = Arc::new(Mutex::new(HashSet::new()));
        let observed = peers.clone();
        let (address, server) = runtime().unwrap().block_on(async {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = listener.local_addr().unwrap();
            let app = axum::Router::new().route(
                "/",
                get(move |ConnectInfo(peer): ConnectInfo<SocketAddr>| {
                    observed.lock().unwrap().insert(peer);
                    async { "ok" }
                }),
            );
            let server = tokio::spawn(async move {
                axum::serve(
                    listener,
                    app.into_make_service_with_connect_info::<SocketAddr>(),
                )
                .await
                .unwrap();
            });
            (address, server)
        });
        let client = reqwest::Client::new();
        for _ in 0..2 {
            let client = client.clone();
            let job = Job::spawn(move |_| async move {
                Ok(client
                    .get(format!("http://{address}/"))
                    .send()
                    .await?
                    .text()
                    .await?)
            });
            assert_eq!(
                job.result
                    .recv_timeout(Duration::from_secs(3))
                    .unwrap()
                    .unwrap(),
                "ok"
            );
        }
        assert_eq!(
            peers.lock().unwrap().len(),
            1,
            "job completion must keep HTTP keep-alive usable"
        );
        server.abort();
    }

    #[test]
    fn dropping_a_job_cancels_in_flight_work() {
        struct Signal(crossbeam_channel::Sender<()>);
        impl Drop for Signal {
            fn drop(&mut self) {
                let _ = self.0.send(());
            }
        }
        let (started, ready) = crossbeam_channel::bounded(1);
        let (cancelled, stopped) = crossbeam_channel::bounded(1);
        let job = Job::<()>::spawn(move |_| async move {
            let _signal = Signal(cancelled);
            let _ = started.send(());
            futures_util::future::pending().await
        });
        ready.recv_timeout(Duration::from_secs(3)).unwrap();
        drop(job);
        stopped
            .recv_timeout(Duration::from_secs(3))
            .expect("obsolete page requests must stop promptly");
    }
}
