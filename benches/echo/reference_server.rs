//! Reference TCP echo server for `cargo xtask bench echo`.
//!
//! A thread-per-connection TCP echo server using `std::net`.

use std::env;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;

fn handle_client(mut stream: TcpStream) {
    let _ = stream.set_nodelay(true);
    let mut buf = [0u8; 8192];
    loop {
        match stream.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                if stream.write_all(&buf[..n]).is_err() {
                    break;
                }
            }
            Err(_) => break,
        }
    }
}

fn main() {
    let port_str = env::args().nth(1).expect("usage: reference_server <port>");
    let port: u16 = port_str.parse().expect("invalid port number");
    let listener = TcpListener::bind(("127.0.0.1", port)).expect("cannot bind listener");

    println!("listening");
    let _ = std::io::stdout().flush();

    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                thread::spawn(move || handle_client(stream));
            }
            Err(_) => break,
        }
    }
}
