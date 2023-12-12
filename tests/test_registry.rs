use std::net::SocketAddr;
use std::sync::atomic::{AtomicU16, Ordering};
use std::time::Duration;

// in case 4367 is already in use, start with the next port
static PORT: AtomicU16 = AtomicU16::new(4368);

// do not use (flavor = "current_thread") here because the user-provided function is blocking
#[tokio::main]
pub async fn with_test_registry<F: FnOnce(&str)>(f: F) {
    let port = PORT.fetch_add(1, Ordering::Relaxed);
    let listen = SocketAddr::new("127.0.0.1".parse().unwrap(), port);
    let url = format!("http://{listen}/registry");

    // spawn test registry in separate process
    let handle = tokio::task::spawn(buffrs::command::test_registry(listen));

    // wait until the test registry is ready
    let dur = Duration::from_millis(10);
    let client = reqwest::Client::builder()
        .connect_timeout(dur)
        .build()
        .unwrap();
    loop {
        // perform a simple request at an arbitrary URL to check readiness
        if client.get(&url).send().await.is_ok() {
            break;
        }
        // check whether the test registry has failed instead of looping indefinitely
        assert!(!handle.is_finished(), "test registry ended unexpectedly");
        // no busy wait
        tokio::time::sleep(dur).await;
    }

    // run user code
    f(&url);
}
