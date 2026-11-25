// Windows release builds should not open a console window.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    openmesh_desktop_lib::run()
}
