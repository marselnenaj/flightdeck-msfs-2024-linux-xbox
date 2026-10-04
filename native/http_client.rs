// SPDX-License-Identifier: MIT
//! HTTPS clients for synchronous download workers.
use reqwest::blocking::ClientBuilder;
use std::time::Duration;

pub(crate) fn builder(timeout: Duration) -> ClientBuilder {
    // Blocking Response::read polls on the calling worker, outside Tokio.
    // An async read_timeout timer panics there. The blocking timeout bounds
    // each connect/read/write operation and needs no runtime on the worker.
    ClientBuilder::new()
        .https_only(true)
        .connect_timeout(timeout)
        .timeout(timeout)
        .user_agent(format!("Flightdeck/{}", crate::VERSION))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use std::{
        io::{Read, Write},
        net::TcpListener,
        sync::mpsc,
        thread,
    };

    fn response(
        timeout: Duration,
    ) -> (
        reqwest::blocking::Response,
        mpsc::Sender<bool>,
        thread::JoinHandle<()>,
    ) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let (release, wait) = mpsc::channel();
        let server = thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut request = Vec::new();
            while !request.ends_with(b"\r\n\r\n") {
                let mut byte = [0];
                socket.read_exact(&mut byte).unwrap();
                request.push(byte[0]);
                assert!(request.len() < 8192);
            }
            socket
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 7\r\nConnection: close\r\n\r\n")
                .unwrap();
            if wait.recv_timeout(Duration::from_secs(5)).unwrap_or(false) {
                socket.write_all(b"payload").unwrap();
            }
        });
        // Only the private unit test admits its loopback HTTP fixture.
        let response = builder(timeout)
            .https_only(false)
            .no_proxy()
            .build()
            .unwrap()
            .get(format!("http://{address}/"))
            .send()
            .unwrap();
        (response, release, server)
    }

    #[test]
    fn worker_reads_streamed_body_without_a_tokio_runtime() {
        let (mut response, release, server) = response(Duration::from_secs(5));
        let reader = thread::spawn(move || {
            assert!(tokio::runtime::Handle::try_current().is_err());
            let mut bytes = Vec::new();
            response.read_to_end(&mut bytes).unwrap();
            bytes
        });
        release.send(true).unwrap();
        assert_eq!(reader.join().unwrap(), b"payload");
        server.join().unwrap();
    }

    #[test]
    fn stalled_body_times_out_without_a_tokio_runtime() {
        let (mut response, release, server) = response(Duration::from_millis(250));
        let reader = thread::spawn(move || {
            assert!(tokio::runtime::Handle::try_current().is_err());
            response.read(&mut [0_u8; 1]).unwrap_err()
        });
        let error = reader.join().unwrap();
        release.send(false).unwrap();
        server.join().unwrap();
        let source = error.into_inner().unwrap();
        assert!(
            source
                .downcast_ref::<reqwest::Error>()
                .unwrap()
                .is_timeout()
        );
    }
}
