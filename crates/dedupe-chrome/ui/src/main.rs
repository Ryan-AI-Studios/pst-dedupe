mod app;
mod invoke;
mod pages;
mod path_id;

use app::App;

fn main() {
    console_error_panic_hook::set_once();
    let _ = console_log::init_with_level(log::Level::Warn);
    leptos::mount::mount_to_body(App);
}
