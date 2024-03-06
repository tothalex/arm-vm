#[derive(Debug)]
pub enum SerialOut {
    Sink(std::io::Sink),
    Stdout(std::io::Stdout),
}

impl std::io::Write for SerialOut {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        match self {
            Self::Sink(sink) => sink.write(buf),
            Self::Stdout(stdout) => stdout.write(buf),
        }
    }
    fn flush(&mut self) -> std::io::Result<()> {
        match self {
            Self::Sink(sink) => sink.flush(),
            Self::Stdout(stdout) => stdout.flush(),
        }
    }
}
