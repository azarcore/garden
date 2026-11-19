#![forbid(unsafe_code)]

mod app;
mod canvas;
mod icons;
mod inspector;
mod plan_art;
mod plant_picker;
mod toolbar;

fn main() {
    console_error_panic_hook::set_once();
    leptos::mount::mount_to_body(app::App);
}
