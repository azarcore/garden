#![forbid(unsafe_code)]

mod app;
mod canvas;
mod inspector;
mod plant_picker;
mod status;
mod toolbar;

fn main() {
    console_error_panic_hook::set_once();
    leptos::mount::mount_to_body(app::App);
}
