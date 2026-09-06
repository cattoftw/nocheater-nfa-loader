// GUI subsystem always — avoids a black console titled nocheater.exe on
// elevated relaunch (release and debug). `npm run tauri dev` still owns its
// own build terminal; that is separate from the app process window.
#![windows_subsystem = "windows"]

fn main() {
    nocheater_lib::run()
}
