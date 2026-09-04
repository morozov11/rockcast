//! Bounded tungstenite adapter for device-control v1.
//! Authentication headers are constructed here and never logged.

use super::protocol::{ControlError, Inbound, MAX_FRAME_BYTES};
use std::io;
use tungstenite::{
    Message, WebSocket,
    client::{IntoClientRequest, connect_with_config},
    http::{HeaderValue, header::AUTHORIZATION},
    protocol::WebSocketConfig,
    stream::MaybeTlsStream,
};

pub(super) trait ControlSocket: Send {
    fn send_text(&mut self, text: String) -> Result<(), ControlError>;
    fn read(&mut self) -> Result<Option<Inbound>, ControlError>;
    fn close(&mut self);
}

pub(super) trait DeviceControlTransport: Send + Sync {
    fn connect(&self, endpoint: &str, token: &str) -> Result<Box<dyn ControlSocket>, ControlError>;
}

pub(super) struct TungsteniteTransport;

struct TungsteniteSocket(WebSocket<MaybeTlsStream<std::net::TcpStream>>);

impl ControlSocket for TungsteniteSocket {
    fn send_text(&mut self, text: String) -> Result<(), ControlError> {
        self.0
            .send(Message::Text(text.into()))
            .map_err(|_| ControlError::Unavailable)
    }

    fn read(&mut self) -> Result<Option<Inbound>, ControlError> {
        match self.0.read() {
            Ok(Message::Text(text)) => Ok(Some(Inbound::Text(text.to_string()))),
            Ok(Message::Ping(payload)) => Ok(Some(Inbound::Ping(payload.to_vec()))),
            Ok(Message::Close(_)) => Ok(Some(Inbound::Close)),
            Ok(_) => Err(ControlError::Protocol),
            Err(tungstenite::Error::Io(error))
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                ) =>
            {
                Ok(None)
            }
            Err(tungstenite::Error::Http(response)) if response.status().as_u16() == 401 => {
                Err(ControlError::Authentication)
            }
            Err(_) => Err(ControlError::Unavailable),
        }
    }

    fn close(&mut self) {
        let _ = self.0.close(None);
    }
}

impl DeviceControlTransport for TungsteniteTransport {
    fn connect(&self, endpoint: &str, token: &str) -> Result<Box<dyn ControlSocket>, ControlError> {
        let mut request = endpoint
            .into_client_request()
            .map_err(|_| ControlError::Unavailable)?;
        let value = HeaderValue::from_str(&format!("Bearer {token}"))
            .map_err(|_| ControlError::Authentication)?;
        request.headers_mut().insert(AUTHORIZATION, value);
        let config = WebSocketConfig::default()
            .read_buffer_size(8 * 1024)
            .write_buffer_size(8 * 1024)
            .max_message_size(Some(MAX_FRAME_BYTES))
            .max_frame_size(Some(MAX_FRAME_BYTES));
        let (mut socket, _) = connect_with_config(request, Some(config), 0).map_err(|error| {
            if matches!(error, tungstenite::Error::Http(ref response) if response.status().as_u16() == 401) {
                ControlError::Authentication
            } else {
                ControlError::Unavailable
            }
        })?;
        set_nonblocking(socket.get_mut()).map_err(|_| ControlError::Unavailable)?;
        Ok(Box::new(TungsteniteSocket(socket)))
    }
}

fn set_nonblocking(stream: &mut MaybeTlsStream<std::net::TcpStream>) -> io::Result<()> {
    match stream {
        MaybeTlsStream::Plain(stream) => stream.set_nonblocking(true),
        MaybeTlsStream::Rustls(stream) => stream.sock.set_nonblocking(true),
        _ => Ok(()),
    }
}
