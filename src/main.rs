mod app;
mod i18n;
mod icons;
mod motion;
mod settings;
mod tauri;

fn main() {
    console_error_panic_hook::set_once();
    leptos::mount::mount_to_body(app::App);
}
