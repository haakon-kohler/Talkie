//! First run. M1 adds the two things that actually need doing here: asking for
//! the microphone, and downloading the Parakeet model with a progress bar.

use leptos::prelude::*;
use leptos::task::spawn_local;
use talkie_shared::commands;

use crate::ipc;

#[component]
pub fn OnboardingPage() -> impl IntoView {
    let error = RwSignal::new(String::new());

    let get_started = move |_| {
        spawn_local(async move {
            if let Err(e) = ipc::fire(commands::COMPLETE_ONBOARDING).await {
                error.set(e);
            }
        });
    };

    view! {
        <div class="page onboarding-page">
            <h1>"Talkie"</h1>
            <p class="lede">"Speak, and it lands in your notes."</p>
            <p>
                "Press the shortcut, say the thing, press it again. What you said is appended to one
                long markdown file — timestamped, silent, offline. No window steals focus, nothing
                is pasted into whatever app you were using."
            </p>
            <p class="muted">
                "Microphone access and the speech model come next; for now this just gets you to the
                editor."
            </p>
            <div class="actions">
                <span class="status error">{move || error.get()}</span>
                <button class="primary" on:click=get_started>"Get Started"</button>
            </div>
        </div>
    }
}
