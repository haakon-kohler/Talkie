//! `Talkie transcribe` and `Talkie capture`: the capture pipeline from a
//! terminal, with no app.
//!
//! ```text
//! Talkie transcribe <file.wav>
//! Talkie capture --wav <file.wav> [--note <path>]
//! Talkie capture --text <text> [--note <path>]
//! ```
//!
//! Both are answered in `run` before the Tauri builder, the way `--debug-log`
//! is, so there is no window, no tray and no single-instance socket: they work
//! beside a running Talkie. There is also no event tap. Nothing here binds the
//! shortcut or touches the Accessibility grant, and nothing opens the
//! microphone; the audio comes from a file.
//!
//! `transcribe` runs the file through what a capture runs it through — the
//! resampler, the voice detector, the ten-minute cap, Parakeet — and prints the
//! transcript. Nothing is written. `capture` carries on from there exactly as
//! the shortcut would: the `on-capture` hook, the write under the document
//! contract, and the `after-capture` hook, which it waits for. `--text` skips
//! the audio and starts at the hook, so a hook or something watching the file
//! can be tried out without speaking. The notes file is the one in settings
//! unless `--note` names another.
//!
//! The output is for scripts. Stdout carries the transcript, or what was
//! written, and nothing else; everything else goes to stderr. The exit code is
//! 0 when the command did its job — a capture the hook dropped and a file with
//! no speech in it included — 1 when it failed, and 2 when it was called wrong.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{anyhow, bail, Context, Result};
use talkie_shared::ModelStatus;
use tauri::PackageInfo;

use crate::recorder::{self, Landed, MIN_CAPTURE_SAMPLES};
use crate::transcriber::Transcriber;
use crate::{audio_toolkit, models, note, settings};

const USAGE: &str = "\
usage: Talkie transcribe <file.wav>
       Talkie capture --wav <file.wav> [--note <path>]
       Talkie capture --text <text> [--note <path>]";

/// What the command line asked for.
#[derive(Debug, PartialEq, Eq)]
enum Command {
    /// Print what is said in a WAV file.
    Transcribe { wav: PathBuf },
    /// Put a capture into the notes file, or into `note` when given.
    Capture {
        source: Source,
        note: Option<PathBuf>,
    },
}

/// Where a command-line capture's words come from.
#[derive(Debug, PartialEq, Eq)]
enum Source {
    /// Speech in a WAV file, heard the way the microphone's would be.
    Wav(PathBuf),
    /// Text that stands in for a transcript.
    Text(String),
}

/// Answer `Talkie transcribe` or `Talkie capture`, or return `None` when the
/// arguments ask for neither and the app should start.
///
/// `data_dir` is the app-data directory, where the model, the hooks and the
/// settings live.
pub fn handle(package: &PackageInfo, data_dir: &Path) -> Option<ExitCode> {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    let command = match parse(&args)? {
        Ok(command) => command,
        Err(message) => {
            eprintln!("talkie: {message}\n{USAGE}");
            return Some(ExitCode::from(2));
        }
    };

    // Warnings from the pipeline — a hook that failed, a capture that hit the
    // cap — belong on this terminal, not in the app's debug log.
    if let Err(e) =
        env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn"))
            .try_init()
    {
        eprintln!("talkie: warnings will not be shown: {e}");
    }

    match execute(command, package, data_dir) {
        Ok(()) => Some(ExitCode::SUCCESS),
        Err(e) => {
            eprintln!("talkie: {e:#}");
            Some(ExitCode::FAILURE)
        }
    }
}

/// `None` when `args` (without the executable) are not a command-line
/// command; otherwise the command, or why it is malformed.
fn parse(args: &[OsString]) -> Option<Result<Command, String>> {
    let (verb, rest) = args.split_first()?;
    if verb == "transcribe" {
        Some(parse_transcribe(rest))
    } else if verb == "capture" {
        Some(parse_capture(rest))
    } else {
        None
    }
}

fn parse_transcribe(rest: &[OsString]) -> Result<Command, String> {
    match rest {
        [wav] if !wav.to_string_lossy().starts_with("--") => Ok(Command::Transcribe {
            wav: PathBuf::from(wav),
        }),
        _ => Err("transcribe takes one WAV file and nothing else".to_owned()),
    }
}

fn parse_capture(rest: &[OsString]) -> Result<Command, String> {
    let mut source = None;
    let mut note = None;
    let mut args = rest.iter();
    while let Some(flag) = args.next() {
        let flag_name = flag.to_string_lossy();
        let Some(value) = args.next() else {
            return Err(format!("`{flag_name}` needs a value"));
        };
        if flag == "--wav" || flag == "--text" {
            if source.is_some() {
                return Err("capture takes one source: --wav or --text, once".to_owned());
            }
            source = Some(if flag == "--wav" {
                Source::Wav(PathBuf::from(value))
            } else {
                let text = value
                    .clone()
                    .into_string()
                    .map_err(|text| format!("the text `{}` is not UTF-8", text.display()))?;
                Source::Text(text)
            });
        } else if flag == "--note" {
            if note.is_some() {
                return Err("capture takes --note once".to_owned());
            }
            note = Some(PathBuf::from(value));
        } else {
            return Err(format!("`{flag_name}` is not an option of capture"));
        }
    }
    let source = source.ok_or_else(|| "capture needs --wav <file> or --text <text>".to_owned())?;
    Ok(Command::Capture { source, note })
}

fn execute(command: Command, package: &PackageInfo, data_dir: &Path) -> Result<()> {
    match command {
        Command::Transcribe { wav } => match hear(&wav, package, data_dir)? {
            Some(text) => println!("{text}"),
            None => eprintln!("talkie: no speech in {}", wav.display()),
        },
        Command::Capture { source, note } => {
            let path = match note {
                // Relative to where the command was run, as a shell user
                // expects; the file itself has to be named by a full path.
                Some(path) => std::path::absolute(&path)
                    .with_context(|| format!("could not resolve {}", path.display()))?,
                None => note::resolve(&settings::stored_note_path(data_dir)?),
            };
            let text = match source {
                Source::Text(text) => text,
                Source::Wav(wav) => {
                    let Some(text) = hear(&wav, package, data_dir)? else {
                        eprintln!("talkie: no speech in {}; nothing written", wav.display());
                        return Ok(());
                    };
                    text
                }
            };
            match recorder::land(data_dir, &path, &text)? {
                Landed::Written { text, after } => {
                    println!("{text}");
                    // A script that runs `capture` should be able to rely on
                    // everything a capture sets off being finished when it
                    // returns, so wait for the hook the app would leave be.
                    if let Some(after) = after {
                        after.join().map_err(|_panic| {
                            anyhow!("the after-capture hook's runner panicked")
                        })?;
                    }
                }
                Landed::Dropped => {
                    eprintln!("talkie: the on-capture hook dropped the capture; nothing written");
                }
                Landed::Blank => eprintln!("talkie: the text is blank; nothing written"),
            }
        }
    }
    Ok(())
}

/// Run a WAV file through a capture's front end and the model.
///
/// `None` when there is no speech in it worth a model run: what the app drops
/// as a slip of the finger, or a transcript that comes back empty.
fn hear(wav: &Path, package: &PackageInfo, data_dir: &Path) -> Result<Option<String>> {
    // Checked before decoding: without the model, detecting voice in a long
    // file is work for nothing.
    if models::status(data_dir) != ModelStatus::Ready {
        bail!(
            "the speech model is not installed in {} — open Talkie once to download it",
            data_dir.display()
        );
    }
    let resource_dir = tauri::utils::platform::resource_dir(package, &tauri::Env::default())
        .context("could not find the bundled resources")?;
    let detector = recorder::voice_detector(&models::vad_path(&resource_dir))?;

    let (samples, sample_rate) = read_wav(wav)?;
    let speech = audio_toolkit::process(&samples, sample_rate, Some(detector));
    if speech.len() < MIN_CAPTURE_SAMPLES {
        return Ok(None);
    }

    let text = Transcriber::new().transcribe(&models::model_path(data_dir)?, &speech)?;
    Ok((!text.is_empty()).then_some(text))
}

/// Decode a WAV file to mono `f32` at its own sample rate.
fn read_wav(path: &Path) -> Result<(Vec<f32>, u32)> {
    let mut reader = hound::WavReader::open(path)
        .with_context(|| format!("could not read {} as WAV", path.display()))?;
    let spec = reader.spec();
    if spec.sample_rate == 0 || spec.channels == 0 {
        bail!(
            "{} declares {} Hz and {} channels; there is no audio in that",
            path.display(),
            spec.sample_rate,
            spec.channels
        );
    }

    let interleaved: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => reader.samples::<f32>().collect::<Result<_, _>>(),
        hound::SampleFormat::Int => {
            let full_scale = 2f32.powi(i32::from(spec.bits_per_sample) - 1);
            #[expect(
                clippy::cast_precision_loss,
                reason = "a sample of at most 32 bits loses only bits far below what the model hears"
            )]
            let to_float = move |sample: i32| sample as f32 / full_scale;
            reader
                .samples::<i32>()
                .map(|sample| sample.map(to_float))
                .collect::<Result<_, _>>()
        }
    }
    .with_context(|| format!("{} is cut short or damaged", path.display()))?;

    let mono = interleaved
        .chunks_exact(usize::from(spec.channels))
        .map(|frame| frame.iter().sum::<f32>() / f32::from(spec.channels))
        .collect();
    Ok((mono, spec.sample_rate))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(v: &[&str]) -> Vec<OsString> {
        v.iter().map(OsString::from).collect()
    }

    fn package() -> PackageInfo {
        let context: tauri::Context<tauri::Wry> = tauri::generate_context!();
        context.package_info().clone()
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("talkie-cli-{name}-{}", std::process::id()));
        crate::test_support::remove_dir_if_present(&dir);
        std::fs::create_dir_all(&dir).expect("scratch dir");
        dir
    }

    #[test]
    fn anything_else_starts_the_app() {
        assert_eq!(parse(&args(&[])), None);
        assert_eq!(parse(&args(&["--debug-log"])), None);
        assert_eq!(parse(&args(&["-psn_0_12345"])), None);
    }

    #[test]
    fn transcribe_takes_one_file() {
        assert_eq!(
            parse(&args(&["transcribe", "a.wav"])),
            Some(Ok(Command::Transcribe {
                wav: PathBuf::from("a.wav")
            }))
        );
        assert!(matches!(parse(&args(&["transcribe"])), Some(Err(_))));
        assert!(matches!(
            parse(&args(&["transcribe", "a.wav", "b.wav"])),
            Some(Err(_))
        ));
        assert!(matches!(
            parse(&args(&["transcribe", "--wav"])),
            Some(Err(_))
        ));
    }

    #[test]
    fn capture_takes_one_source_and_an_optional_note() {
        assert_eq!(
            parse(&args(&["capture", "--text", "hello", "--note", "n.md"])),
            Some(Ok(Command::Capture {
                source: Source::Text("hello".to_owned()),
                note: Some(PathBuf::from("n.md")),
            }))
        );
        assert_eq!(
            parse(&args(&["capture", "--note", "n.md", "--wav", "a.wav"])),
            Some(Ok(Command::Capture {
                source: Source::Wav(PathBuf::from("a.wav")),
                note: Some(PathBuf::from("n.md")),
            }))
        );
        assert_eq!(
            parse(&args(&["capture", "--wav", "a.wav"])),
            Some(Ok(Command::Capture {
                source: Source::Wav(PathBuf::from("a.wav")),
                note: None,
            }))
        );
    }

    #[test]
    fn a_malformed_capture_is_refused() {
        for bad in [
            &["capture"][..],
            &["capture", "--note", "n.md"],
            &["capture", "--text"],
            &["capture", "--text", "a", "--wav", "b.wav"],
            &["capture", "--text", "a", "--text", "b"],
            &["capture", "--text", "a", "--note", "x", "--note", "y"],
            &["capture", "--text", "a", "--loud", "yes"],
        ] {
            assert!(matches!(parse(&args(bad)), Some(Err(_))), "{bad:?}");
        }
    }

    #[test]
    fn a_stereo_wav_is_read_as_mono_at_its_own_rate() {
        let dir = scratch("wav");
        let path = dir.join("stereo.wav");
        let spec = hound::WavSpec {
            channels: 2,
            sample_rate: 44_100,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer = hound::WavWriter::create(&path, spec).expect("create wav");
        for (left, right) in [(16_384_i16, 0_i16), (-32_768, -32_768), (0, 0)] {
            writer.write_sample(left).expect("left");
            writer.write_sample(right).expect("right");
        }
        writer.finalize().expect("finalize");

        let (mono, rate) = read_wav(&path).expect("read wav");
        assert_eq!(rate, 44_100);
        assert_eq!(mono, vec![0.25, -1.0, 0.0]);
        crate::test_support::remove_dir_if_present(&dir);
    }

    #[test]
    fn a_file_that_is_not_wav_is_an_error() {
        let dir = scratch("not-wav");
        let path = dir.join("note.wav");
        std::fs::write(&path, "not audio").expect("seed");
        assert!(read_wav(&path).is_err());
        crate::test_support::remove_dir_if_present(&dir);
    }

    #[test]
    fn a_text_capture_lands_under_the_document_contract() {
        let dir = scratch("text");
        let note = dir.join("talkie.md");
        std::fs::write(&note, "# Notes\n").expect("seed");
        let package = package();

        execute(
            Command::Capture {
                source: Source::Text("hello from a script".to_owned()),
                note: Some(note.clone()),
            },
            &package,
            &dir,
        )
        .expect("capture");

        let written = std::fs::read_to_string(&note).expect("read note");
        let mut lines = written.lines();
        assert_eq!(lines.next(), Some("# Notes"), "{written}");
        assert_eq!(lines.next(), Some(""), "{written}");
        assert!(
            lines.next().is_some_and(|l| l.starts_with("## ")),
            "{written}"
        );
        assert_eq!(lines.next(), Some("hello from a script"), "{written}");
        assert_eq!(lines.next(), None, "{written}");
        crate::test_support::remove_dir_if_present(&dir);
    }

    #[test]
    fn hearing_without_the_model_says_so() {
        let dir = scratch("no-model");
        let package = package();
        let error = execute(
            Command::Transcribe {
                wav: dir.join("any.wav"),
            },
            &package,
            &dir,
        )
        .expect_err("no model");
        assert!(error.to_string().contains("not installed"), "{error}");
        crate::test_support::remove_dir_if_present(&dir);
    }
}
