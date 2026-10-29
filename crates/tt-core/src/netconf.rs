//! NETCONF over SSH framing (RFC 6242): 1.0 end-of-message and 1.1 chunked.
use anyhow::{Result, bail};

pub const EOM: &str = "]]>]]>";
pub const BASE_10: &str = "urn:ietf:params:netconf:base:1.0";
pub const BASE_11: &str = "urn:ietf:params:netconf:base:1.1";

pub fn client_hello() -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><hello xmlns=\"urn:ietf:params:xml:ns:netconf:base:1.0\"><capabilities><capability>{BASE_10}</capability><capability>{BASE_11}</capability></capabilities></hello>"
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Framing {
    Eom,
    Chunked,
}

/// Frame an outgoing message.
pub fn encode(framing: Framing, msg: &str) -> Vec<u8> {
    match framing {
        Framing::Eom => format!("{msg}{EOM}").into_bytes(),
        Framing::Chunked => {
            if msg.is_empty() {
                return b"\n##\n".to_vec();
            }
            format!("\n#{}\n{}\n##\n", msg.len(), msg).into_bytes()
        }
    }
}

/// Pull one complete message out of `buf` (removing it). Ok(None) = need more data.
pub fn decode(framing: Framing, buf: &mut Vec<u8>) -> Result<Option<String>> {
    match framing {
        Framing::Eom => {
            let pat = EOM.as_bytes();
            if let Some(pos) = buf.windows(pat.len()).position(|w| w == pat) {
                let msg: Vec<u8> = buf.drain(..pos + pat.len()).take(pos).collect();
                return Ok(Some(String::from_utf8(msg)?.trim().to_string()));
            }
            Ok(None)
        }
        Framing::Chunked => {
            let mut out = Vec::new();
            let mut i = 0;
            loop {
                // need at least "\n#N\n" or "\n##\n"
                if buf.len() < i + 4 {
                    return Ok(None);
                }
                if buf[i] != b'\n' || buf[i + 1] != b'#' {
                    bail!("invalid chunk header");
                }
                if buf[i + 2] == b'#' {
                    if buf[i + 3] != b'\n' {
                        bail!("invalid end-of-chunks marker");
                    }
                    buf.drain(..i + 4);
                    return Ok(Some(String::from_utf8(out)?));
                }
                let Some(nl) = buf[i + 2..].iter().position(|&b| b == b'\n') else {
                    return Ok(None);
                };
                let size: usize = std::str::from_utf8(&buf[i + 2..i + 2 + nl])?.parse()?;
                let start = i + 2 + nl + 1;
                if buf.len() < start + size {
                    return Ok(None);
                }
                out.extend_from_slice(&buf[start..start + size]);
                i = start + size;
            }
        }
    }
}

/// Chooses the framing from the server `<hello>`: chunked only if both sides offer base:1.1.
pub fn negotiate(server_hello: &str) -> Framing {
    if server_hello.contains(BASE_11) { Framing::Chunked } else { Framing::Eom }
}

/// Wraps an operation body in an `<rpc>` envelope.
pub fn rpc(message_id: u32, body: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><rpc message-id=\"{message_id}\" xmlns=\"urn:ietf:params:xml:ns:netconf:base:1.0\">{body}</rpc>"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eom_roundtrip_and_partial() {
        let mut buf = encode(Framing::Eom, "<a/>");
        buf.extend(b"<b");
        assert_eq!(decode(Framing::Eom, &mut buf).unwrap().as_deref(), Some("<a/>"));
        assert_eq!(decode(Framing::Eom, &mut buf).unwrap(), None);
    }

    #[test]
    fn chunked_roundtrip_multi_chunk() {
        let mut buf = encode(Framing::Chunked, "<rpc-reply/>");
        assert_eq!(decode(Framing::Chunked, &mut buf[..5].to_vec()).unwrap(), None);
        assert_eq!(decode(Framing::Chunked, &mut buf).unwrap().as_deref(), Some("<rpc-reply/>"));
        let mut two = b"\n#3\n<a>\n#4\n</a>\n##\n".to_vec();
        assert_eq!(decode(Framing::Chunked, &mut two).unwrap().as_deref(), Some("<a></a>"));
        assert!(two.is_empty());
    }

    #[test]
    fn negotiation() {
        assert_eq!(negotiate(&client_hello()), Framing::Chunked);
        assert_eq!(negotiate("<capability>urn:ietf:params:netconf:base:1.0</capability>"), Framing::Eom);
    }
}
