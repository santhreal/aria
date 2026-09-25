# aria-android

The Android harness app (Kotlin + Jetpack Compose). It loads the `aria-mobile`
`cdylib` (`.so`) at runtime and calls the JNI bridge (`AriaNative`), which
mirrors the full C ABI the iOS shell uses: editing sessions (JSON commands /
intents / ui_state, undo, save), preview renders, export jobs, thumbnailers, and
realtime audio.

`MainActivity` runs the engine smoke test on launch: open a session, add a solid
clip through wire commands, read `ui_state`, undo/redo, and render frame 0 on
the device GPU. The screen shows the transcript and the frame; failures name the
layer that broke (`adb logcat -s aria` has the native detail).

Build the native library into `jniLibs`, then build the app:

```bash
cargo ndk -t arm64-v8a -o apps/aria-android/app/src/main/jniLibs \
    build -p aria-mobile --release
```

Open this folder in Android Studio, or `./gradlew assembleDebug`.
