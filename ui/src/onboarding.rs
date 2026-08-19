//! First run: the two things that actually need doing before Talkie can work —
//! granting the microphone, and downloading the speech model.
//!
//! The steps run in order and the page only ever shows the current one, so first
//! run reads as one instruction at a time rather than a checklist. Prose is
//! synced from `COPY.md` at the repo root, which is the source of truth for
//! every user-visible string.

use leptos::prelude::*;
use leptos::task::spawn_local;
use talkie_shared::commands;

use crate::ipc;
use crate::model::ModelSection;

/// Where first run has got to. `Model` covers both "not downloaded" and
/// "downloading", because the page shows the same block either way.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Step {
    Microphone,
    Model,
    Done,
}

#[component]
pub fn OnboardingPage() -> impl IntoView {
    let step = RwSignal::new(Step::Microphone);
    let error = RwSignal::new(String::new());
    let busy = RwSignal::new(false);

    let grant_microphone = move |_| {
        busy.set(true);
        error.set(String::new());
        spawn_local(async move {
            match ipc::fetch::<bool>(commands::REQUEST_MICROPHONE).await {
                Ok(true) => step.set(Step::Model),
                // COPY: onboarding.microphone.denied
                Ok(false) => error.set(
                    "macOS denied the microphone. Open System Settings › Privacy & Security › \
                     Microphone and switch Talkie on."
                        .to_string(),
                ),
                Err(e) => error.set(e),
            }
            busy.set(false);
        });
    };

    // The model may already be installed — a second run of onboarding, or a
    // reinstall over an existing app-data directory. `ModelSection` reports that
    // the moment it reads the status, which skips this step rather than offering
    // a download that would return instantly.
    let model_ready = move || {
        if step.get_untracked() == Step::Model {
            step.set(Step::Done);
        }
    };

    let finish = move |_| {
        spawn_local(async move {
            if let Err(e) = ipc::fire(commands::COMPLETE_ONBOARDING).await {
                error.set(e);
            }
        });
    };

    view! {
        <div class="page onboarding-page">
            <h1>"Talkie"</h1>
            // COPY: onboarding.lede
            <p class="lede">"The modern notepad."</p>
            // COPY: onboarding.body
            <p>
                "Press the shortcut and you can record directly to a markdown notepad. Want to
                enable obsidian or openclaw integration? Point the document at the home folder!"
            </p>

            <Show when=move || step.get() == Step::Microphone>
                // COPY: onboarding.microphone.body — placeholder
                <p class="muted">
                    "Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod tempor
                    incididunt ut labore."
                </p>
                <div class="actions">
                    <span class="status error">{move || error.get()}</span>
                    <button
                        class="primary"
                        prop:disabled=move || busy.get()
                        on:click=grant_microphone
                    >
                        // COPY: onboarding.microphone.cta — placeholder
                        "Allow Microphone"
                    </button>
                </div>
            </Show>

            <Show when=move || step.get() == Step::Model>
                <ModelSection on_ready=model_ready />
            </Show>

            <Show when=move || step.get() == Step::Done>
                // COPY: onboarding.done.body — placeholder
                <p class="muted">
                    "Duis aute irure dolor in reprehenderit in voluptate velit esse cillum dolore."
                </p>
                <div class="actions">
                    <span class="status error">{move || error.get()}</span>
                    // COPY: onboarding.cta
                    <button class="primary" on:click=finish>"Start Writing"</button>
                </div>
            </Show>
        </div>
    }
}
