//! Tauri 2 desktop shell around the OpenMesh web client.
//!
//! The frontend (Vite build in `../dist`) talks to the local `openmesh-bridge`
//! over loopback WebSocket/REST exactly like the browser build; the shell only
//! provides a native window and app identity.

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .run(tauri::generate_context!())
        .expect("error while running the OpenMesh desktop client");
}
