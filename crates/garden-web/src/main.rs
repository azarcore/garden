#![forbid(unsafe_code)]

mod app;
mod canvas;
mod catalog;
mod files;
mod garden;
mod icons;
mod inspector;
mod layers;
mod library;
mod list;
mod plan_art;
mod print;
mod storage;
mod templates;
mod toolbar;
mod trace;
mod view3d;

fn main() {
    console_error_panic_hook::set_once();
    leptos::mount::mount_to_body(app::App);
}
