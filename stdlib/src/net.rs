use std::collections::HashMap;

/// HTTP response structure
#[derive(Debug, Clone)]
pub struct HttpResponse {
    pub status: u16,
    pub headers: HashMap<String, String>,
    pub body: String,
}

/// Fetch a URL via HTTP GET
pub fn http_get(url: &str, headers: Option<HashMap<String, String>>) -> anyhow::Result<HttpResponse> {
    let mut request = ureq::get(url);
    
    if let Some(hdrs) = headers {
        for (key, value) in hdrs {
            request = request.set(&key, &value);
        }
    }
    
    let response = request.call()?;
    let status = response.status();
    
    let mut response_headers = HashMap::new();
    for header_name in response.headers_names() {
        if let Some(value) = response.header(&header_name) {
            response_headers.insert(header_name, value.to_string());
        }
    }
    
    let body = response.into_string()?;
    
    Ok(HttpResponse {
        status,
        headers: response_headers,
        body,
    })
}

/// Fetch a URL via HTTP POST
pub fn http_post(url: &str, body: &str, headers: Option<HashMap<String, String>>) -> anyhow::Result<HttpResponse> {
    let mut request = ureq::post(url);
    
    if let Some(hdrs) = headers {
        for (key, value) in hdrs {
            request = request.set(&key, &value);
        }
    }
    
    let response = request.send_string(body)?;
    let status = response.status();
    
    let mut response_headers = HashMap::new();
    for header_name in response.headers_names() {
        if let Some(value) = response.header(&header_name) {
            response_headers.insert(header_name, value.to_string());
        }
    }
    
    let body = response.into_string()?;
    
    Ok(HttpResponse {
        status,
        headers: response_headers,
        body,
    })
}

/// TCP connect scan - checks if a port is open
pub fn tcp_scan(host: &str, port: u16, timeout_ms: u64) -> bool {
    use std::net::{TcpStream, ToSocketAddrs};
    use std::time::Duration;
    
    let addr = format!("{}:{}", host, port);
    match addr.to_socket_addrs() {
        Ok(mut addrs) => {
            if let Some(addr) = addrs.next() {
                return TcpStream::connect_timeout(&addr, Duration::from_millis(timeout_ms)).is_ok();
            }
            false
        }
        Err(_) => false,
    }
}

/// Grab banner from a TCP service
pub fn tcp_banner_grab(host: &str, port: u16, timeout_ms: u64) -> Option<String> {
    use std::io::{Read, Write};
    use std::net::{TcpStream, ToSocketAddrs};
    use std::time::Duration;
    
    let addr = format!("{}:{}", host, port);
    match addr.to_socket_addrs() {
        Ok(mut addrs) => {
            if let Some(addr) = addrs.next() {
                if let Ok(mut stream) = TcpStream::connect_timeout(&addr, Duration::from_millis(timeout_ms)) {
                    stream.set_read_timeout(Some(Duration::from_millis(timeout_ms))).ok();
                    
                    // Send a generic probe
                    stream.write_all(b"\r\n").ok();
                    
                    let mut buffer = vec![0; 1024];
                    if let Ok(n) = stream.read(&mut buffer) {
                        if n > 0 {
                            return String::from_utf8_lossy(&buffer[..n]).trim().to_string().into();
                        }
                    }
                }
            }
            None
        }
        Err(_) => None,
    }
}