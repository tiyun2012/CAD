//! Local experimental service. No remote listener or production security is provided.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use solidcraft_pdm::{Error, Request, Store, bind_loopback, read_frame, serve_connection};
use std::{
    io::{self, Write},
    net::{Ipv4Addr, SocketAddrV4, TcpStream},
    path::Path,
    time::Duration,
};

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("serve") if args.len() == 4 => {
            let root = args.get(2).ok_or(Error::Invalid("store path"))?;
            let port = args.get(3).ok_or(Error::Invalid("port"))?.parse::<u16>()?;
            let mut store = Store::open(Path::new(root))?;
            let listener = bind_loopback(port)?;
            eprintln!("Experimental PDM listening on {}; toy documents only", listener.local_addr()?);
            for stream in listener.incoming() {
                if let Err(error) = stream.map_err(Error::from).and_then(|stream| serve_connection(&mut store, stream)) {
                    eprintln!("PDM connection failed: {error}");
                }
            }
        }
        Some("request") if args.len() == 3 => {
            let port = args.get(2).ok_or(Error::Invalid("port"))?.parse::<u16>()?;
            let request: Request = serde_json::from_slice(&read_frame(io::stdin().lock())?)?;
            let address = SocketAddrV4::new(Ipv4Addr::LOCALHOST, port);
            let mut stream = TcpStream::connect_timeout(&address.into(), Duration::from_secs(5))?;
            stream.set_read_timeout(Some(Duration::from_secs(5)))?;
            stream.set_write_timeout(Some(Duration::from_secs(5)))?;
            serde_json::to_writer(&mut stream, &request)?;
            stream.write_all(b"\n")?;
            io::stdout().lock().write_all(&read_frame(stream)?)?;
        }
        _ => return Err("usage: solidcraft-pdm serve STORE PORT | request PORT (JSON line on stdin)".into()),
    }
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
