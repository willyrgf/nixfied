//! Unpredictable runtime-generated identifiers. Callers still check collisions
//! against the durable namespace they allocate into.

use crate::error::{ErrorCode, RuntimeError, RuntimeResult};

/// 128 bits from the kernel entropy source, hex encoded.
pub(crate) fn random_hex() -> RuntimeResult<String> {
    let mut bytes = [0_u8; 16];
    // SAFETY: getentropy writes at most the supplied length (<= 256 bytes).
    if unsafe { libc::getentropy(bytes.as_mut_ptr().cast(), bytes.len()) } != 0 {
        return Err(RuntimeError::new(
            ErrorCode::StateUnwritable,
            format!(
                "failed to obtain identifier entropy: {}",
                std::io::Error::last_os_error()
            ),
        ));
    }
    Ok(hex::encode(bytes))
}
