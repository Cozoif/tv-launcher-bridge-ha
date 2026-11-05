//! Finds TVs with "Wireless debugging" on the home network (mDNS / DNS-SD),
//! because the TV picks a new port every time wireless debugging restarts.

use std::collections::HashMap;
use std::net::{Ipv4Addr, SocketAddrV4, UdpSocket};
use std::time::{Duration, Instant};

#[derive(Clone, Debug, PartialEq)]
pub enum Kind {
    /// Paired TV, ready to connect.
    Connect,
    /// The "Pair device with pairing code" screen is open on the TV.
    Pairing,
}

#[derive(Clone, Debug)]
pub struct Found {
    pub name: String,
    pub ip: String,
    pub port: u16,
    pub kind: Kind,
}

const SERVICES: [&str; 2] = ["_adb-tls-connect._tcp.local", "_adb-tls-pairing._tcp.local"];
const GROUP: Ipv4Addr = Ipv4Addr::new(224, 0, 0, 251);

fn encode_name(out: &mut Vec<u8>, name: &str) {
    for label in name.split('.') {
        out.push(label.len() as u8);
        out.extend_from_slice(label.as_bytes());
    }
    out.push(0);
}

fn query(unicast: bool) -> Vec<u8> {
    let mut q = vec![0, 0, 0, 0, 0, SERVICES.len() as u8, 0, 0, 0, 0, 0, 0];
    for s in SERVICES {
        encode_name(&mut q, s);
        q.extend_from_slice(&[0, 12]); // PTR
        q.extend_from_slice(if unicast { &[0x80, 1] } else { &[0, 1] });
    }
    q
}

fn read_name(msg: &[u8], mut pos: usize) -> Option<(String, usize)> {
    let mut labels = Vec::new();
    let mut end = None;
    let mut jumps = 0;
    loop {
        let len = *msg.get(pos)? as usize;
        if len == 0 {
            pos += 1;
            break;
        }
        if len & 0xc0 == 0xc0 {
            let ptr = ((len & 0x3f) << 8) | *msg.get(pos + 1)? as usize;
            if end.is_none() {
                end = Some(pos + 2);
            }
            jumps += 1;
            if jumps > 20 {
                return None;
            }
            pos = ptr;
            continue;
        }
        labels.push(String::from_utf8_lossy(msg.get(pos + 1..pos + 1 + len)?).into_owned());
        pos += 1 + len;
    }
    Some((labels.join("."), end.unwrap_or(pos)))
}

#[derive(Default)]
struct Records {
    srv: HashMap<String, (u16, String)>,
    a: HashMap<String, Ipv4Addr>,
}

fn parse(msg: &[u8], rec: &mut Records) -> Option<()> {
    if msg.len() < 12 {
        return None;
    }
    let count = |i: usize| u16::from_be_bytes([msg[i], msg[i + 1]]) as usize;
    let (qd, an, ns, ar) = (count(4), count(6), count(8), count(10));
    let mut pos = 12;
    for _ in 0..qd {
        let (_, p) = read_name(msg, pos)?;
        pos = p + 4;
    }
    for _ in 0..(an + ns + ar) {
        let (name, p) = read_name(msg, pos)?;
        let h = msg.get(p..p + 10)?;
        let rtype = u16::from_be_bytes([h[0], h[1]]);
        let rdlen = u16::from_be_bytes([h[8], h[9]]) as usize;
        let rdata_at = p + 10;
        let rdata = msg.get(rdata_at..rdata_at + rdlen)?;
        match rtype {
            33 if rdata.len() > 6 => {
                let port = u16::from_be_bytes([rdata[4], rdata[5]]);
                let (target, _) = read_name(msg, rdata_at + 6)?;
                rec.srv.insert(name.to_lowercase(), (port, target.to_lowercase()));
            }
            1 if rdata.len() == 4 => {
                rec.a.insert(name.to_lowercase(), Ipv4Addr::new(rdata[0], rdata[1], rdata[2], rdata[3]));
            }
            _ => {}
        }
        pos = rdata_at + rdlen;
    }
    Some(())
}

fn multicast_socket() -> Option<UdpSocket> {
    use socket2::{Domain, Protocol, Socket, Type};
    let s = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP)).ok()?;
    s.set_reuse_address(true).ok()?;
    let _ = s.set_reuse_port(true);
    s.bind(&SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 5353).into()).ok()?;
    s.join_multicast_v4(&GROUP, &Ipv4Addr::UNSPECIFIED).ok()?;
    let _ = s.set_multicast_loop_v4(false);
    Some(s.into())
}

/// Look for TVs for about `wait`.
pub fn discover(wait: Duration) -> Vec<Found> {
    let dest = SocketAddrV4::new(GROUP, 5353);
    let mut sockets = Vec::new();
    if let Ok(s) = UdpSocket::bind("0.0.0.0:0") {
        let _ = s.send_to(&query(true), dest);
        sockets.push(s);
    }
    if let Some(s) = multicast_socket() {
        let _ = s.send_to(&query(false), dest);
        sockets.push(s);
    }
    let mut rec = Records::default();
    let start = Instant::now();
    let mut buf = [0u8; 9000];
    let mut resent = false;
    while start.elapsed() < wait {
        for s in &sockets {
            let _ = s.set_read_timeout(Some(Duration::from_millis(120)));
            if let Ok((n, _)) = s.recv_from(&mut buf) {
                let _ = parse(&buf[..n], &mut rec);
            }
        }
        if !resent && start.elapsed() > wait / 2 {
            resent = true;
            for s in &sockets {
                let _ = s.send_to(&query(false), dest);
            }
        }
    }
    let mut out = Vec::new();
    for (inst, (port, target)) in &rec.srv {
        let kind = if inst.ends_with("._adb-tls-connect._tcp.local") {
            Kind::Connect
        } else if inst.ends_with("._adb-tls-pairing._tcp.local") {
            Kind::Pairing
        } else {
            continue;
        };
        let Some(ip) = rec.a.get(target) else { continue };
        let name = inst.split("._adb").next().unwrap_or(inst).to_string();
        out.push(Found {
            name,
            ip: ip.to_string(),
            port: *port,
            kind,
        });
    }
    out.sort_by(|a, b| a.ip.cmp(&b.ip).then(a.port.cmp(&b.port)));
    out
}
