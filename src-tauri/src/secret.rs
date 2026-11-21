//! The platform's way of sealing the secrets this device keeps (`soundify_core::secret`): DPAPI on Windows (only this Windows user
//! can open what it sealed), the Keystore on Android (`SoundifyPlugin.sealSecret`). Elsewhere nothing is sealed.

use std::sync::Arc;

use soundify_core::secret::Sealer;

#[cfg(windows)]
mod dpapi {
    use std::io;

    use soundify_core::secret::Sealer;
    use windows::Win32::Foundation::{HLOCAL, LocalFree};
    use windows::Win32::Security::Cryptography::{CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData, CryptUnprotectData};

    /// Windows' own protection of data for a user (no password to keep: it is the user's login).
    pub struct Dpapi;

    fn blob(bytes: &[u8]) -> io::Result<CRYPT_INTEGER_BLOB> {
        let cbdata = u32::try_from(bytes.len()).map_err(|_| io::Error::other("too big to seal"))?;
        Ok(CRYPT_INTEGER_BLOB { cbData: cbdata, pbData: bytes.as_ptr().cast_mut() })
    }

    /// Copies what Windows returned and gives its memory back.
    fn take(out: CRYPT_INTEGER_BLOB) -> Vec<u8> {
        // SAFETY: Windows filled `out` with `cbData` bytes at `pbData`, allocated for us to free with `LocalFree`.
        unsafe {
            let bytes = std::slice::from_raw_parts(out.pbData, out.cbData as usize).to_vec();
            let _ = LocalFree(Some(HLOCAL(out.pbData.cast())));
            bytes
        }
    }

    impl Sealer for Dpapi {
        fn seal(&self, plain: &[u8]) -> io::Result<Vec<u8>> {
            let input = blob(plain)?;
            let mut out = CRYPT_INTEGER_BLOB::default();
            // SAFETY: both blobs live through the call; Windows allocates `out`.
            unsafe { CryptProtectData(&input, None, None, None, None, CRYPTPROTECT_UI_FORBIDDEN, &mut out) }.map_err(io::Error::other)?;
            Ok(take(out))
        }

        fn open(&self, sealed: &[u8]) -> io::Result<Vec<u8>> {
            let input = blob(sealed)?;
            let mut out = CRYPT_INTEGER_BLOB::default();
            // SAFETY: as above.
            unsafe { CryptUnprotectData(&input, None, None, None, None, CRYPTPROTECT_UI_FORBIDDEN, &mut out) }.map_err(io::Error::other)?;
            Ok(take(out))
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn dpapi_seals_for_this_user_and_gives_it_back() {
            let sealed = Dpapi.seal(b"a token").unwrap();
            assert!(!sealed.windows(7).any(|w| w == b"a token"));
            assert_eq!(Dpapi.open(&sealed).unwrap(), b"a token");
            // What is not something DPAPI sealed does not open.
            assert!(Dpapi.open(b"not sealed by anybody").is_err());
        }
    }
}

/// The sealer of this platform (a plain one where there is no way).
#[cfg_attr(not(target_os = "android"), allow(unused_variables))]
pub fn platform(app: &tauri::AppHandle) -> Arc<dyn Sealer> {
    #[cfg(windows)]
    {
        Arc::new(dpapi::Dpapi)
    }
    #[cfg(target_os = "android")]
    {
        Arc::new(crate::mobile::KeystoreSealer::new(app))
    }
    #[cfg(not(any(windows, target_os = "android")))]
    {
        Arc::new(soundify_core::secret::Plain)
    }
}
