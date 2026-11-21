//! Secrets at rest: what must not sit in a file as plain text (the tokens a device holds to be let into the others) is sealed
//! with what the platform keeps for the user: DPAPI on Windows, the Keystore on Android (both in the app, `src-tauri`; this
//! crate only knows the shape). A sealed file starts with [`MAGIC`]; a file without it is plain (the way it always was), so
//! an old file still opens and is sealed the next time it is written.
//!
//! Nothing here may lose a secret: when sealing fails the plain text is written (and said in the log), and a sealed file that
//! cannot be opened (the folder was moved to another computer or user) is kept as it is under another name, never overwritten.

use std::io;

/// What a sealed file starts with.
pub const MAGIC: &[u8] = b"SFYSEC1:";

/// A way of sealing bytes so that only this user on this machine can open them.
pub trait Sealer: Send + Sync {
    fn seal(&self, plain: &[u8]) -> io::Result<Vec<u8>>;
    fn open(&self, sealed: &[u8]) -> io::Result<Vec<u8>>;
    /// False for [`Plain`]: nothing is sealed, files stay as plain text (tests, and platforms without a way).
    fn protects(&self) -> bool {
        true
    }
}

/// No protection: what the files always were.
pub struct Plain;

impl Sealer for Plain {
    fn seal(&self, plain: &[u8]) -> io::Result<Vec<u8>> {
        Ok(plain.to_vec())
    }

    fn open(&self, sealed: &[u8]) -> io::Result<Vec<u8>> {
        Ok(sealed.to_vec())
    }

    fn protects(&self) -> bool {
        false
    }
}

/// What reading a file that may be sealed gave.
#[derive(Debug, PartialEq, Eq)]
pub enum Opened {
    /// The bytes of a plain file.
    Plain(Vec<u8>),
    /// The bytes of a sealed file, opened.
    Sealed(Vec<u8>),
    /// A sealed file that this user here cannot open.
    Unreadable,
}

/// Reads `bytes` as a file that may or may not be sealed.
pub fn read(sealer: &dyn Sealer, bytes: &[u8]) -> Opened {
    match bytes.strip_prefix(MAGIC) {
        None => Opened::Plain(bytes.to_vec()),
        Some(sealed) => sealer.open(sealed).map_or(Opened::Unreadable, Opened::Sealed),
    }
}

/// What to write for `plain`: sealed when the sealer protects and manages to, plain text otherwise (the secret is never lost
/// over a sealing that failed). The flag says whether it is sealed.
pub fn write(sealer: &dyn Sealer, plain: &[u8]) -> (Vec<u8>, bool) {
    if !sealer.protects() {
        return (plain.to_vec(), false);
    }
    match sealer.seal(plain) {
        Ok(sealed) => {
            let mut out = MAGIC.to_vec();
            out.extend_from_slice(&sealed);
            (out, true)
        }
        Err(error) => {
            log::warn!("Could not seal a secret file ({error}): it is written as it always was");
            (plain.to_vec(), false)
        }
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;

    /// A sealer for the tests: reverses the bytes (so a sealed file is not the plain one) and can be made to fail.
    pub struct Reverse {
        pub fail_seal: bool,
        pub fail_open: bool,
    }

    impl Sealer for Reverse {
        fn seal(&self, plain: &[u8]) -> io::Result<Vec<u8>> {
            if self.fail_seal {
                return Err(io::Error::other("no way to seal"));
            }
            Ok(plain.iter().rev().copied().collect())
        }

        fn open(&self, sealed: &[u8]) -> io::Result<Vec<u8>> {
            if self.fail_open {
                return Err(io::Error::other("another user"));
            }
            Ok(sealed.iter().rev().copied().collect())
        }
    }

    #[test]
    fn a_sealed_file_opens_and_a_plain_one_still_does() {
        let sealer = Reverse { fail_seal: false, fail_open: false };
        let (bytes, sealed) = write(&sealer, b"[\"token\"]");
        assert!(sealed && bytes.starts_with(MAGIC) && !bytes.windows(5).any(|w| w == b"token"));
        assert_eq!(read(&sealer, &bytes), Opened::Sealed(b"[\"token\"]".to_vec()));
        assert_eq!(read(&sealer, b"[1]"), Opened::Plain(b"[1]".to_vec()));
    }

    #[test]
    fn a_secret_is_never_lost_to_a_sealing_that_fails_and_an_unopenable_file_is_said_so() {
        let (bytes, sealed) = write(&Reverse { fail_seal: true, fail_open: false }, b"[1]");
        assert!(!sealed);
        assert_eq!(bytes, b"[1]");
        let (sealed_bytes, _) = write(&Reverse { fail_seal: false, fail_open: false }, b"[1]");
        assert_eq!(read(&Reverse { fail_seal: false, fail_open: true }, &sealed_bytes), Opened::Unreadable);
        // The plain sealer leaves files as they are.
        assert_eq!(write(&Plain, b"[1]"), (b"[1]".to_vec(), false));
    }
}
