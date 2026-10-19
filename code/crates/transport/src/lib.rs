#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportKind {
    Tcp,
    SharedMemory,
    Usb,
}

pub trait Transport: Send + Sync {
    fn kind(&self) -> TransportKind;
}
