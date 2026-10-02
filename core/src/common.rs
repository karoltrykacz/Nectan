use std::path::{self, Component, PathBuf};

use base64::{Engine, engine::general_purpose::STANDARD};
use ed25519_dalek::SigningKey;
use rand::{rand_core::UnwrapErr, rngs::SysRng};

use crate::storage::{
    Key::{self},
    Storage, Value,
};

// fn validate_path_component(component: &str) -> anyhow::Result<()> {
//     anyhow::ensure!(
//         !component.contains('/'),
//         "path components must not contain the only correct path separator, /"
//     );
//     Ok(())
// }

/// Only for absolute paths
pub fn common_parent(paths: &[PathBuf]) -> PathBuf {
    let Some(first) = paths.first() else {
        return PathBuf::new();
    };
    let mut common = PathBuf::new();

    for (idx, component) in first.components().enumerate() {
        if paths
            .iter()
            .any(|p| p.components().nth(idx) != Some(component))
        {
            break;
        }
        common.push(component);
    }

    common
}

pub fn get_signing_key(storage: &Storage) -> SigningKey {
    if let Some(Value::Key(key)) = storage.get(Key::Key) {
        key
    } else {
        let mut csprng = UnwrapErr(SysRng);
        let signing_key = SigningKey::generate(&mut csprng);
        let _ = storage.set(Value::Key(signing_key.clone()));
        signing_key
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

pub fn gen_transfer_name() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};

    const ADJECTIVES: &[&str] = &[
        "Red", "Blue", "Green", "Yellow", "Pink", "Purple", "Orange", "White", "Black", "Gold",
        "Silver", "Neon", "Round", "Square", "Tiny", "Huge", "Long", "Short", "Tall", "Flat",
        "Happy", "Sad", "Angry", "Calm", "Shy", "Silly", "Sleepy", "Brave", "Proud", "Scared",
        "Soft", "Hard", "Fast", "Slow", "Warm", "Cold", "Sweet", "Fresh", "Bright", "Dark",
        "Clean", "Shiny",
    ];

    const CREATURES: &[&str] = &[
        "Goblin",
        "Blobfish",
        "Wombat",
        "Capybara",
        "Chupacabra",
        "Monster",
        "Ferret",
        "Axolotl",
        "Gargoyle",
        "Panda",
        "Pigeon",
        "Penguin",
        "Manatee",
        "Narwhal",
        "Seagull",
        "Shrimp",
        "Octopus",
        "Pelican",
        "Duckling",
        "Walrus",
        "Raccoon",
        "Possum",
        "Badger",
        "Sasquatch",
        "Yeti",
        "Hydra",
        "Gnome",
        "Hamster",
        "Chinchilla",
        "Sloth",
        "Mothman",
        "Beetle",
        "Caterpillar",
        "Toad",
        "Salamander",
        "Potato",
        "Nugget",
        "Cactus",
        "Gremlin",
        "Kraken",
    ];

    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .subsec_nanos() as usize;

    let seed = nanos.wrapping_mul(2654435761);
    let adj = ADJECTIVES[seed % ADJECTIVES.len()];
    let creature = CREATURES[(seed / ADJECTIVES.len()) % CREATURES.len()];

    format!("{adj} {creature}")
}
