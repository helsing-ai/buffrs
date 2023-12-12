use std::process::{Child, Command};
use std::time::Duration;

struct ChildGuard(Child);

impl Drop for ChildGuard {
    fn drop(&mut self) {
        self.0.kill().expect("could not kill test-registry");
    }
}

pub fn with_test_registry<F: FnOnce(&str)>(f: F) {
    // spawn test registry in separate process
    let mut handle = ChildGuard(
        Command::new("cargo")
            .arg("run")
            .arg("--")
            .arg("test-registry")
            .spawn()
            .expect("could not spawn test-registry"),
    );

    // wait until the test registry is ready
    let dur = Duration::from_millis(10);
    let client = reqwest::blocking::Client::new();
    loop {
        // perform a simple request at an arbitrary URL to check readiness
        let mut req = reqwest::blocking::Request::new(
            reqwest::Method::GET,
            "http://localhost:4367/registry".try_into().unwrap(),
        );
        *req.timeout_mut() = Some(dur);
        if client.execute(req).is_ok() {
            break;
        }
        // check whether the test registry has failed instead of looping indefinitely
        if handle.0.try_wait().unwrap().is_some() {
            handle.0.wait().unwrap();
        }
        // no busy wait
        std::thread::sleep(dur);
    }

    // run user code
    f("http://localhost:4367/registry");
}
