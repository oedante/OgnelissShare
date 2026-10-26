//! Мелкие помощники: время, идентификаторы, имена.

use std::time::{SystemTime, UNIX_EPOCH};

use data_encoding::BASE32_NOPAD;
use iroh::EndpointId;

/// Секунды Unix — для отметок «добавлено» и «последний раз в сети».
pub fn now_unix() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// Случайный идентификатор (передачи, запроса) — 16 символов base32.
pub fn random_id() -> String {
    let bytes: [u8; 10] = rand::random();
    BASE32_NOPAD.encode(&bytes).to_lowercase()
}

/// Короткий код устройства для людей: 4 символа из отпечатка ключа.
/// Показывается рядом с именем, чтобы два «Android» различались.
pub fn short_code(id: &EndpointId) -> String {
    let hash = blake3::hash(id.as_bytes());
    BASE32_NOPAD.encode(&hash.as_bytes()[..3])[..4].to_string()
}

/// Имя устройства: без управляющих символов, обрезанное до 40 символов.
pub fn clean_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .filter(|c| !c.is_control())
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let trimmed: String = cleaned.chars().take(40).collect();
    if trimmed.is_empty() { "Устройство".to_string() } else { trimmed }
}

/// Платформа — одно из известных значений, иначе «other».
pub fn clean_platform(p: &str) -> String {
    match p {
        "android" | "ios" | "windows" | "macos" | "linux" => p.to_string(),
        _ => "other".to_string(),
    }
}

/// Сравнение секретов без утечки по времени.
pub fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b) {
        diff |= x ^ y;
    }
    diff == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_cleaned() {
        assert_eq!(clean_name("  Ноутбук\n Олега  "), "Ноутбук Олега");
        assert_eq!(clean_name(""), "Устройство");
        assert_eq!(clean_name(&"x".repeat(100)).chars().count(), 40);
    }

    #[test]
    fn constant_time_compare() {
        assert!(ct_eq(b"abc", b"abc"));
        assert!(!ct_eq(b"abc", b"abd"));
        assert!(!ct_eq(b"abc", b"ab"));
    }
}
