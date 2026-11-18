// OWNER: S6 (net) — LAN address discovery, spec 08-net.md §A.2.1 `lan_urls(bind, port)`:
// 1. bind not empty / not 0.0.0.0 → only `http://{bind}:{port}`;
// 2. else UDP socket `connect(("10.255.255.255", 1))` (sends nothing) → first IP;
// 3. plus the host name's IPv4 addresses (sorted, deduplicated);
// 4. every IPv4 not starting with `127.` → `http://{ip}:{port}`.

use std::net::{IpAddr, Ipv4Addr, ToSocketAddrs, UdpSocket};

pub fn lan_urls(bind: &str, port: u16) -> Vec<String> {
    if !bind.is_empty() && bind != "0.0.0.0" {
        return vec![format!("http://{bind}:{port}")];
    }
    let mut ips: Vec<Ipv4Addr> = Vec::new();
    if let Ok(s) = UdpSocket::bind("0.0.0.0:0")
        && s.connect("10.255.255.255:1").is_ok()
        && let Ok(a) = s.local_addr()
        && let IpAddr::V4(v4) = a.ip()
    {
        ips.push(v4);
    }
    let host = std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_default();
    if !host.is_empty()
        && let Ok(addrs) = (host.as_str(), 0).to_socket_addrs()
    {
        let mut more: Vec<Ipv4Addr> = addrs
            .filter_map(|a| match a.ip() {
                IpAddr::V4(v) if !ips.contains(&v) => Some(v),
                _ => None,
            })
            .collect();
        more.sort();
        more.dedup();
        ips.extend(more);
    }
    ips.iter()
        .filter(|ip| ip.octets()[0] != 127 && !ip.is_unspecified())
        .map(|ip| format!("http://{ip}:{port}"))
        .collect()
}
