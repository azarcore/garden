#![forbid(unsafe_code)]

mod app;
mod canvas;
mod files;
mod icons;
mod inspector;
mod library;
mod plan_art;
mod plant_picker;
mod print;
mod storage;
mod toolbar;
mod trace;

fn main() {
    console_error_panic_hook::set_once();
    leptos::mount::mount_to_body(app::App);
}
