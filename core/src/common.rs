use base64::{Engine, engine::general_purpose::STANDARD};
use ed25519_dalek::SigningKey;
use rand::{rand_core::UnwrapErr, rngs::SysRng};

use crate::storage_utils::KvStore;

// fn validate_path_component(component: &str) -> anyhow::Result<()> {
//     anyhow::ensure!(
//         !component.contains('/'),
//         "path components must not contain the only correct path separator, /"
//     );
//     Ok(())
// }

pub fn get_signing_key(store: &KvStore) -> SigningKey {
    match store.get("key") {
        Some(bytes) => {
            let signing_key_str = bytes.as_str().expect("signing_key is not an string");
            signing_key_from_string(signing_key_str).expect("Failed to deserialize signing key")
        }
        None => {
            let mut csprng = UnwrapErr(SysRng);
            let signing_key = SigningKey::generate(&mut csprng);
            store
                .set(
                    "key",
                    serde_json::Value::String(signing_key_to_string(&signing_key)),
                )
                .expect("Failed to set signing key.");
            signing_key
        }
    }
}

pub fn signing_key_to_string(key: &ed25519_dalek::SigningKey) -> String {
    STANDARD.encode(key.as_bytes())
}

pub fn signing_key_from_string(key: &str) -> Result<SigningKey, base64::DecodeError> {
    let bytes = STANDARD.decode(key)?;
    let arr: [u8; 32] = bytes
        .try_into()
        .map_err(|_| base64::DecodeError::InvalidLength(32))?;
    Ok(SigningKey::from_bytes(&arr))
}
