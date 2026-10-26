//! Очередь событий для интерфейса и короткий журнал.
//!
//! Интерфейс опрашивает ядро (`osh_poll`) несколько раз в секунду и получает
//! номер версии состояния и разовые события. Номер изменился — интерфейс
//! заново берёт снимок состояния. Разовые события — то, что нужно показать
//! один раз: «файлы получены», «устройство отключилось» и т. п.

use std::collections::VecDeque;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::{Value, json};

const MAX_EVENTS: usize = 256;
const MAX_LOG: usize = 400;

#[derive(Default)]
pub struct Events {
    version: AtomicU64,
    queue: Mutex<VecDeque<Value>>,
    log: Mutex<VecDeque<String>>,
}

impl Events {
    /// Состояние изменилось — интерфейсу пора обновить снимок.
    pub fn changed(&self) {
        self.version.fetch_add(1, Ordering::SeqCst);
    }

    pub fn version(&self) -> u64 {
        self.version.load(Ordering::SeqCst)
    }

    pub fn emit(&self, event: Value) {
        if let Ok(mut q) = self.queue.lock() {
            if q.len() >= MAX_EVENTS {
                q.pop_front();
            }
            q.push_back(event);
        }
        self.changed();
    }

    pub fn log(&self, line: impl Into<String>) {
        let line = format!("{} {}", crate::util::now_unix(), line.into());
        if let Ok(mut l) = self.log.lock() {
            if l.len() >= MAX_LOG {
                l.pop_front();
            }
            l.push_back(line);
        }
    }

    pub fn poll(&self) -> Value {
        let events: Vec<Value> = match self.queue.lock() {
            Ok(mut q) => q.drain(..).collect(),
            Err(_) => Vec::new(),
        };
        json!({ "v": self.version(), "events": events })
    }

    pub fn log_lines(&self) -> Vec<String> {
        self.log.lock().map(|l| l.iter().cloned().collect()).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn events_are_drained_and_versioned() {
        let e = Events::default();
        let v0 = e.version();
        e.emit(json!({"type": "x"}));
        e.emit(json!({"type": "y"}));
        let p = e.poll();
        assert_eq!(p["events"].as_array().unwrap().len(), 2);
        assert!(p["v"].as_u64().unwrap() > v0);
        assert!(e.poll()["events"].as_array().unwrap().is_empty());
        for i in 0..1000 {
            e.emit(json!({ "i": i }));
        }
        assert_eq!(e.poll()["events"].as_array().unwrap().len(), MAX_EVENTS);
    }
}
