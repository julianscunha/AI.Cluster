use async_trait::async_trait;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportKind {
    Tcp,
    SharedMemory,
    Usb,
}

#[async_trait]
pub trait Transport: Send + Sync {
    async fn probe(&self) -> Result<(), String>;
    fn kind(&self) -> TransportKind;
}

pub struct TcpTransport;

#[async_trait]
impl Transport for TcpTransport {
    async fn probe(&self) -> Result<(), String> {
        Ok(())
    }

    fn kind(&self) -> TransportKind {
        TransportKind::Tcp
    }
}
