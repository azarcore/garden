#![forbid(unsafe_code)]

mod app;
mod calendar;
mod canvas;
mod care;
mod catalog;
mod files;
mod garden;
mod icons;
mod inspector;
mod layers;
mod library;
mod list;
mod photos;
mod plan_art;
mod print;
mod pwa;
mod reminders;
mod storage;
mod templates;
mod toolbar;
mod trace;
mod view3d;

fn main() {
    console_error_panic_hook::set_once();
    leptos::mount::mount_to_body(app::App);
}
