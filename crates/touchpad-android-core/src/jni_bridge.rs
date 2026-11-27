#![allow(non_snake_case)]

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use jni::objects::{JClass, JString};
use jni::sys::{jboolean, jfloat, jint, jstring, JNI_FALSE, JNI_TRUE};
use jni::JNIEnv;
use tracing::{error, warn};

use crate::engine::AndroidEngine;
use crate::models::mouse_button_from_ordinal;

static ENGINE: RwLock<Option<Arc<AndroidEngine>>> = RwLock::new(None);

fn with_catch_unwind<T, F>(fallback: T, f: F) -> T
where
    F: FnOnce() -> T,
{
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(value) => value,
        Err(_) => {
            error!("Panic intercepted at JNI boundary in RustTouchpadBridge");
            fallback
        }
    }
}

fn get_engine() -> Option<Arc<AndroidEngine>> {
    ENGINE
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .as_ref()
        .map(Arc::clone)
}

fn read_jstring(env: &mut JNIEnv<'_>, value: &JString<'_>) -> String {
    if value.is_null() {
        return String::new();
    }
    match env.get_string(value) {
        Ok(java_str) => java_str.into(),
        Err(err) => {
            warn!(error = %err, "Failed to read JNI string");
            String::new()
        }
    }
}

fn write_jstring(env: &mut JNIEnv<'_>, value: &str) -> jstring {
    match env.new_string(value) {
        Ok(jstr) => jstr.into_raw(),
        Err(err) => {
            error!(error = %err, "Failed to allocate JNI string");
            std::ptr::null_mut()
        }
    }
}

#[no_mangle]
pub extern "system" fn Java_com_touchpad_android_nativecore_RustTouchpadBridge_nativeInit<
    'local,
>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    files_dir: JString<'local>,
    device_name: JString<'local>,
) -> jboolean {
    with_catch_unwind(JNI_FALSE, || {
        let dir_str = read_jstring(&mut env, &files_dir);
        if dir_str.trim().is_empty() {
            warn!("nativeInit called with empty filesDir");
            return JNI_FALSE;
        }
        let dev_name = read_jstring(&mut env, &device_name);

        let previous = {
            let mut guard = ENGINE
                .write()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            guard.take()
        };
        if let Some(prev) = previous {
            prev.shutdown();
        }

        match AndroidEngine::new(PathBuf::from(dir_str), dev_name) {
            Ok(engine) => {
                let mut guard = ENGINE
                    .write()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                *guard = Some(Arc::new(engine));
                JNI_TRUE
            }
            Err(err) => {
                error!(error = %err, "Failed to initialize AndroidEngine");
                JNI_FALSE
            }
        }
    })
}

#[no_mangle]
pub extern "system" fn Java_com_touchpad_android_nativecore_RustTouchpadBridge_nativeShutdown<
    'local,
>(
    _env: JNIEnv<'local>,
    _class: JClass<'local>,
) {
    with_catch_unwind((), || {
        let existing = {
            let mut guard = ENGINE
                .write()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            guard.take()
        };
        if let Some(engine) = existing {
            engine.shutdown();
        }
    });
}

#[no_mangle]
pub extern "system" fn Java_com_touchpad_android_nativecore_RustTouchpadBridge_nativeGetStateJson<
    'local,
>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
) -> jstring {
    with_catch_unwind(std::ptr::null_mut(), || {
        let json = match get_engine() {
            Some(engine) => engine.get_state_json(),
            None => "{}".to_string(),
        };
        write_jstring(&mut env, &json)
    })
}

#[no_mangle]
pub extern "system" fn Java_com_touchpad_android_nativecore_RustTouchpadBridge_nativeWaitEventsJson<
    'local,
>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    timeout_ms: jint,
) -> jstring {
    with_catch_unwind(std::ptr::null_mut(), || {
        let json = match get_engine() {
            Some(engine) => engine.wait_events_json(timeout_ms.max(0) as u64),
            None => "[]".to_string(),
        };
        write_jstring(&mut env, &json)
    })
}

#[no_mangle]
pub extern "system" fn Java_com_touchpad_android_nativecore_RustTouchpadBridge_nativeAddOrUpdateComputerJson<
    'local,
>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    json: JString<'local>,
) -> jstring {
    with_catch_unwind(std::ptr::null_mut(), || {
        let payload = read_jstring(&mut env, &json);
        let result_json = match get_engine() {
            Some(engine) => match engine.add_or_update_computer_json(&payload) {
                Ok(snapshot) => {
                    serde_json::to_string(&snapshot).unwrap_or_else(|_| engine.get_state_json())
                }
                Err(err) => {
                    warn!(error = %err, "add_or_update_computer_json failed");
                    engine.push_toast(err.to_string());
                    engine.get_state_json()
                }
            },
            None => "{}".to_string(),
        };
        write_jstring(&mut env, &result_json)
    })
}

#[no_mangle]
pub extern "system" fn Java_com_touchpad_android_nativecore_RustTouchpadBridge_nativeRemoveComputer<
    'local,
>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    computer_id: JString<'local>,
) -> jstring {
    with_catch_unwind(std::ptr::null_mut(), || {
        let id = read_jstring(&mut env, &computer_id);
        let result_json = match get_engine() {
            Some(engine) => match engine.remove_computer(&id) {
                Ok(snapshot) => {
                    serde_json::to_string(&snapshot).unwrap_or_else(|_| engine.get_state_json())
                }
                Err(err) => {
                    warn!(error = %err, "remove_computer failed");
                    engine.push_toast(err.to_string());
                    engine.get_state_json()
                }
            },
            None => "{}".to_string(),
        };
        write_jstring(&mut env, &result_json)
    })
}

#[no_mangle]
pub extern "system" fn Java_com_touchpad_android_nativecore_RustTouchpadBridge_nativeSaveEdgeLinksJson<
    'local,
>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    json: JString<'local>,
) -> jstring {
    with_catch_unwind(std::ptr::null_mut(), || {
        let payload = read_jstring(&mut env, &json);
        let result_json = match get_engine() {
            Some(engine) => match engine.save_edge_links_json(&payload) {
                Ok(snapshot) => {
                    serde_json::to_string(&snapshot).unwrap_or_else(|_| engine.get_state_json())
                }
                Err(err) => {
                    warn!(error = %err, "save_edge_links_json failed");
                    engine.push_toast(err.to_string());
                    engine.get_state_json()
                }
            },
            None => "{}".to_string(),
        };
        write_jstring(&mut env, &result_json)
    })
}

#[no_mangle]
pub extern "system" fn Java_com_touchpad_android_nativecore_RustTouchpadBridge_nativeSaveSettingsJson<
    'local,
>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    json: JString<'local>,
) -> jstring {
    with_catch_unwind(std::ptr::null_mut(), || {
        let payload = read_jstring(&mut env, &json);
        let result_json = match get_engine() {
            Some(engine) => match engine.save_settings_json(&payload) {
                Ok(snapshot) => {
                    serde_json::to_string(&snapshot).unwrap_or_else(|_| engine.get_state_json())
                }
                Err(err) => {
                    warn!(error = %err, "save_settings_json failed");
                    engine.push_toast(err.to_string());
                    engine.get_state_json()
                }
            },
            None => "{}".to_string(),
        };
        write_jstring(&mut env, &result_json)
    })
}

#[no_mangle]
pub extern "system" fn Java_com_touchpad_android_nativecore_RustTouchpadBridge_nativeRefreshStatus<
    'local,
>(
    _env: JNIEnv<'local>,
    _class: JClass<'local>,
) {
    with_catch_unwind((), || {
        if let Some(engine) = get_engine() {
            engine.refresh_status();
        }
    });
}

#[no_mangle]
pub extern "system" fn Java_com_touchpad_android_nativecore_RustTouchpadBridge_nativeDiscoverComputers<
    'local,
>(
    _env: JNIEnv<'local>,
    _class: JClass<'local>,
    timeout_ms: jint,
) {
    with_catch_unwind((), || {
        if let Some(engine) = get_engine() {
            engine.discover_computers(timeout_ms.max(0) as u64);
        }
    });
}

#[no_mangle]
pub extern "system" fn Java_com_touchpad_android_nativecore_RustTouchpadBridge_nativeClearDiscovery<
    'local,
>(
    _env: JNIEnv<'local>,
    _class: JClass<'local>,
) {
    with_catch_unwind((), || {
        if let Some(engine) = get_engine() {
            engine.clear_discovery();
        }
    });
}

#[no_mangle]
pub extern "system" fn Java_com_touchpad_android_nativecore_RustTouchpadBridge_nativeStartSession<
    'local,
>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    computer_id: JString<'local>,
    multi_pc_enabled: jboolean,
) {
    with_catch_unwind((), || {
        let id = read_jstring(&mut env, &computer_id);
        if let Some(engine) = get_engine() {
            engine.start_session(&id, multi_pc_enabled != 0);
        }
    });
}

#[no_mangle]
pub extern "system" fn Java_com_touchpad_android_nativecore_RustTouchpadBridge_nativeCancelPairing<
    'local,
>(
    _env: JNIEnv<'local>,
    _class: JClass<'local>,
) {
    with_catch_unwind((), || {
        if let Some(engine) = get_engine() {
            engine.cancel_pairing();
        }
    });
}

#[no_mangle]
pub extern "system" fn Java_com_touchpad_android_nativecore_RustTouchpadBridge_nativeExitSession<
    'local,
>(
    _env: JNIEnv<'local>,
    _class: JClass<'local>,
) {
    with_catch_unwind((), || {
        if let Some(engine) = get_engine() {
            engine.exit_session();
        }
    });
}

#[no_mangle]
pub extern "system" fn Java_com_touchpad_android_nativecore_RustTouchpadBridge_nativeSwitchActiveComputer<
    'local,
>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    target_computer_id: JString<'local>,
) -> jboolean {
    with_catch_unwind(JNI_FALSE, || {
        let target_id = read_jstring(&mut env, &target_computer_id);
        match get_engine() {
            Some(engine) if engine.switch_active_computer(&target_id) => JNI_TRUE,
            _ => JNI_FALSE,
        }
    })
}

#[no_mangle]
pub extern "system" fn Java_com_touchpad_android_nativecore_RustTouchpadBridge_nativeSetEdgeLock<
    'local,
>(
    _env: JNIEnv<'local>,
    _class: JClass<'local>,
    locked: jboolean,
) {
    with_catch_unwind((), || {
        if let Some(engine) = get_engine() {
            engine.set_edge_lock(locked != 0);
        }
    });
}

#[no_mangle]
pub extern "system" fn Java_com_touchpad_android_nativecore_RustTouchpadBridge_nativeSendMove<
    'local,
>(
    _env: JNIEnv<'local>,
    _class: JClass<'local>,
    dx: jfloat,
    dy: jfloat,
) {
    with_catch_unwind((), || {
        if let Some(engine) = get_engine() {
            engine.send_move(dx, dy);
        }
    });
}

#[no_mangle]
pub extern "system" fn Java_com_touchpad_android_nativecore_RustTouchpadBridge_nativeSendClick<
    'local,
>(
    _env: JNIEnv<'local>,
    _class: JClass<'local>,
    button_ordinal: jint,
) {
    with_catch_unwind((), || {
        let Some(button) = mouse_button_from_ordinal(button_ordinal) else {
            return;
        };
        if let Some(engine) = get_engine() {
            engine.send_click(button);
        }
    });
}

#[no_mangle]
pub extern "system" fn Java_com_touchpad_android_nativecore_RustTouchpadBridge_nativeSendButtonState<
    'local,
>(
    _env: JNIEnv<'local>,
    _class: JClass<'local>,
    button_ordinal: jint,
    pressed: jboolean,
) {
    with_catch_unwind((), || {
        let Some(button) = mouse_button_from_ordinal(button_ordinal) else {
            return;
        };
        if let Some(engine) = get_engine() {
            engine.send_button_state(button, pressed != 0);
        }
    });
}

#[no_mangle]
pub extern "system" fn Java_com_touchpad_android_nativecore_RustTouchpadBridge_nativeSendScroll<
    'local,
>(
    _env: JNIEnv<'local>,
    _class: JClass<'local>,
    delta_y: jfloat,
) {
    with_catch_unwind((), || {
        if let Some(engine) = get_engine() {
            engine.send_scroll(delta_y);
        }
    });
}
