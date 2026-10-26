//! Ядро Ogneliss Share: прямые зашифрованные передачи файлов между
//! устройствами без учётных записей.
//!
//! Интерфейс (Flutter) общается с ядром через три функции C:
//!
//! - `osh_call(json) -> json` — команда и её результат (см. `api.rs`);
//! - `osh_poll() -> json` — номер версии состояния и накопившиеся события;
//! - `osh_free(ptr)` — освободить строку, которую вернуло ядро.

pub mod api;
pub mod events;
pub mod node;
pub mod pairing;
pub mod proto;
pub mod store;
pub mod transfer;
pub mod util;

#[cfg(test)]
mod tests;

use std::ffi::{CStr, CString, c_char};
use std::panic::{AssertUnwindSafe, catch_unwind};

fn into_c(s: String) -> *mut c_char {
    CString::new(s.replace('\0', "")).map(CString::into_raw).unwrap_or(std::ptr::null_mut())
}

/// Выполнить команду.
///
/// # Safety
///
/// `request` — указатель на строку UTF-8 с завершающим нулём (или null).
/// Результат нужно освободить через [`osh_free`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn osh_call(request: *const c_char) -> *mut c_char {
    let out = catch_unwind(AssertUnwindSafe(|| {
        if request.is_null() {
            return r#"{"ok":false,"error":"bad_request"}"#.to_string();
        }
        // SAFETY: вызывающая сторона передаёт строку с завершающим нулём.
        let req = unsafe { CStr::from_ptr(request) }.to_string_lossy().into_owned();
        api::call(&req)
    }))
    .unwrap_or_else(|_| r#"{"ok":false,"error":"panic"}"#.to_string());
    into_c(out)
}

/// Забрать события. Результат нужно освободить через [`osh_free`].
#[unsafe(no_mangle)]
pub extern "C" fn osh_poll() -> *mut c_char {
    let out = catch_unwind(api::poll).unwrap_or_else(|_| r#"{"v":0,"events":[]}"#.to_string());
    into_c(out)
}

/// Освободить строку, которую вернули `osh_call` или `osh_poll`.
///
/// # Safety
///
/// `s` — указатель, полученный от `osh_call`/`osh_poll`, либо null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn osh_free(s: *mut c_char) {
    if !s.is_null() {
        // SAFETY: строка создана через CString::into_raw в этой библиотеке.
        drop(unsafe { CString::from_raw(s) });
    }
}

/// Android: передать ядру JavaVM и контекст приложения, чтобы узнавать DNS
/// текущей сети (на Android нет /etc/resolv.conf). Вызывается из
/// `MainActivity.onCreate` до запуска интерфейса.
#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_ogneliss_share_MainActivity_nativeInit(
    env: *mut jni_sys::JNIEnv,
    _this: jni_sys::jobject,
    context: jni_sys::jobject,
) {
    static ONCE: std::sync::Once = std::sync::Once::new();
    let _ = catch_unwind(AssertUnwindSafe(|| {
        ONCE.call_once(|| {
            if env.is_null() || context.is_null() {
                return;
            }
            // SAFETY: env — действительный JNIEnv текущего потока (его передаёт JVM);
            // глобальная ссылка на контекст живёт до конца процесса.
            unsafe {
                let fns = *env;
                let (Some(get_vm), Some(new_global)) = ((*fns).GetJavaVM, (*fns).NewGlobalRef) else {
                    return;
                };
                let mut vm: *mut jni_sys::JavaVM = std::ptr::null_mut();
                if get_vm(env, &mut vm) != 0 || vm.is_null() {
                    return;
                }
                let ctx = new_global(env, context);
                if ctx.is_null() {
                    return;
                }
                iroh::dns::install_android_jni_context(vm.cast(), ctx.cast());
            }
        });
    }));
}
