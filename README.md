# TouchPad Remote — Aplicativo Android (Rust Core + Jetpack Compose)

Aplicativo Android que transforma o celular em um touchpad sem fio de baixa latência para **Windows** e **Linux** (incluindo jogos 3D e troca Multi-PC pela borda da tela).

## Arquitetura (v2.0.0)
- **Motor Rust (`crates/touchpad-android-core`)**: protocolo UDP v1/v2, descoberta LAN, persistência atômica em JSON, heartbeat, e roteador `MultiPcSession` compilados em `libtouchpad_android_core.so` via `cargo-ndk` + ponte JNI protegida por `catch_unwind`.
- **UI Jetpack Compose (`android-app/`)**: casca leve com tela inicial em quadrados, editor visual de **Layout Multi-PC (Bordas)**, configurações persistentes e touchpad tela cheia preta com risco central (`ESQ` / `DIR`).

## Build local
```powershell
cargo ndk -t arm64-v8a -t armeabi-v7a -t x86_64 -o android-app/src/main/jniLibs build --release -p touchpad-android-core
.\gradlew.bat :android-app:assembleDebug --no-daemon
```

## Servidores Desktop
- **Windows**: https://github.com/Henzogabriel954/TouchPadRemote-Windows
- **Linux**: https://github.com/Henzogabriel954/TouchPadRemote-Linux
