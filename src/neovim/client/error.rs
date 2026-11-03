use nvim_rs::error::CallError;
use std::{error::Error, fmt};

#[derive(Debug)]
pub enum ClientError {
    NotConnected,
    Timeout(&'static str),
    Disconnected {
        operation: &'static str,
        source: Box<CallError>,
    },
    Rpc {
        operation: &'static str,
        source: Box<CallError>,
    },
    InvalidResponse(&'static str),
}

impl ClientError {
    pub(super) fn rpc(operation: &'static str, source: Box<CallError>) -> Self {
        if source.is_channel_closed() {
            Self::Disconnected { operation, source }
        } else {
            Self::Rpc { operation, source }
        }
    }

    pub fn requires_recovery(&self) -> bool {
        matches!(
            self,
            Self::NotConnected | Self::Timeout(_) | Self::Disconnected { .. }
        )
    }
}

impl fmt::Display for ClientError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotConnected => write!(f, "Neovim not connected"),
            Self::Timeout(operation) => write!(f, "Timeout {operation}"),
            Self::Disconnected { operation, source } | Self::Rpc { operation, source } => {
                write!(f, "Failed to {operation}: {source}")
            }
            Self::InvalidResponse(message) => f.write_str(message),
        }
    }
}

impl Error for ClientError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Disconnected { source, .. } | Self::Rpc { source, .. } => Some(source.as_ref()),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nvim_rs::error::EncodeError;

    #[test]
    fn recovery_depends_on_error_kind_not_display_text() {
        let closed = ClientError::rpc(
            "send input",
            Box::new(CallError::SendError(
                EncodeError::WriterError(std::io::ErrorKind::BrokenPipe.into()),
                "input".into(),
            )),
        );
        assert!(closed.requires_recovery());
        let rpc = ClientError::rpc(
            "execute Lua",
            Box::new(CallError::NeovimError(
                None,
                "Timeout: Failed to run user code".into(),
            )),
        );
        assert!(!rpc.requires_recovery());
        assert!(!ClientError::InvalidResponse("Failed to parse response").requires_recovery());
        assert!(ClientError::Timeout("getting cursor").requires_recovery());
        assert!(ClientError::NotConnected.requires_recovery());
    }
}
