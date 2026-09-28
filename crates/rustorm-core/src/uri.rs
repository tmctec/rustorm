//! Connection URIs: `[user@]host[:port]`, with bracketed IPv6 literals.

use crate::{Error, Result};

/// A parsed connection URI. Missing parts are `None`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectionUri {
    /// The user before `@`.
    pub user: Option<String>,
    /// The host, without brackets for an IPv6 literal.
    pub host: String,
    /// The port after `:`.
    pub port: Option<u16>,
}

impl ConnectionUri {
    /// Parses `[user@]host[:port]`. IPv6 literals go in brackets:
    /// `[2001:db8::1]:22`. A bare IPv6 literal without brackets is taken
    /// whole as the host. A non-numeric or out-of-range port is an error.
    ///
    /// ```
    /// use rustorm_core::ConnectionUri;
    /// let u = ConnectionUri::parse("root@vps.example.com:2222").unwrap();
    /// assert_eq!(u.user.as_deref(), Some("root"));
    /// assert_eq!(u.host, "vps.example.com");
    /// assert_eq!(u.port, Some(2222));
    /// ```
    pub fn parse(uri: &str) -> Result<ConnectionUri> {
        let invalid = |reason: &str| Error::InvalidUri {
            uri: uri.to_string(),
            reason: reason.to_string(),
        };
        if uri.is_empty() || uri.chars().any(char::is_whitespace) {
            return Err(invalid("expected [user@]host[:port]"));
        }
        let (user, rest) = match uri.rsplit_once('@') {
            Some((u, r)) if !u.is_empty() => (Some(u.to_string()), r),
            Some(_) => return Err(invalid("the user before @ is empty")),
            None => (None, uri),
        };
        let (host, port) = if let Some(after) = rest.strip_prefix('[') {
            let (host, tail) = after
                .split_once(']')
                .ok_or_else(|| invalid("missing ] after the IPv6 address"))?;
            let port = match tail {
                "" => None,
                t => Some(
                    t.strip_prefix(':')
                        .ok_or_else(|| invalid("expected :port after ]"))?,
                ),
            };
            (host.to_string(), port)
        } else if rest.matches(':').count() > 1 {
            (rest.to_string(), None)
        } else {
            match rest.split_once(':') {
                Some((h, p)) => (h.to_string(), Some(p)),
                None => (rest.to_string(), None),
            }
        };
        if host.is_empty() {
            return Err(invalid("the host is empty"));
        }
        let port =
            match port {
                None => None,
                Some(p) => Some(p.parse::<u16>().ok().filter(|n| *n > 0).ok_or_else(|| {
                    invalid(&format!("port {p} is not a number from 1 to 65535"))
                })?),
            };
        Ok(ConnectionUri { user, host, port })
    }
}

impl std::str::FromStr for ConnectionUri {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self> {
        ConnectionUri::parse(s)
    }
}
