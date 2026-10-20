//! DNS: spektr's own stub resolver over `hickory-proto` (wire format only).
//!
//! UDP to the configured nameservers (or `/etc/resolv.conf`), EDNS0 with a
//! 1232-byte payload, a random query id matched against the response and
//! its question, TCP when the answer is truncated, and failover across
//! nameservers with two attempts each. No caching: the engine caches by
//! module, version and input. No search domains: modules always query
//! fully qualified names.

use std::net::{IpAddr, SocketAddr};
use std::path::Path;
use std::time::Duration;

use hickory_proto::op::{Edns, Message, MessageType, Query};
use hickory_proto::rr::{Name, RData, RecordType};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpStream, UdpSocket};

use super::fixtures::{Exchange, record};
use super::{NetError, Transport};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RType {
    A,
    Aaaa,
    Mx,
    Txt,
    Ptr,
    Ns,
    Cname,
}

impl RType {
    pub fn as_str(self) -> &'static str {
        match self {
            RType::A => "A",
            RType::Aaaa => "AAAA",
            RType::Mx => "MX",
            RType::Txt => "TXT",
            RType::Ptr => "PTR",
            RType::Ns => "NS",
            RType::Cname => "CNAME",
        }
    }

    fn wire(self) -> RecordType {
        match self {
            RType::A => RecordType::A,
            RType::Aaaa => RecordType::AAAA,
            RType::Mx => RecordType::MX,
            RType::Txt => RecordType::TXT,
            RType::Ptr => RecordType::PTR,
            RType::Ns => RecordType::NS,
            RType::Cname => RecordType::CNAME,
        }
    }
}

/// Answers as text: an IP for A/AAAA, `<pref> <host>` for MX, the joined
/// strings for TXT, a host (no trailing dot) for PTR/NS/CNAME.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct DnsResponse {
    pub rcode: String,
    pub answers: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DnsConfig {
    /// Empty: read `/etc/resolv.conf`.
    pub nameservers: Vec<IpAddr>,
    pub timeout: Duration,
    pub attempts: u32,
}

impl Default for DnsConfig {
    fn default() -> Self {
        Self {
            nameservers: Vec::new(),
            timeout: Duration::from_secs(2),
            attempts: 2,
        }
    }
}

/// `nameserver` lines from a resolv.conf (link-local addresses with a zone
/// are skipped).
pub fn parse_resolv_conf(text: &str) -> Vec<IpAddr> {
    text.lines()
        .filter_map(|l| {
            let mut parts = l.split_whitespace();
            (parts.next() == Some("nameserver"))
                .then(|| parts.next())
                .flatten()
        })
        .filter_map(|ip| ip.parse().ok())
        .collect()
}

/// `203.0.113.24` → `24.113.0.203.in-addr.arpa`.
pub fn reverse_name(ip: IpAddr) -> String {
    match ip {
        IpAddr::V4(v4) => {
            let o = v4.octets();
            format!("{}.{}.{}.{}.in-addr.arpa", o[3], o[2], o[1], o[0])
        }
        IpAddr::V6(v6) => {
            let mut s = String::new();
            for b in v6.octets().iter().rev() {
                s.push_str(&format!("{:x}.{:x}.", b & 0xf, b >> 4));
            }
            s.push_str("ip6.arpa");
            s
        }
    }
}

#[derive(Clone)]
pub struct Dns {
    transport: Transport,
    config: DnsConfig,
}

impl Dns {
    pub fn new(transport: Transport, config: DnsConfig) -> Self {
        Self { transport, config }
    }

    /// The nameservers live lookups use.
    pub fn nameservers(&self) -> Result<Vec<SocketAddr>, NetError> {
        let ips = if self.config.nameservers.is_empty() {
            std::fs::read_to_string(Path::new("/etc/resolv.conf"))
                .map(|t| parse_resolv_conf(&t))
                .unwrap_or_default()
        } else {
            self.config.nameservers.clone()
        };
        if ips.is_empty() {
            return Err(NetError::NoNameservers);
        }
        Ok(ips.into_iter().map(|ip| SocketAddr::new(ip, 53)).collect())
    }

    pub async fn lookup(
        &self,
        module: &str,
        name: &str,
        rtype: RType,
    ) -> Result<DnsResponse, NetError> {
        let name = name.trim_end_matches('.').to_ascii_lowercase();
        if let Transport::Replay(fx) = &self.transport {
            return match fx.dns(&name, rtype.as_str()) {
                Some(Exchange::Dns { answers, rcode, .. }) => Ok(DnsResponse {
                    rcode: rcode.clone(),
                    answers: answers.clone(),
                }),
                _ => Err(NetError::NoDnsFixture {
                    name,
                    rtype: rtype.as_str().into(),
                }),
            };
        }
        let resp = self.query_live(&name, rtype).await?;
        if let Transport::Record(dir) = &self.transport {
            let ex = Exchange::Dns {
                name: name.clone(),
                rtype: rtype.as_str().into(),
                answers: resp.answers.clone(),
                rcode: resp.rcode.clone(),
            };
            record(dir, module, &ex).map_err(NetError::Record)?;
        }
        Ok(resp)
    }

    async fn query_live(&self, name: &str, rtype: RType) -> Result<DnsResponse, NetError> {
        let qname = Name::from_ascii(format!("{name}."))
            .map_err(|e| NetError::Dns(format!("bad name {name}: {e}")))?;
        let mut msg = Message::query();
        msg.metadata.recursion_desired = true;
        msg.add_query(Query::query(qname.clone(), rtype.wire()));
        let mut edns = Edns::new();
        edns.set_max_payload(1232);
        msg.set_edns(edns);
        let id = msg.metadata.id;
        let bytes = msg.to_vec().map_err(|e| NetError::Dns(e.to_string()))?;
        let servers = self.nameservers()?;
        let mut last = NetError::Dns("no attempt made".into());
        for _ in 0..self.config.attempts.max(1) {
            for ns in &servers {
                match self.exchange(*ns, &bytes, id, &qname, rtype).await {
                    Ok(r) => return Ok(r),
                    Err(e) => last = e,
                }
            }
        }
        Err(last)
    }

    async fn exchange(
        &self,
        ns: SocketAddr,
        query: &[u8],
        id: u16,
        qname: &Name,
        rtype: RType,
    ) -> Result<DnsResponse, NetError> {
        let t = self.config.timeout;
        let bind: SocketAddr = if ns.is_ipv4() {
            ([0, 0, 0, 0], 0).into()
        } else {
            (std::net::Ipv6Addr::UNSPECIFIED, 0).into()
        };
        let sock = UdpSocket::bind(bind)
            .await
            .map_err(|e| NetError::Dns(e.to_string()))?;
        sock.connect(ns)
            .await
            .map_err(|e| NetError::Dns(e.to_string()))?;
        sock.send(query)
            .await
            .map_err(|e| NetError::Dns(e.to_string()))?;
        let mut buf = vec![0u8; 4096];
        let deadline = tokio::time::Instant::now() + t;
        let resp = loop {
            let n = tokio::time::timeout_at(deadline, sock.recv(&mut buf))
                .await
                .map_err(|_| NetError::Timeout)?
                .map_err(|e| NetError::Dns(e.to_string()))?;
            // Ignore anything that isn't the answer to this question.
            if let Ok(m) = Message::from_vec(&buf[..n])
                && m.metadata.id == id
                && m.metadata.message_type == MessageType::Response
                && m.queries
                    .first()
                    .is_some_and(|q| q.name() == qname && q.query_type() == rtype.wire())
            {
                break m;
            }
        };
        let resp = if resp.metadata.truncation {
            self.tcp(ns, query, id).await?
        } else {
            resp
        };
        Ok(to_response(&resp, rtype))
    }

    async fn tcp(&self, ns: SocketAddr, query: &[u8], id: u16) -> Result<Message, NetError> {
        let t = self.config.timeout;
        let io = |e: std::io::Error| NetError::Dns(e.to_string());
        let mut s = tokio::time::timeout(t, TcpStream::connect(ns))
            .await
            .map_err(|_| NetError::Timeout)?
            .map_err(io)?;
        let len = u16::try_from(query.len()).map_err(|_| NetError::Dns("query too long".into()))?;
        let mut framed = len.to_be_bytes().to_vec();
        framed.extend_from_slice(query);
        tokio::time::timeout(t, s.write_all(&framed))
            .await
            .map_err(|_| NetError::Timeout)?
            .map_err(io)?;
        let mut lb = [0u8; 2];
        tokio::time::timeout(t, s.read_exact(&mut lb))
            .await
            .map_err(|_| NetError::Timeout)?
            .map_err(io)?;
        let mut body = vec![0u8; usize::from(u16::from_be_bytes(lb))];
        tokio::time::timeout(t, s.read_exact(&mut body))
            .await
            .map_err(|_| NetError::Timeout)?
            .map_err(io)?;
        let m = Message::from_vec(&body).map_err(|e| NetError::Dns(e.to_string()))?;
        if m.metadata.id != id {
            return Err(NetError::Dns("TCP answer id mismatch".into()));
        }
        Ok(m)
    }
}

fn host(n: &Name) -> String {
    n.to_ascii().trim_end_matches('.').to_ascii_lowercase()
}

fn to_response(m: &Message, rtype: RType) -> DnsResponse {
    let rcode = match m.metadata.response_code.low() {
        0 => "NOERROR".to_string(),
        2 => "SERVFAIL".to_string(),
        3 => "NXDOMAIN".to_string(),
        5 => "REFUSED".to_string(),
        n => format!("RCODE{n}"),
    };
    let answers = m
        .answers
        .iter()
        .filter_map(|r| match (&r.data, rtype) {
            (RData::A(a), RType::A) => Some(a.0.to_string()),
            (RData::AAAA(a), RType::Aaaa) => Some(a.0.to_string()),
            (RData::MX(mx), RType::Mx) => Some(format!("{} {}", mx.preference, host(&mx.exchange))),
            (RData::TXT(t), RType::Txt) => Some(
                t.txt_data
                    .iter()
                    .map(|b| String::from_utf8_lossy(b).into_owned())
                    .collect(),
            ),
            (RData::PTR(p), RType::Ptr) => Some(host(&p.0)),
            (RData::NS(n), RType::Ns) => Some(host(&n.0)),
            (RData::CNAME(c), RType::Cname) => Some(host(&c.0)),
            _ => None,
        })
        .collect();
    DnsResponse { rcode, answers }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolv_conf_and_reverse_names() {
        let ns = parse_resolv_conf(
            "# comment\nnameserver 9.9.9.9\nnameserver fe80::1%en0\nsearch lan\nnameserver 2620:fe::fe\n",
        );
        assert_eq!(
            ns,
            vec![
                "9.9.9.9".parse::<IpAddr>().unwrap(),
                "2620:fe::fe".parse().unwrap()
            ]
        );
        assert_eq!(
            reverse_name("203.0.113.24".parse().unwrap()),
            "24.113.0.203.in-addr.arpa"
        );
        assert!(
            reverse_name("2001:db8::1".parse().unwrap()).ends_with(".8.b.d.0.1.0.0.2.ip6.arpa")
        );
    }

    /// A local fake nameserver: answers one UDP query, checks the id and
    /// question round-trip through our encoder and decoder.
    #[tokio::test]
    async fn udp_exchange_against_a_local_server() {
        use hickory_proto::rr::Record;
        use hickory_proto::rr::rdata::A;
        let server = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let addr = server.local_addr().unwrap();
        tokio::spawn(async move {
            let mut buf = vec![0u8; 4096];
            let (n, peer) = server.recv_from(&mut buf).await.unwrap();
            let q = Message::from_vec(&buf[..n]).unwrap();
            assert!(q.metadata.recursion_desired);
            assert!(q.edns.is_some());
            // a stray packet with the wrong id first: must be ignored
            let mut wrong = Message::response(q.metadata.id.wrapping_add(1), q.metadata.op_code);
            wrong.add_queries(q.queries.clone());
            server
                .send_to(&wrong.to_vec().unwrap(), peer)
                .await
                .unwrap();
            let mut r = Message::response(q.metadata.id, q.metadata.op_code);
            r.add_queries(q.queries.clone());
            r.add_answer(Record::from_rdata(
                q.queries[0].name().clone(),
                300,
                RData::A(A::new(203, 0, 113, 24)),
            ));
            server.send_to(&r.to_vec().unwrap(), peer).await.unwrap();
        });
        let dns = Dns::new(
            Transport::Live,
            DnsConfig {
                nameservers: vec![addr.ip()],
                timeout: Duration::from_secs(2),
                attempts: 1,
            },
        );
        // nameservers() always targets port 53; exercise exchange() directly
        let mut msg = Message::query();
        msg.metadata.recursion_desired = true;
        let qname = Name::from_ascii("kestrel.example.").unwrap();
        msg.add_query(Query::query(qname.clone(), RecordType::A));
        let mut edns = Edns::new();
        edns.set_max_payload(1232);
        msg.set_edns(edns);
        let id = msg.metadata.id;
        let resp = dns
            .exchange(addr, &msg.to_vec().unwrap(), id, &qname, RType::A)
            .await
            .unwrap();
        assert_eq!(
            resp,
            DnsResponse {
                rcode: "NOERROR".into(),
                answers: vec!["203.0.113.24".into()]
            }
        );
    }
}
