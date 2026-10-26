//! Протокол ogneliss-share/1 поверх QUIC-потоков iroh.
//!
//! Каждый запрос — отдельный двунаправленный поток. Управляющие сообщения —
//! JSON с префиксом длины (4 байта, big-endian); содержимое файлов идёт
//! между ними сырыми байтами ровно указанной длины. Полное описание —
//! docs/PROTOCOL.md.

use anyhow::{Context, Result, bail};
use iroh::EndpointAddr;
use iroh::endpoint::{RecvStream, SendStream};
use serde::{Deserialize, Serialize};

/// ALPN протокола. Смена несовместимого формата — новая версия в конце.
pub const ALPN: &[u8] = b"ogneliss-share/1";

/// Предел управляющего сообщения: список из десятков тысяч файлов влезает.
pub const MAX_MSG: usize = 8 * 1024 * 1024;

/// Сообщение читается кусками не больше этого.
const CHUNK: usize = 64 * 1024;

/// Коды закрытия потоков и соединений.
pub const CODE_CANCELLED: u32 = 1;
/// Устройство не связано с нами: соединение отклонено.
pub const CODE_NOT_PAIRED: u32 = 403;
/// Связь разорвана владельцем: «Забыть устройство» или конец разового обмена.
pub const CODE_REMOVED: u32 = 410;
/// Слишком много одновременных попыток сопряжения.
pub const CODE_BUSY: u32 = 429;

/// Предел текста в одном сообщении.
pub const MAX_TEXT: usize = 1024 * 1024;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct FileMeta {
    /// Относительный путь с разделителем «/», например «Фото/IMG_0001.jpg».
    pub path: String,
    pub size: u64,
    /// Время изменения у отправителя (секунды Unix, 0 — неизвестно).
    /// Сохраняется у получателя и входит в ключ докачки: файл изменился —
    /// докачки не будет, придёт заново.
    #[serde(default)]
    pub mtime: u64,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum Msg {
    /// Приветствие при соединении: имя, платформа и текущие адреса.
    Hello {
        name: String,
        platform: String,
        version: String,
        #[serde(default)]
        addr: Option<EndpointAddr>,
    },
    /// Запрос сопряжения по одноразовому коду из QR.
    Pair {
        token: String,
        name: String,
        platform: String,
        mode: String,
    },
    PairOk {
        name: String,
        platform: String,
    },
    PairDenied {
        reason: String,
    },
    /// Предложение файлов. Получатель отвечает OfferAccept или OfferDecline.
    Offer {
        id: String,
        files: Vec<FileMeta>,
        total: u64,
    },
    /// Согласие и смещения докачки: сколько байт каждого файла уже есть.
    OfferAccept {
        offsets: Vec<u64>,
    },
    OfferDecline {
        reason: String,
    },
    /// Короткий текст или ссылка: показывается получателю с кнопкой «Копировать».
    Text {
        id: String,
        text: String,
    },
    /// После данных каждого файла — его хеш BLAKE3 целиком.
    FileHash {
        index: u32,
        hash: String,
    },
    /// Итог от получателя: какие файлы не прошли проверку.
    Complete {
        ok: bool,
        failed: Vec<u32>,
    },
}

pub async fn write_msg(send: &mut SendStream, msg: &Msg) -> Result<()> {
    let body = serde_json::to_vec(msg)?;
    if body.len() > MAX_MSG {
        bail!("message too large");
    }
    send.write_all(&(body.len() as u32).to_be_bytes()).await.context("write length")?;
    send.write_all(&body).await.context("write body")?;
    Ok(())
}

pub async fn read_msg(recv: &mut RecvStream) -> Result<Msg> {
    let mut len = [0u8; 4];
    recv.read_exact(&mut len).await.context("read length")?;
    let len = u32::from_be_bytes(len) as usize;
    if len > MAX_MSG {
        bail!("message too large: {len}");
    }
    // Память — по мере прихода данных, а не сразу на заявленную длину:
    // иначе каждый поток мог бы заставить выделить по 8 МБ заранее.
    let mut body = Vec::with_capacity(len.min(CHUNK));
    while body.len() < len {
        let want = (len - body.len()).min(CHUNK);
        match recv.read_chunk(want).await.context("read body")? {
            Some(chunk) => body.extend_from_slice(&chunk),
            None => bail!("stream finished early"),
        }
    }
    serde_json::from_slice(&body).context("parse message")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_round_trip() {
        let msg = Msg::Offer {
            id: "abc".into(),
            files: vec![FileMeta { path: "Фото/1.jpg".into(), size: 42, mtime: 7 }],
            total: 42,
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"t\":\"offer\""));
        match serde_json::from_str::<Msg>(&json).unwrap() {
            Msg::Offer { files, total, .. } => {
                assert_eq!(total, 42);
                assert_eq!(files[0].path, "Фото/1.jpg");
            }
            other => panic!("unexpected {other:?}"),
        }
        // Старое приветствие без адреса тоже читается.
        let hello: Msg =
            serde_json::from_str(r#"{"t":"hello","name":"A","platform":"linux","version":"0"}"#).unwrap();
        assert!(matches!(hello, Msg::Hello { addr: None, .. }));
    }
}
