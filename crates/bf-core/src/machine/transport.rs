use std::io;
use std::time::{Duration, Instant};

/// A byte pipe to a controller: a serial port, a TCP socket, or a simulator
/// in tests. Implementations live where their I/O lives (`src-tauri` for the
/// real ports); everything above this trait is shared.
pub trait Transport: Send {
    /// Writes all of `bytes`.
    fn write(&mut self, bytes: &[u8]) -> io::Result<()>;

    /// Reads whatever has arrived, waiting up to `timeout` for the first byte.
    /// Returns `Ok(0)` only when the timeout passed with nothing to read; a
    /// closed or unplugged port is an error, never `Ok(0)`.
    fn read(&mut self, buf: &mut [u8], timeout: Duration) -> io::Result<usize>;
}

impl<T: Transport + ?Sized> Transport for Box<T> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<()> {
        (**self).write(bytes)
    }

    fn read(&mut self, buf: &mut [u8], timeout: Duration) -> io::Result<usize> {
        (**self).read(buf, timeout)
    }
}

/// Time as the handshake sees it. The simulator's clock stands in for the
/// wall clock in tests, so timeouts are deterministic.
pub trait Clock {
    fn now(&self) -> Duration;
}

/// The wall clock, measured from when it was created.
pub struct SystemClock(Instant);

impl SystemClock {
    pub fn new() -> Self {
        SystemClock(Instant::now())
    }
}

impl Default for SystemClock {
    fn default() -> Self {
        Self::new()
    }
}

impl Clock for SystemClock {
    fn now(&self) -> Duration {
        self.0.elapsed()
    }
}
