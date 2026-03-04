use std::env;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

#[derive(Debug)]
struct Args {
    url: String,
    contains: Option<String>,
    timeout_secs: u64,
}

#[derive(Debug)]
struct ParsedUrl {
    host: String,
    port: u16,
    path_and_query: String,
}

fn parse_args() -> Result<Args, String> {
    let mut args = env::args().skip(1);
    let mut url = "http://localhost:9026/health/ready".to_string();
    let mut contains: Option<String> = None;
    let mut timeout_secs: u64 = 3;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--url" => {
                url = args
                    .next()
                    .ok_or_else(|| "--url expects a value".to_string())?;
            }
            "--contains" => {
                contains = Some(
                    args.next()
                        .ok_or_else(|| "--contains expects a value".to_string())?,
                );
            }
            "--timeout-seconds" => {
                let raw = args
                    .next()
                    .ok_or_else(|| "--timeout-seconds expects a value".to_string())?;
                timeout_secs = raw
                    .parse::<u64>()
                    .map_err(|_| "--timeout-seconds expects an integer".to_string())?;
            }
            _ => return Err(format!("unknown argument: {arg}")),
        }
    }

    Ok(Args {
        url,
        contains,
        timeout_secs,
    })
}

fn parse_http_url(input: &str) -> Result<ParsedUrl, String> {
    let prefix = "http://";
    if !input.starts_with(prefix) {
        return Err("only http:// URLs are supported".to_string());
    }

    let rest = &input[prefix.len()..];
    let (host_port, path_part) = match rest.split_once('/') {
        Some((hp, path)) => (hp, format!("/{path}")),
        None => (rest, "/".to_string()),
    };

    if host_port.is_empty() {
        return Err("missing host".to_string());
    }

    let (host, port) = match host_port.split_once(':') {
        Some((h, p)) => {
            let port = p.parse::<u16>().map_err(|_| "invalid port".to_string())?;
            (h.to_string(), port)
        }
        None => (host_port.to_string(), 80),
    };

    Ok(ParsedUrl {
        host,
        port,
        path_and_query: path_part,
    })
}

fn main() {
    match run() {
        Ok(()) => {
            println!("ok");
        }
        Err(error) => {
            eprintln!("healthcheck failed: {error}");
            std::process::exit(1);
        }
    }
}

fn run() -> Result<(), String> {
    let args = parse_args()?;
    let target = parse_http_url(&args.url)?;
    let timeout = Duration::from_secs(args.timeout_secs);

    let mut stream = TcpStream::connect((target.host.as_str(), target.port))
        .map_err(|e| format!("connect error: {e}"))?;
    stream
        .set_read_timeout(Some(timeout))
        .map_err(|e| format!("set_read_timeout error: {e}"))?;
    stream
        .set_write_timeout(Some(timeout))
        .map_err(|e| format!("set_write_timeout error: {e}"))?;

    let request = format!(
        "GET {} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n\r\n",
        target.path_and_query, target.host
    );
    stream
        .write_all(request.as_bytes())
        .map_err(|e| format!("write error: {e}"))?;
    stream.flush().map_err(|e| format!("flush error: {e}"))?;

    let mut response_bytes = Vec::with_capacity(4096);
    stream
        .read_to_end(&mut response_bytes)
        .map_err(|e| format!("read error: {e}"))?;

    let response = String::from_utf8(response_bytes).map_err(|e| format!("utf8 error: {e}"))?;
    let mut sections = response.splitn(2, "\r\n\r\n");
    let headers = sections
        .next()
        .ok_or_else(|| "invalid HTTP response (missing headers)".to_string())?;
    let body = sections.next().unwrap_or("");

    let status_line = headers
        .lines()
        .next()
        .ok_or_else(|| "invalid HTTP response (missing status line)".to_string())?;
    let status_code = status_line
        .split_whitespace()
        .nth(1)
        .ok_or_else(|| "invalid HTTP response (missing status code)".to_string())?
        .parse::<u16>()
        .map_err(|_| "invalid HTTP response (bad status code)".to_string())?;

    if !(200..300).contains(&status_code) {
        return Err(format!("unexpected status code: {status_code}"));
    }

    if let Some(expected) = args.contains {
        if !body.contains(&expected) {
            return Err(format!("response body does not contain: {expected}"));
        }
    }

    Ok(())
}
