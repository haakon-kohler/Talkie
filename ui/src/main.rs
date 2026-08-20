//! One WASM bundle, three windows. Each window asks Tauri for its own label and
//! renders the surface that belongs to it.

mod accessibility;
mod cm;
mod editor;
mod ipc;
mod model;
mod onboarding;
mod settings;
mod shortcut;

use leptos::prelude::*;
use talkie_shared::WindowLabel;

fn main() {
    console_error_panic_hook::set_once();

    let label = WindowLabel::parse(&ipc::current_window_label());
    mount_to_body(move || view! { <Root label=label /> })
}

#[component]
fn Root(label: Option<WindowLabel>) -> impl IntoView {
    match label {
        Some(WindowLabel::Editor) => view! { <editor::EditorPage /> }.into_any(),
        Some(WindowLabel::Settings) => view! { <settings::SettingsPage /> }.into_any(),
        Some(WindowLabel::Onboarding) => view! { <onboarding::OnboardingPage /> }.into_any(),
        // Only reachable if a window is created without a matching label.
        None => view! { <p class="unknown-window">"Unknown window."</p> }.into_any(),
    }
}
