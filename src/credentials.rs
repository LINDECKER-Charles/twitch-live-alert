use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::{fs, io::Write, path::Path};
use zeroize::{Zeroize, Zeroizing};

pub(crate) const CREDENTIALS_FILE: &str = "data/twitch.dpapi";

// These values may only leave the process in a TLS request to Twitch or encrypted by Windows.
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Credentials {
    pub(crate) client_id: String,
    pub(crate) client_secret: String,
}

impl Drop for Credentials {
    fn drop(&mut self) {
        self.client_id.zeroize();
        self.client_secret.zeroize();
    }
}

impl Credentials {
    pub(crate) fn valid(&self) -> bool {
        [&self.client_id, &self.client_secret]
            .into_iter()
            .all(|value| {
                !value.is_empty()
                    && value.len() <= 512
                    && value.bytes().all(|byte| byte.is_ascii_graphic())
            })
    }

    pub(crate) fn load(path: &Path) -> Result<Option<Self>> {
        let encrypted = match fs::read(path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(_) => bail!("Impossible de lire les identifiants locaux"),
        };
        let plaintext = Zeroizing::new(protect(&encrypted, false)?);
        let credentials: Self = serde_json::from_slice(&plaintext)
            .map_err(|_| anyhow::anyhow!("Identifiants locaux illisibles"))?;
        anyhow::ensure!(credentials.valid(), "Identifiants locaux invalides");
        Ok(Some(credentials))
    }

    pub(crate) fn save(&self, path: &Path) -> Result<()> {
        anyhow::ensure!(self.valid(), "Identifiants invalides");
        let plaintext = Zeroizing::new(serde_json::to_vec(self)?);
        let encrypted = protect(&plaintext, true)?;
        let directory = path.parent().context("Chemin de stockage invalide")?;
        fs::create_dir_all(directory)?;
        let mut file = tempfile::NamedTempFile::new_in(directory)?;
        file.write_all(&encrypted)?;
        file.as_file().sync_all()?;
        file.persist(path)?;
        Ok(())
    }
}

#[cfg(windows)]
pub(crate) fn protect(bytes: &[u8], encrypt: bool) -> Result<Vec<u8>> {
    use std::ptr::{null, null_mut};
    use windows_sys::Win32::{
        Foundation::LocalFree,
        Security::Cryptography::{
            CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData, CryptUnprotectData,
        },
    };
    let input = CRYPT_INTEGER_BLOB {
        cbData: u32::try_from(bytes.len())?,
        pbData: bytes.as_ptr().cast_mut(),
    };
    let mut output = CRYPT_INTEGER_BLOB::default();
    // Both routines borrow the input for this call and allocate output using LocalAlloc.
    // Omitting CRYPTPROTECT_LOCAL_MACHINE binds decryption to the current Windows account.
    let ok = unsafe {
        if encrypt {
            CryptProtectData(
                &input,
                null(),
                null(),
                null(),
                null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        } else {
            CryptUnprotectData(
                &input,
                null_mut(),
                null(),
                null(),
                null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        }
    };
    if ok == 0 {
        bail!("Windows n’a pas pu protéger ou ouvrir les identifiants de ce compte");
    }
    let result = if output.cbData == 0 || output.pbData.is_null() {
        Vec::new()
    } else {
        let native =
            unsafe { std::slice::from_raw_parts_mut(output.pbData, output.cbData as usize) };
        let copy = native.to_vec();
        if !encrypt {
            native.zeroize();
        }
        copy
    };
    unsafe { LocalFree(output.pbData.cast()) };
    Ok(result)
}

#[cfg(not(windows))]
pub(crate) fn protect(_: &[u8], _: bool) -> Result<Vec<u8>> {
    bail!("Le stockage sécurisé des identifiants nécessite Windows")
}
