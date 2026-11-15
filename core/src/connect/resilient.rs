//! A stream that goes on when a call is interrupted by a signal.
//!
//! On Android a blocking `read` or `write` can end with `EINTR` («Interrupted system call», os error 4) when the process gets a signal (the runtime
//! uses them). Rust's `read` does not retry it, and the WebSocket library sees an `io::Error` and ends the connection: on the HONOR tablet that was
//! a connection every second or two, in both directions, until Android warned «Soundify drains the battery». An interrupted call is not a failure:
//! it is called again. Everything that reads or writes a socket here goes through [`Resilient`] (or [`read_retrying`] where it is a plain `TcpStream`).

use std::io::{self, Read, Write};

/// Reads like `read`, trying again while the call is interrupted.
pub fn read_retrying<R: Read>(reader: &mut R, buf: &mut [u8]) -> io::Result<usize> {
    loop {
        match reader.read(buf) {
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            other => return other,
        }
    }
}

/// A stream whose `read`, `write` and `flush` are called again when they are interrupted.
pub struct Resilient<S>(S);

impl<S> Resilient<S> {
    pub fn new(stream: S) -> Self {
        Self(stream)
    }

    /// The stream inside (to set its timeouts, for instance).
    pub fn inner(&self) -> &S {
        &self.0
    }
}

impl<S: Read> Read for Resilient<S> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        read_retrying(&mut self.0, buf)
    }
}

impl<S: Write> Write for Resilient<S> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        loop {
            match self.0.write(buf) {
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                other => return other,
            }
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        loop {
            match self.0.flush() {
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                other => return other,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Fails `interruptions` times with `Interrupted` before every call that works.
    struct Flaky {
        interruptions: usize,
        data: Vec<u8>,
        written: Vec<u8>,
    }

    impl Read for Flaky {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            if self.interruptions > 0 {
                self.interruptions -= 1;
                return Err(io::Error::from(io::ErrorKind::Interrupted));
            }
            let n = self.data.len().min(buf.len());
            buf[..n].copy_from_slice(&self.data[..n]);
            self.data.drain(..n);
            Ok(n)
        }
    }

    impl Write for Flaky {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            if self.interruptions > 0 {
                self.interruptions -= 1;
                return Err(io::Error::from(io::ErrorKind::Interrupted));
            }
            self.written.extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            if self.interruptions > 0 {
                self.interruptions -= 1;
                return Err(io::Error::from(io::ErrorKind::Interrupted));
            }
            Ok(())
        }
    }

    #[test]
    fn an_interrupted_read_is_read_again() {
        let mut stream = Resilient::new(Flaky { interruptions: 3, data: b"hola".to_vec(), written: vec![] });
        let mut buf = [0u8; 8];
        assert_eq!(stream.read(&mut buf).unwrap(), 4);
        assert_eq!(&buf[..4], b"hola");
    }

    #[test]
    fn an_interrupted_write_and_flush_go_on() {
        let mut stream = Resilient::new(Flaky { interruptions: 2, data: vec![], written: vec![] });
        stream.write_all(b"adios").unwrap();
        let mut stream = Resilient::new(Flaky { interruptions: 2, data: vec![], written: vec![] });
        stream.flush().unwrap();
    }

    #[test]
    fn a_real_error_is_not_swallowed() {
        struct Broken;
        impl Read for Broken {
            fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
                Err(io::Error::from(io::ErrorKind::ConnectionReset))
            }
        }
        let mut buf = [0u8; 4];
        assert_eq!(read_retrying(&mut Broken, &mut buf).unwrap_err().kind(), io::ErrorKind::ConnectionReset);
    }
}
