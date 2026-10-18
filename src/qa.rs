// SPDX-License-Identifier: MIT

//! `strata --qa <directory>`: the normal window, driven by JSON commands on
//! stdin, with one JSON reply per command on stdout. Built only with the `qa`
//! feature; `tests/qa.rs` is the runner and `docs/e2e-testing.md` the guide.

mod criticals;
mod input;
mod pixels;
mod protocol;
mod state;
mod targets;

use std::{
    cell::RefCell,
    collections::HashMap,
    ffi::OsString,
    fs::File,
    io::{BufRead, Write},
    panic::AssertUnwindSafe,
    path::Path,
    time::{Duration, Instant},
};

use futures_channel::mpsc;
use futures_lite::{FutureExt as _, StreamExt};
use gtk::{gdk, glib, prelude::*};
use serde_json::{Value, json};

use self::{
    input::{Event, Injector},
    pixels::Capture,
    protocol::{
        Accessor, At, Check, Chord, Command, Direction, EntryTarget, Expr, Failure, FieldRef,
        Measure, Modifier, Orientation, PointAxis, Predicate, PressCheck, Query, Repeat, Reply,
        Target,
    },
    state::FieldState,
    targets::{Aim, Boxes, Kept, LookupError, Memory, Rect},
};

/// Present in every `qa` build. The release workflow fails a binary containing its
/// stable prefix `usage: strata --qa`, and the QA job checks a `qa` build does;
/// `scripts/test_qa_parity.py` requires both checks' pattern to prefix this text.
const USAGE: &str = "usage: strata --qa <directory>";
/// `ui::browser::peek::PEEK_LABEL`, whose module is private to the browser; the
/// Python harness names it alike, and a peek test fails if the two drift apart.
pub(crate) const PEEK_LABEL: &str = "Folder peek";
const READY_TIMEOUT: Duration = Duration::from_secs(60);
const POLL_INTERVAL: Duration = Duration::from_millis(50);
const FRAME_TIMEOUT: Duration = Duration::from_millis(250);
// The pointer timings mirror `tests/e2e/harness/interaction.py`, so both
// harnesses feed GTK the same rhythm; each gap also lets a frame settle.
/// Held like a physical click: a layout change caused by the press makes GTK
/// synthesize motion from the live pointer state one frame later, and a
/// release it has not read yet would cancel the click.
const BUTTON_HOLD: Duration = Duration::from_millis(100);
/// With the hold, two clicks stay well inside GTK's 400 ms double-click time,
/// so nothing may settle between them.
const DOUBLE_CLICK_GAP: Duration = Duration::from_millis(90);
/// Comfortably outside the double-click time, so a drag right after a
/// selecting click does not become a double-click activation.
const SEPARATE_CLICK_GAP: Duration = Duration::from_millis(600);
const EVENT_GAP: Duration = Duration::from_millis(20);
const POINTER_GAP: Duration = Duration::from_millis(50);
/// `select_entry` in `tests/e2e/harness/browser.py`: a click can land before
/// GTK maps the row for input, so it clicks once more after this long.
const SELECT_RETRY_AFTER: Duration = Duration::from_secs(4);
/// `tree.DEFAULT_TIMEOUT`: how long the Python lookups wait for a node.
const LOCATE_TIMEOUT: Duration = Duration::from_secs(15);
/// `Strata.settle` samples: the same bounds this many polls in a row.
const SETTLE_SAMPLES: usize = 3;
/// `MENU_RETRY_INTERVAL` in `tests/e2e/harness/browser.py`.
const MENU_RETRY_AFTER: Duration = Duration::from_secs(1);
/// `Strata.KEYBOARD_SELECT_STEPS`: enough to cross any fixture directory.
const KEYBOARD_SELECT_STEPS: usize = 40;
const PANIC_EXIT: i32 = 70;
const ARRIVAL_ATTEMPTS: usize = 5;
/// Approach a point in two steps so GTK sees enter and motion, not a teleport.
const APPROACH_OFFSET: i16 = 6;
/// When `test_drag_completion_keeps_the_source_label_in_place` samples the
/// source label after letting go, and the latest any sample may be taken.
const DROP_SAMPLE_DELAYS: [Duration; 3] = [
    Duration::from_millis(60),
    Duration::from_millis(120),
    Duration::from_millis(180),
];
const DROP_SAMPLE_LIMIT: Duration = Duration::from_millis(240);
/// How far a sampled label may sit from where it rested, in pixels.
const DROP_SAMPLE_DRIFT: f64 = 1.5;
/// How much a sampled label's contrast must recover over the dragging one.
const DROP_SAMPLE_RECOVERY: f64 = 1.25;

pub(crate) struct Session {
    protocol: File,
    application_arguments: Vec<OsString>,
}

impl Session {
    /// Takes over stdout for replies before anything can log there; the
    /// application's own output, tracing included, goes to stderr instead.
    pub(crate) fn prepare(arguments: &[OsString]) -> Result<Self, String> {
        let [program, _, directory] = arguments else {
            return Err(USAGE.to_owned());
        };
        if !Path::new(directory).is_dir() {
            return Err(format!("--qa needs a directory, not {directory:?}"));
        }
        let protocol = rustix::io::fcntl_dupfd_cloexec(std::io::stdout(), 3)
            .map_err(|error| format!("cannot keep stdout for replies: {error}"))?;
        rustix::stdio::dup2_stdout(std::io::stderr())
            .map_err(|error| format!("cannot move application output to stderr: {error}"))?;
        criticals::capture();
        Ok(Self {
            protocol: File::from(protocol),
            application_arguments: vec![program.clone(), directory.clone()],
        })
    }

    /// The command line the application itself sees: an ordinary launch in the directory.
    pub(crate) fn application_arguments(&self) -> &[OsString] {
        &self.application_arguments
    }

    pub(crate) fn attach(self, application: &gtk::Application) {
        let session = RefCell::new(Some(self));
        application.connect_startup(move |application| {
            if let Some(session) = session.take() {
                session.start(application);
            }
        });
    }

    fn start(self, application: &gtk::Application) {
        let (sender, receiver) = mpsc::unbounded();
        std::thread::spawn(move || {
            for line in std::io::stdin().lock().lines() {
                let Ok(line) = line else { break };
                if line.trim().is_empty() {
                    continue;
                }
                if sender.unbounded_send(line).is_err() {
                    break;
                }
            }
        });
        let last_words = self.protocol.try_clone();
        let controller = Controller {
            application: application.clone(),
            protocol: self.protocol,
            injector: None,
            main: glib::WeakRef::new(),
            previous: None,
            settled: RefCell::default(),
            memory: RefCell::default(),
            widgets: RefCell::default(),
            samples: RefCell::default(),
            clipboard: RefCell::default(),
        };
        // GLib would keep a panic in the task's join handle and leave the
        // runner waiting; report it and exit instead.
        glib::MainContext::default().spawn_local(async move {
            let Err(panic) = AssertUnwindSafe(controller.serve(receiver))
                .catch_unwind()
                .await
            else {
                return;
            };
            let message = panic
                .downcast_ref::<&str>()
                .map(|message| (*message).to_owned())
                .or_else(|| panic.downcast_ref::<String>().cloned())
                .unwrap_or_default();
            if let Ok(mut protocol) = last_words {
                let reply = Reply {
                    ok: false,
                    app: Value::Null,
                    changed: Vec::new(),
                    criticals: criticals::take(),
                    result: Value::Null,
                    failures: Vec::new(),
                    error: Some(format!("the QA controller panicked: {message}")),
                };
                if let Ok(line) = serde_json::to_string(&reply) {
                    let _ = writeln!(protocol, "{line}");
                    let _ = protocol.flush();
                }
            }
            std::process::exit(PANIC_EXIT);
        });
    }
}

struct Controller {
    application: gtk::Application,
    protocol: File,
    injector: Option<Injector>,
    /// The browser window the run started with, `strata.window`: a floating
    /// chooser becomes the active window while it is open, and state is
    /// still read from the browser.
    main: glib::WeakRef<gtk::Window>,
    previous: Option<Value>,
    /// Where each target came to rest in its last `settle`, keyed by its JSON,
    /// and the boxes `settle` and `remember` kept under a name.
    settled: RefCell<Boxes>,
    /// The fields `remember` kept, for `{"field":"name"}` and `of`.
    memory: RefCell<Memory>,
    /// The other widgets `remember` kept, apart from the fields so a field
    /// lookup never resolves one, for `replaced`.
    widgets: RefCell<Memory>,
    /// Screen captures, text measurements, and instants, by the name a step
    /// kept them under.
    samples: RefCell<HashMap<String, Sample>>,
    /// The second display connection a `clipboard_set` opened and owns the
    /// clipboard through, until the `clipboard` read that closes it.
    clipboard: RefCell<Option<gdk::Display>>,
}

#[derive(Clone)]
enum Sample {
    Pixels(Capture),
    /// `text_position`: the ink's mean row and its contrast.
    Text {
        position: f64,
        contrast: u64,
    },
    Instant(Instant),
}

#[derive(Default)]
struct Outcome {
    result: Value,
    failures: Vec<Failure>,
    error: Option<String>,
}

impl Outcome {
    fn error(error: String) -> Self {
        Self {
            error: Some(error),
            ..Self::default()
        }
    }

    fn from(result: Result<(), String>) -> Self {
        result.map_or_else(Self::error, |()| Self::default())
    }

    fn timed_out(window: &gtk::Window, failure: Failure, timeout: Duration) -> Self {
        Self {
            result: json!({ "tree": state::dump(window) }),
            failures: vec![failure],
            error: Some(format!("timed out after {} ms", timeout.as_millis())),
        }
    }
}

impl Controller {
    async fn serve(mut self, mut commands: mpsc::UnboundedReceiver<String>) {
        let _hold = self.application.hold();
        let ready = self.await_ready().await;
        let ready = match ready.and_then(|()| Injector::connect()) {
            Ok(injector) => {
                self.injector = Some(injector);
                Outcome {
                    result: json!({ "ready": true }),
                    ..Outcome::default()
                }
            }
            Err(error) => Outcome::error(error),
        };
        let started = ready.error.is_none();
        self.reply(ready);
        if started {
            while let Some(line) = commands.next().await {
                let outcome = match protocol::parse_command(&line) {
                    Ok(command) => self.execute(command).await,
                    Err(error) => Outcome::error(error),
                };
                self.reply(outcome);
            }
        }
        self.application.quit();
    }

    fn window(&self) -> Option<gtk::Window> {
        self.main.upgrade().or_else(|| {
            self.application
                .active_window()
                .or_else(|| self.application.windows().into_iter().next())
        })
    }

    async fn await_ready(&self) -> Result<(), String> {
        let deadline = Instant::now() + READY_TIMEOUT;
        loop {
            if let Some(window) = self.window().filter(state::is_ready) {
                self.main.set(Some(&window));
                settle(Some(&window)).await;
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err("the Strata window never showed an entry list".to_owned());
            }
            glib::timeout_future(POLL_INTERVAL).await;
        }
    }

    fn reply(&mut self, outcome: Outcome) {
        let app = self
            .window()
            .map(|window| state::scan(&window).observation.app_state())
            .and_then(|app| serde_json::to_value(app).ok())
            .unwrap_or(Value::Null);
        let changed = protocol::changed_keys(self.previous.as_ref(), &app);
        self.previous = Some(app.clone());
        let reply = Reply {
            ok: outcome.error.is_none() && outcome.failures.is_empty(),
            app,
            changed,
            criticals: criticals::take(),
            result: outcome.result,
            failures: outcome.failures,
            error: outcome.error,
        };
        let line = serde_json::to_string(&reply).unwrap_or_else(|error| {
            format!(r#"{{"ok":false,"error":"cannot encode the reply: {error}"}}"#)
        });
        if writeln!(self.protocol, "{line}")
            .and_then(|()| self.protocol.flush())
            .is_err()
        {
            self.application.quit();
        }
    }

    async fn execute(&self, command: Command) -> Outcome {
        let Some(window) = self.window() else {
            return Outcome::error("the Strata window is gone".to_owned());
        };
        let Some(injector) = self.injector.as_ref() else {
            return Outcome::error("no X input connection".to_owned());
        };
        if injector.logging() {
            match window
                .surface()
                .ok_or_else(|| "the window has no surface".to_owned())
                .and_then(|surface| injector.surface_origin(&surface))
            {
                Ok(origin) => injector.set_log_origin(origin),
                Err(error) => return Outcome::error(error),
            }
        }
        let input = Input {
            injector,
            window: &window,
            memory: &self.memory,
            widgets: &self.widgets,
            boxes: &self.settled,
        };
        match command {
            Command::Key { chord, repeat } => {
                for _ in 0..repeat {
                    if let Err(error) = input.press(&chord).await {
                        return Outcome::error(error);
                    }
                }
                Outcome::default()
            }
            Command::Type(text) => {
                for character in text.chars() {
                    let chord = Chord {
                        modifiers: Vec::new(),
                        key: character.to_string(),
                    };
                    if let Err(error) = input.press(&chord).await {
                        return Outcome::error(error);
                    }
                }
                Outcome::default()
            }
            Command::SelectWithKeyboard { name, message } => {
                input.select_with_keyboard(&name, message.as_deref()).await
            }
            Command::OpenMenu {
                target,
                until,
                message,
            } => self.open_menu(&input, &target, &until, message).await,
            Command::Activate { target, message } => {
                Outcome::from(input.activate(&target, message.as_deref()).await)
            }
            Command::Holds(predicate) => match self.check(&input, &predicate) {
                Ok(verdict) => Outcome {
                    result: json!(verdict.is_ok()),
                    ..Outcome::default()
                },
                Err(error) => Outcome::error(error),
            },
            Command::Remember { target, name } => match input.find(&target, None, Aim::Point).await
            {
                Ok(located) if matches!(target, Target::Field(None) | Target::FieldWithin(_)) => {
                    self.widgets.borrow_mut().remove(&name);
                    self.memory
                        .borrow_mut()
                        .insert(name, located.widget.downgrade());
                    Outcome::default()
                }
                Ok(located) => {
                    self.memory.borrow_mut().remove(&name);
                    self.widgets
                        .borrow_mut()
                        .insert(name.clone(), located.widget.downgrade());
                    self.settled.borrow_mut().insert(name, located.rect);
                    Outcome::default()
                }
                Err(error) => Outcome::error(error),
            },
            Command::Note {
                value,
                dir,
                wait,
                message,
                from,
                through,
                unless_first,
                of,
                middle,
                plus,
                sorted,
                pick,
                expr,
                until,
                until_names,
            } => {
                let shape = Shape {
                    from: from.as_deref(),
                    through: through.as_deref(),
                    middle,
                    plus: &plus,
                    sorted,
                    pick,
                };
                let read = || match value {
                    Accessor::ScrollTarget => {
                        let Some(of) = of.as_ref() else {
                            return Err("scroll_target needs of".to_owned());
                        };
                        scroll_target(&input, of, dir.as_deref())
                    }
                    Accessor::Bounds => {
                        let Some(of) = of.as_ref() else {
                            return Err("bounds needs of".to_owned());
                        };
                        let [x, y, width, height] = input
                            .locate_now(of, None, Aim::Point)
                            .map_err(|error| error.message().to_owned())?
                            .rect
                            .pixels();
                        Ok(json!(format!(
                            "Bounds(x={x}, y={y}, width={width}, height={height})"
                        )))
                    }
                    Accessor::EditableFields => Ok(json!(targets::editable_fields(&window))),
                    Accessor::Number => {
                        let Some(expr) = expr.as_ref() else {
                            return Err("number needs expr".to_owned());
                        };
                        let number = match evaluate(&input, expr)? {
                            Ok(number) => number,
                            // Polled until found, a number not there yet is no answer yet.
                            Err(_) if wait.is_some() => return Ok(Value::Null),
                            Err(missing) => {
                                return Err(format!("cannot work out the number: {missing}"));
                            }
                        };
                        for bound in &until {
                            match evaluate(&input, &bound.than)? {
                                Ok(than) if bound.op.holds(number, than) => {}
                                _ => return Ok(Value::Null),
                            }
                        }
                        Ok(number_json(number))
                    }
                    Accessor::VisibleTop => shape.apply(visible_top(&input)?),
                    Accessor::Visible => shape.apply(
                        visible_rows(&input, dir.as_deref())?
                            .into_iter()
                            .map(|row| row.name)
                            .collect(),
                    ),
                    _ => note(
                        &window,
                        value,
                        dir.as_deref(),
                        &shape,
                        unless_first.as_deref(),
                    ),
                };
                // A list that does not match yet is no answer yet.
                let read = || {
                    read().map(|value| match &until_names {
                        Some(names) => {
                            let listed: Vec<String> = value
                                .as_array()
                                .into_iter()
                                .flatten()
                                .filter_map(|name| name.as_str().map(str::to_owned))
                                .collect();
                            if value.is_array() && names.matches(&listed) {
                                value
                            } else {
                                Value::Null
                            }
                        }
                        None => value,
                    })
                };
                let Some(timeout) = wait else {
                    return read().map_or_else(Outcome::error, |result| Outcome {
                        result,
                        ..Outcome::default()
                    });
                };
                let deadline = Instant::now() + timeout;
                loop {
                    match read() {
                        Err(error) => return Outcome::error(error),
                        Ok(Value::Null) if Instant::now() >= deadline => {
                            let failure = Failure {
                                predicate: json!({ "note": value }),
                                actual: Value::Null,
                                message,
                            };
                            return Outcome::timed_out(&window, failure, timeout);
                        }
                        Ok(Value::Null) => glib::timeout_future(POLL_INTERVAL).await,
                        Ok(result) => {
                            return Outcome {
                                result,
                                ..Outcome::default()
                            };
                        }
                    }
                }
            }
            Command::Click {
                target,
                button,
                count,
                modifiers,
                at,
                left_bound,
                release_modifiers_first,
            } => {
                let aim = Aimed {
                    target: &target,
                    at: at.as_ref(),
                    left_bound: left_bound.as_deref(),
                };
                Outcome::from(if release_modifiers_first {
                    input
                        .click_releasing_modifiers_first(&aim, button, &modifiers)
                        .await
                } else {
                    input.click(&aim, button, count, &modifiers).await
                })
            }
            Command::ClickResultRows { states, modifiers } => {
                let rows = targets::result_rows(window.upcast_ref());
                for row in rows {
                    let now = targets::states(&row);
                    if !states.matches(now.selected, now.focused) {
                        continue;
                    }
                    if let Err(error) = input.click_widget(&row, &modifiers).await {
                        return Outcome::error(error);
                    }
                }
                Outcome::default()
            }
            Command::Select(target) => input.select(&target).await,
            Command::Move {
                target,
                at,
                left_bound,
            } => Outcome::from(
                input
                    .move_onto(&Aimed {
                        target: &target,
                        at: at.as_ref(),
                        left_bound: left_bound.as_deref(),
                    })
                    .await,
            ),
            Command::MoveBy { x, y } => Outcome::from(input.move_by(x, y).await),
            Command::Sleep(duration) => {
                glib::timeout_future(duration).await;
                Outcome::default()
            }
            Command::SetScrollbar { orientation, value } => {
                Outcome::from(set_scrollbar(&window, orientation, value))
            }
            Command::Wheel {
                target,
                at,
                clicks,
                down,
                modifiers,
                until: None,
            } => Outcome::from(
                input
                    .wheel(&target, at.as_ref(), clicks, down, &modifiers)
                    .await,
            ),
            Command::Wheel {
                target,
                at,
                clicks,
                down,
                modifiers,
                until: Some(repeat),
            } => {
                self.wheel_until(
                    &input,
                    &target,
                    at.as_ref(),
                    clicks,
                    down,
                    &modifiers,
                    &repeat,
                )
                .await
            }
            Command::KeysUntil {
                predicate,
                keys,
                times,
                check_first,
            } => {
                for _ in 0..times {
                    if check_first {
                        match self.check(&input, &predicate) {
                            Err(error) => return Outcome::error(error),
                            Ok(Ok(())) => break,
                            Ok(Err(_)) => {}
                        }
                    }
                    for chord in &keys {
                        if let Err(error) = input.press(chord).await {
                            return Outcome::error(error);
                        }
                    }
                    if !check_first {
                        match self.check(&input, &predicate) {
                            Err(error) => return Outcome::error(error),
                            Ok(Ok(())) => break,
                            Ok(Err(_)) => {}
                        }
                    }
                }
                Outcome::default()
            }
            Command::KeyDown(key) => Outcome::from(input.raw_key(&key, true).await),
            Command::KeyUp(key) => Outcome::from(input.raw_key(&key, false).await),
            Command::During {
                checks,
                duration,
                every,
            } => {
                let deadline = Instant::now() + duration;
                loop {
                    for (predicate, message) in &checks {
                        match self.check(&input, predicate) {
                            Err(error) => return Outcome::error(error),
                            Ok(Err(mut failure)) => {
                                failure.message.clone_from(message);
                                return Outcome {
                                    failures: vec![failure],
                                    error: Some("the predicate stopped holding".to_owned()),
                                    ..Outcome::default()
                                };
                            }
                            Ok(Ok(())) => {}
                        }
                    }
                    if Instant::now() >= deadline {
                        return Outcome::default();
                    }
                    glib::timeout_future(every).await;
                }
            }
            Command::Capture {
                target,
                inset,
                name,
            } => match input
                .find(&target, None, Aim::Point)
                .await
                .and_then(|located| {
                    let [x, y, width, height] = located.rect.pixels();
                    let inset = i64::from(inset);
                    grab(
                        injector,
                        [x + inset, y + inset, width - 2 * inset, height - 2 * inset],
                    )
                }) {
                Ok(capture) => {
                    self.samples
                        .borrow_mut()
                        .insert(name, Sample::Pixels(capture));
                    Outcome::default()
                }
                Err(error) => Outcome::error(error),
            },
            Command::MeasureText { kept, name } => match self.measure_text(injector, &kept) {
                Ok(Ok((position, contrast))) => {
                    self.samples
                        .borrow_mut()
                        .insert(name, Sample::Text { position, contrast });
                    Outcome::default()
                }
                Ok(Err(failure)) => Outcome {
                    failures: vec![failure],
                    error: Some("the label has no ink".to_owned()),
                    ..Outcome::default()
                },
                Err(error) => Outcome::error(error),
            },
            Command::DropSamples {
                kept,
                resting,
                dragging,
                escape,
                name,
            } => {
                self.drop_samples(&input, &kept, &resting, &dragging, escape, name)
                    .await
            }
            Command::Drag {
                from,
                to,
                from_at,
                to_at,
                steps,
                release,
                modifiers,
                after_press,
            } => {
                let check = |predicate: &Predicate| self.check(&input, predicate);
                let ends = DragEnds {
                    from: &from,
                    to: &to,
                    from_at: from_at.as_ref(),
                    to_at: to_at.as_ref(),
                };
                let pressed = AfterPress {
                    checks: &after_press,
                    check: &check,
                };
                match input
                    .drag(&ends, steps, release, &modifiers, &pressed)
                    .await
                {
                    Ok(None) => Outcome::default(),
                    Ok(Some(failure)) => Outcome {
                        failures: vec![failure],
                        error: Some("the check after the press failed".to_owned()),
                        ..Outcome::default()
                    },
                    Err(error) => Outcome::error(error),
                }
            }
            Command::ButtonDown(number) => Outcome::from(
                input
                    .emit(
                        Event::Button {
                            number,
                            pressed: true,
                        },
                        Duration::ZERO,
                    )
                    .await,
            ),
            Command::ButtonUp(number) => Outcome::from(
                input
                    .emit(
                        Event::Button {
                            number,
                            pressed: false,
                        },
                        POINTER_GAP,
                    )
                    .await,
            ),
            Command::Resize { width, height } => {
                let height = height.map_or_else(
                    || u16::try_from(window.height()).map_err(|error| error.to_string()),
                    Ok,
                );
                let resized = window
                    .surface()
                    .ok_or_else(|| "the window has no surface".to_owned())
                    .and_then(|surface| injector.resize(&surface, width, height?));
                settle(Some(&window)).await;
                Outcome::from(resized)
            }
            Command::Settle {
                target,
                timeout,
                name,
            } => self.settle_target(&input, &target, timeout, name).await,
            Command::Wait {
                predicate,
                timeout,
                message,
                samples,
                settle,
                never,
            } => {
                let deadline = Instant::now() + timeout;
                let mut steady = 0;
                let mut seen: Option<Failure> = None;
                loop {
                    if let Some(target) = &settle {
                        let settled = self
                            .settle_target(&input, target, protocol::DEFAULT_WAIT, None)
                            .await;
                        if settled.error.is_some() {
                            return settled;
                        }
                    }
                    if let Some((forbidden, forbidden_message)) = never.as_deref()
                        && seen.is_none()
                    {
                        match self.check(&input, forbidden) {
                            Err(error) => return Outcome::error(error),
                            Ok(Ok(())) => {
                                seen = Some(Failure {
                                    predicate: json!({ "never": forbidden }),
                                    actual: json!("held at a poll"),
                                    message: forbidden_message.clone(),
                                });
                            }
                            Ok(Err(_)) => {}
                        }
                    }
                    match self.check(&input, &predicate) {
                        Err(error) => return Outcome::error(error),
                        Ok(Ok(())) => {
                            steady += 1;
                            if steady >= samples {
                                return match seen {
                                    None => Outcome::default(),
                                    Some(failure) => Outcome {
                                        failures: vec![failure],
                                        error: Some(
                                            "the forbidden predicate held while waiting".to_owned(),
                                        ),
                                        ..Outcome::default()
                                    },
                                };
                            }
                            if Instant::now() >= deadline {
                                let failure = Failure {
                                    predicate: serde_json::to_value(&predicate)
                                        .unwrap_or(Value::Null),
                                    actual: json!(format!("held for {steady} of {samples} polls")),
                                    message,
                                };
                                return Outcome::timed_out(&window, failure, timeout);
                            }
                        }
                        Ok(Err(mut failure)) if Instant::now() >= deadline => {
                            failure.message = message;
                            return Outcome::timed_out(&window, failure, timeout);
                        }
                        Ok(Err(_)) => steady = 0,
                    }
                    glib::timeout_future(POLL_INTERVAL).await;
                }
            }
            Command::Expect {
                predicates,
                message,
                timeout,
                hold,
            } => {
                let deadline = timeout.map(|timeout| Instant::now() + timeout);
                let mut since: Option<Instant> = None;
                loop {
                    let mut failures = Vec::new();
                    for predicate in &predicates {
                        match self.check(&input, predicate) {
                            Ok(Ok(())) => {}
                            Ok(Err(mut failure)) => {
                                failure.message.clone_from(&message);
                                failures.push(failure);
                            }
                            Err(error) => return Outcome::error(error),
                        }
                    }
                    let held = match hold {
                        None => true,
                        Some(hold) if failures.is_empty() => {
                            since.get_or_insert_with(Instant::now).elapsed() >= hold
                        }
                        Some(_) => {
                            since = None;
                            false
                        }
                    };
                    // `_settles` gives up with one last look at the condition,
                    // which need not hold any longer.
                    if (failures.is_empty() && held)
                        || deadline.is_none_or(|deadline| Instant::now() >= deadline)
                    {
                        return Outcome {
                            failures,
                            ..Outcome::default()
                        };
                    }
                    glib::timeout_future(POLL_INTERVAL).await;
                }
            }
            Command::ClipboardSet(text) => match private_display() {
                Ok(display) => {
                    display.clipboard().set_text(&text);
                    display.flush();
                    if let Some(previous) = self.clipboard.replace(Some(display)) {
                        previous.close();
                    }
                    Outcome::default()
                }
                Err(error) => Outcome::error(error),
            },
            Command::Clipboard { text, message } => {
                let display = match self.clipboard.take().map_or_else(private_display, Ok) {
                    Ok(display) => display,
                    Err(error) => return Outcome::error(error),
                };
                let read = read_clipboard(&display).await;
                display.close();
                match read {
                    Some(read) if text.matches(&read) => Outcome::default(),
                    Some(read) => Outcome {
                        failures: vec![Failure {
                            predicate: json!({ "clipboard": text }),
                            actual: json!(abridged(&read)),
                            message,
                        }],
                        error: Some("the clipboard holds other text".to_owned()),
                        ..Outcome::default()
                    },
                    None => {
                        let failure = Failure {
                            predicate: json!({ "clipboard": text }),
                            actual: Value::Null,
                            message: Some(
                                "selected table text on the private display clipboard".to_owned(),
                            ),
                        };
                        Outcome::timed_out(&window, failure, protocol::DEFAULT_WAIT)
                    }
                }
            }
            Command::Query(Query::State) => Outcome {
                result: serde_json::to_value(state::scan(&window).observation.app_state())
                    .unwrap_or(Value::Null),
                ..Outcome::default()
            },
            Command::Query(Query::Tree) => Outcome {
                result: Value::String(state::dump(&window)),
                ..Outcome::default()
            },
            Command::Query(Query::Prefs) => match preferences() {
                Ok(preferences) => Outcome {
                    result: preferences,
                    ..Outcome::default()
                },
                Err(error) => Outcome::error(error),
            },
        }
    }

    /// `open_appearance_menu`: a late startup focus change can close the
    /// menu as it opens, so an unchecked target is clicked again each second.
    async fn open_menu(
        &self,
        input: &Input<'_>,
        target: &Target,
        until: &Predicate,
        message: Option<String>,
    ) -> Outcome {
        let aim = Aimed::plain(target);
        if let Err(error) = input.click(&aim, 1, 1, &[]).await {
            return Outcome::error(error);
        }
        let mut clicked = Instant::now();
        let deadline = clicked + protocol::DEFAULT_WAIT;
        loop {
            match self.check(input, until) {
                Err(error) => return Outcome::error(error),
                Ok(Ok(())) => return Outcome::default(),
                Ok(Err(mut failure)) if Instant::now() >= deadline => {
                    failure.message = message;
                    return Outcome::timed_out(input.window, failure, protocol::DEFAULT_WAIT);
                }
                Ok(Err(_)) => {}
            }
            // The twin tests AT-SPI's `checked`, which GTK never reports for
            // a toggle button (an open menu's toggle is `pressed`), so it
            // re-clicks whenever the menu is still closed; so does this.
            let checked = input
                .locate_now(target, None, Aim::Point)
                .is_ok_and(|located| targets::states(&located.widget).checked);
            if clicked.elapsed() > MENU_RETRY_AFTER && !checked {
                if let Err(error) = input.click(&aim, 1, 1, &[]).await {
                    return Outcome::error(error);
                }
                clicked = Instant::now();
            }
            glib::timeout_future(POLL_INTERVAL).await;
        }
    }

    /// `Strata.settle`: polls until the target's bounds repeat, then keeps
    /// them for later `moved` checks.
    async fn settle_target(
        &self,
        input: &Input<'_>,
        target: &Target,
        timeout: Duration,
        name: Option<String>,
    ) -> Outcome {
        let deadline = Instant::now() + timeout;
        let mut history: Vec<Option<[i64; 4]>> = Vec::new();
        loop {
            let located = input.locate_now(target, None, Aim::Point);
            if let Err(LookupError::Broken(error)) = located {
                return Outcome::error(error);
            }
            history.push(located.as_ref().ok().map(|located| located.rect.pixels()));
            if history.len() > SETTLE_SAMPLES {
                history.remove(0);
            }
            if let Ok(located) = &located
                && history.len() == SETTLE_SAMPLES
                && history.iter().all(|sample| *sample == history[0])
            {
                let mut settled = self.settled.borrow_mut();
                settled.insert(target.to_json().to_string(), located.rect);
                if let Some(name) = name {
                    settled.insert(name, located.rect);
                }
                return Outcome::default();
            }
            if Instant::now() >= deadline {
                let failure = Failure {
                    predicate: json!({ "settle": target }),
                    actual: located.map_or_else(
                        |error| json!(error.message()),
                        |located| json!(located.rect.pixels()),
                    ),
                    message: Some("the target to stop moving".to_owned()),
                };
                return Outcome::timed_out(input.window, failure, timeout);
            }
            glib::timeout_future(POLL_INTERVAL).await;
        }
    }

    /// A wheel's `until`: turns the wheel while the predicate does not hold,
    /// checking first, as a twin's loop does.
    #[expect(
        clippy::too_many_arguments,
        reason = "a wheel's own fields plus its repeat"
    )]
    async fn wheel_until(
        &self,
        input: &Input<'_>,
        target: &Target,
        at: Option<&At>,
        clicks: u32,
        down: bool,
        modifiers: &[Modifier],
        repeat: &Repeat,
    ) -> Outcome {
        let deadline = Instant::now() + repeat.timeout;
        let mut turns = 0;
        loop {
            match self.check(input, &repeat.predicate) {
                Err(error) => return Outcome::error(error),
                Ok(Ok(())) => return Outcome::default(),
                Ok(Err(_)) if repeat.tries.is_some_and(|tries| turns >= tries) => {
                    return Outcome::default();
                }
                Ok(Err(mut failure)) if repeat.tries.is_none() && Instant::now() >= deadline => {
                    failure.message.clone_from(&repeat.message);
                    return Outcome::timed_out(input.window, failure, repeat.timeout);
                }
                Ok(Err(_)) => {}
            }
            if let Err(error) = input.wheel(target, at, clicks, down, modifiers).await {
                return Outcome::error(error);
            }
            turns += 1;
            if repeat.tries.is_none() {
                glib::timeout_future(POLL_INTERVAL).await;
            }
        }
    }

    /// `text_position` over a kept box, from a fresh grab.
    fn measure_text(
        &self,
        injector: &Injector,
        kept: &str,
    ) -> Result<Result<(f64, u64), Failure>, String> {
        let rect = self.kept_box(kept)?;
        let capture = grab_on_screen(injector, rect.pixels())?;
        Ok(match pixels::text_position(&capture) {
            (Some(position), contrast) => Ok((position, contrast)),
            (None, _) => Err(Failure {
                predicate: json!({ "measure_text": kept }),
                actual: json!(0),
                message: Some("the source label disappeared".to_owned()),
            }),
        })
    }

    fn kept_box(&self, name: &str) -> Result<Rect, String> {
        self.settled
            .borrow()
            .get(name)
            .copied()
            .ok_or_else(|| format!("no box was kept as {name:?}"))
    }

    fn text_sample(&self, name: &str) -> Result<(f64, u64), String> {
        match self.samples.borrow().get(name) {
            Some(Sample::Text { position, contrast }) => Ok((*position, *contrast)),
            _ => Err(format!("no text was measured as {name:?}")),
        }
    }

    /// The drag-completion scenario's release: Escape first when asked, then
    /// the button, with no pause, then a grab at each of the exact delays
    /// rather than the 50 ms poll, each measured only after all are taken.
    async fn drop_samples(
        &self,
        input: &Input<'_>,
        kept: &str,
        resting: &str,
        dragging: &str,
        escape: bool,
        name: String,
    ) -> Outcome {
        let measured = (|| {
            Ok::<_, String>((
                self.kept_box(kept)?,
                self.text_sample(resting)?.0,
                self.text_sample(dragging)?.1,
            ))
        })();
        let (rect, resting_position, dragging_contrast) = match measured {
            Ok(measured) => measured,
            Err(error) => return Outcome::error(error),
        };
        let released = Instant::now();
        self.samples
            .borrow_mut()
            .insert(name, Sample::Instant(released));
        let mut events = Vec::new();
        if escape {
            match input::keysym("Escape").and_then(|keysym| input.injector.keycode(keysym)) {
                Ok((keycode, _)) => {
                    events.push(Event::Key {
                        keycode,
                        pressed: true,
                    });
                    events.push(Event::Key {
                        keycode,
                        pressed: false,
                    });
                }
                Err(error) => return Outcome::error(error),
            }
        }
        events.push(Event::Button {
            number: 1,
            pressed: false,
        });
        for event in events {
            if let Err(error) = input.injector.send(event) {
                return Outcome::error(error);
            }
        }
        let mut grabs = Vec::new();
        for delay in DROP_SAMPLE_DELAYS {
            if let Some(remaining) = (released + delay).checked_duration_since(Instant::now()) {
                glib::timeout_future(remaining).await;
            }
            match grab_on_screen(input.injector, rect.pixels()) {
                Ok(capture) => grabs.push((released.elapsed(), capture)),
                Err(error) => return Outcome::error(error),
            }
        }
        let mut samples = Vec::new();
        for (elapsed, capture) in &grabs {
            match pixels::text_position(capture) {
                (Some(position), contrast) => samples.push((*elapsed, position, contrast)),
                (None, _) => return drop_failure("the source label disappeared", &samples),
            }
        }
        if samples
            .iter()
            .any(|(elapsed, _, _)| *elapsed >= DROP_SAMPLE_LIMIT)
        {
            return drop_failure(&python_samples(&samples), &samples);
        }
        if samples
            .iter()
            .any(|(_, position, _)| (position - resting_position).abs() >= DROP_SAMPLE_DRIFT)
        {
            return drop_failure("source label slid out after drag completion", &samples);
        }
        #[expect(
            clippy::cast_precision_loss,
            reason = "pixel sums stay far below 2^52, as Python's float arithmetic assumes"
        )]
        let recovered = samples.last().is_some_and(|(_, _, contrast)| {
            *contrast as f64 > dragging_contrast as f64 * DROP_SAMPLE_RECOVERY
        });
        if !recovered {
            return drop_failure("dragging opacity must recover", &samples);
        }
        Outcome::default()
    }

    /// A mismatch is the predicate not holding; the outer error is a lookup
    /// that broke, which no amount of waiting fixes.
    fn check(
        &self,
        input: &Input<'_>,
        predicate: &Predicate,
    ) -> Result<Result<(), Failure>, String> {
        if !predicate.check.is_live() {
            return Ok(state::scan(input.window).observation.evaluate(predicate));
        }
        let mismatch = |actual: Value| Failure {
            predicate: serde_json::to_value(predicate).unwrap_or(Value::Null),
            actual,
            message: None,
        };
        let verdict = |holds: bool, actual: Value| {
            if holds { Ok(()) } else { Err(mismatch(actual)) }
        };
        let locate = |target: &Target| match input.locate_now(target, None, Aim::Point) {
            Ok(located) => Ok(Some(located)),
            Err(LookupError::Missing(_)) => Ok(None),
            Err(LookupError::Broken(error)) => Err(error),
        };
        Ok(match &predicate.check {
            Check::Any(predicates) => {
                let mut actual = Vec::new();
                for predicate in predicates {
                    match self.check(input, predicate)? {
                        Ok(()) => return Ok(Ok(())),
                        Err(failure) => actual.push(failure.actual),
                    }
                }
                Err(mismatch(json!(actual)))
            }
            Check::Field(check) => {
                let field = input
                    .field(&check.field())?
                    .and_then(|field| FieldState::read(&field));
                verdict(state::field_matches(check, field.as_ref()), json!(field))
            }
            Check::Pref(check) => {
                let actual = preferences()?
                    .get(&check.key)
                    .cloned()
                    .unwrap_or(Value::Null);
                let holds = match (&check.value, &check.contains, &check.one_of) {
                    (Some(value), _, _) => actual == *value,
                    (None, Some(part), _) => match &actual {
                        Value::Array(items) => items.iter().any(|item| item.as_str() == Some(part)),
                        Value::String(text) => text.contains(part.as_str()),
                        _ => false,
                    },
                    (None, None, Some(values)) => values.contains(&actual),
                    (None, None, None) => false,
                };
                verdict(holds, actual)
            }
            Check::EditorWithinViewport(field) => {
                let Some(editor) = input.field(field)? else {
                    return Ok(Err(mismatch(json!("no field"))));
                };
                let window = input.window.upcast_ref::<gtk::Widget>();
                let scene = state::scan(input.window);
                let pane = scene
                    .pane_widgets(None)
                    .and_then(|pane| pane.pane.compute_bounds(window));
                let editor = editor.compute_bounds(window);
                match (editor, pane) {
                    (Some(editor), Some(pane)) => {
                        let editor = [editor.x(), editor.y(), editor.width(), editor.height()]
                            .map(|value| f64::from(value).floor() as i64);
                        let pane_x = f64::from(pane.x()).floor() as i64;
                        let width = i64::from(input.window.width());
                        verdict(
                            state::within_viewport(editor, pane_x, width),
                            json!({ "editor": editor, "pane_x": pane_x, "window_width": width }),
                        )
                    }
                    _ => Err(mismatch(json!("the field or the pane has no bounds"))),
                }
            }
            Check::State(check) => match locate(&check.target)? {
                None => Err(mismatch(json!("not on screen"))),
                Some(located) => {
                    let states = targets::states(&located.widget);
                    let wanted = [
                        (check.selected, states.selected),
                        (check.focused, states.focused),
                        (check.checked, states.checked),
                        (check.pressed, states.pressed),
                        (check.sensitive, states.sensitive),
                        (check.selectable, states.selectable),
                        (check.editable, states.editable),
                    ];
                    verdict(
                        wanted
                            .iter()
                            .all(|(wanted, actual)| wanted.is_none_or(|wanted| wanted == *actual)),
                        json!({
                            "selected": states.selected,
                            "focused": states.focused,
                            "checked": states.checked,
                            "pressed": states.pressed,
                            "sensitive": states.sensitive,
                            "selectable": states.selectable,
                            "editable": states.editable,
                        }),
                    )
                }
            },
            Check::Present(target) => match locate(target)? {
                Some(_) => Ok(()),
                None => Err(mismatch(json!(false))),
            },
            Check::Absent(target) => match locate(target)? {
                Some(located) => Err(mismatch(json!(located.rect.pixels()))),
                None => Ok(()),
            },
            Check::Moved(movement) => {
                let key = movement
                    .since
                    .clone()
                    .unwrap_or_else(|| movement.target.to_json().to_string());
                let Some(before) = self.settled.borrow().get(&key).copied() else {
                    return Err(format!(
                        "moved needs an earlier settle or remember on {key}"
                    ));
                };
                match locate(&movement.target)? {
                    None => Err(mismatch(json!("not on screen"))),
                    Some(now) => {
                        let (settled, now) = (before.pixels(), now.rect.pixels());
                        let moved = match (movement.direction, movement.y_within) {
                            (Direction::Up, _) => now[1] < settled[1],
                            (Direction::Down, _) => now[1] > settled[1],
                            (Direction::Left, _) => now[0] < settled[0],
                            (Direction::Right, _) => now[0] > settled[0],
                            (Direction::None, None) if movement.within.is_some() => {
                                let within = i64::from(movement.within.unwrap_or_default());
                                (now[0] - settled[0]).abs() <= within
                                    && (now[1] - settled[1]).abs() <= within
                                    && now[2..] == settled[2..]
                            }
                            (Direction::None, None) => now == settled,
                            (Direction::None, Some(within)) => {
                                (now[1] - settled[1]).abs() <= i64::from(within)
                            }
                        };
                        verdict(moved, json!({ "settled": settled, "now": now }))
                    }
                }
            }
            Check::OnScreen(target) => match locate(target)? {
                None => Err(mismatch(json!("not on screen"))),
                Some(located) => {
                    let window = targets::screen_rect(input.window.upcast_ref(), input.injector)
                        .map_err(|error| error.message().to_owned())?
                        .pixels();
                    let bounds = located.rect.pixels();
                    verdict(
                        state::on_screen(bounds, window),
                        json!({ "bounds": bounds, "window": window }),
                    )
                }
            },
            Check::Blank(blank) => match locate(&blank.target)? {
                None => Err(mismatch(json!("not on screen"))),
                Some(located) => {
                    let point = located.rect.point(blank.at.as_ref());
                    let point = (point.0.floor() as i64, point.1.floor() as i64);
                    let mut covering = Vec::new();
                    for widget in targets::presented(&located.widget).into_iter().skip(1) {
                        let role = targets::role_name(&widget);
                        if matches!(role, "panel" | "filler" | "group")
                            || state::rendered_bounds(&widget).is_none()
                        {
                            continue;
                        }
                        let Ok(rect) = targets::screen_rect(&widget, input.injector) else {
                            continue;
                        };
                        if state::contains(rect.pixels(), point) {
                            covering.push(json!({
                                "role": role,
                                "name": targets::accessible_name(&widget),
                            }));
                        }
                    }
                    verdict(covering.is_empty(), json!(covering))
                }
            },
            Check::Not(inner) => match self.check(input, inner)? {
                Ok(()) => Err(mismatch(json!("the negated predicate holds"))),
                Err(_) => Ok(()),
            },
            Check::Visible(names) => {
                let visible: Vec<String> = visible_rows(input, predicate.dir.as_deref())?
                    .into_iter()
                    .map(|row| row.name)
                    .collect();
                verdict(names.matches(&visible), json!(visible))
            }
            Check::ScrollValue(check) => {
                let value = scroll_value(input, predicate.dir.as_deref())?;
                let wanted = check.at_least.as_f64().unwrap_or(f64::INFINITY);
                verdict(value.is_some_and(|value| value >= wanted), json!(value))
            }
            Check::BandSelected(band) => {
                let (from, to) = (self.kept_box(&band.from)?, self.kept_box(&band.to)?);
                let [from_x, ..] = from.pixels();
                let [to_x, to_y, ..] = to.pixels();
                let inside: Vec<(String, bool)> = visible_rows(input, predicate.dir.as_deref())?
                    .into_iter()
                    .filter(|row| {
                        let [x, y, width, height] = row.name_box;
                        y + height <= to_y && x < to_x && x + width > from_x
                    })
                    .map(|row| (row.name, row.selected))
                    .collect();
                verdict(
                    !inside.is_empty() && inside.iter().all(|(_, selected)| *selected),
                    json!(inside),
                )
            }
            Check::FocusWithin(target) => match locate(target)? {
                None => Err(mismatch(json!("not on screen"))),
                Some(located) => {
                    let focus = gtk::prelude::GtkWindowExt::focus(input.window)
                        .filter(|focus| focus.has_focus() && focus.is_mapped());
                    let within = focus.as_ref().is_some_and(|focus| {
                        *focus == located.widget || focus.is_ancestor(&located.widget)
                    });
                    verdict(
                        within,
                        json!(focus.map(|focus| targets::accessible_name(&focus))),
                    )
                }
            },
            Check::Resized(resized) => {
                let before = self.kept_box(&resized.since)?.pixels();
                match locate(&resized.target)? {
                    None => Err(mismatch(json!("not on screen"))),
                    Some(now) => {
                        let now = now.rect.pixels();
                        verdict(
                            resized.width.matches(now[2] - before[2]),
                            json!({ "kept": before, "now": now }),
                        )
                    }
                }
            }
            Check::ContainsPoint(contains) => {
                match (locate(&contains.target)?, locate(&contains.point)?) {
                    (Some(target), Some(point)) => {
                        let [x, y, width, height] = target.rect.pixels();
                        let (point_x, point_y) = point.point;
                        let (start, extent, at) = match contains.axis {
                            PointAxis::X => (x, width, point_x.floor() as i64),
                            PointAxis::Y => (y, height, point_y.floor() as i64),
                        };
                        verdict(
                            start <= at && at < start + extent,
                            json!({ "bounds": target.rect.pixels(), "point": [point_x, point_y] }),
                        )
                    }
                    _ => Err(mismatch(json!("not on screen"))),
                }
            }
            Check::FooterFill(fill) => match footer_fill(input, fill)? {
                Ok(()) => Ok(()),
                Err(actual) => Err(mismatch(actual)),
            },
            Check::Repainted(repainted) => {
                let Some(Sample::Pixels(before)) =
                    self.samples.borrow().get(&repainted.since).cloned()
                else {
                    return Err(format!("no capture was kept as {:?}", repainted.since));
                };
                let width = i64::try_from(before.width).map_err(|error| error.to_string())?;
                let height = i64::try_from(before.height).map_err(|error| error.to_string())?;
                let after = grab(input.injector, [before.x, before.y, width, height])?;
                let pointer = (before.x + width / 2, before.y + height / 2);
                verdict(
                    pixels::differs_beside_pointer(&before, &after, pointer),
                    json!("no pixel changed beside the pointer"),
                )
            }
            Check::ContrastBelow(below) => {
                let (_, of) = self.text_sample(&below.of)?;
                let (_, to) = self.text_sample(&below.to)?;
                let share = below.share.as_f64().unwrap_or_default();
                #[expect(
                    clippy::cast_precision_loss,
                    reason = "pixel sums stay far below 2^52, as Python's float arithmetic assumes"
                )]
                let holds = (of as f64) < to as f64 * share;
                verdict(holds, json!({ "of": of, "to": to }))
            }
            Check::Elapsed(elapsed) => match self.samples.borrow().get(&elapsed.since) {
                Some(Sample::Instant(since)) => {
                    let passed = since.elapsed();
                    verdict(
                        passed >= Duration::from_millis(elapsed.ms),
                        json!(passed.as_millis()),
                    )
                }
                _ => return Err(format!("no instant was kept as {:?}", elapsed.since)),
            },
            Check::DiagnosticsMention(words) => {
                let words: Vec<String> = words.iter().map(|word| word.to_lowercase()).collect();
                let mentioned = self.application.windows().iter().any(|window| {
                    targets::presented(window.upcast_ref())
                        .iter()
                        .any(|widget| {
                            let name = targets::accessible_name(widget).to_lowercase();
                            let description = crate::ui::accessibility_qa::naming(widget)
                                .and_then(|naming| naming.description)
                                .unwrap_or_default()
                                .to_lowercase();
                            words
                                .iter()
                                .any(|word| name.contains(word) || description.contains(word))
                        })
                });
                verdict(mentioned, json!(false))
            }
            Check::All(predicates) => {
                let mut actual = Vec::new();
                let mut holds = true;
                for predicate in predicates {
                    match self.check(input, predicate)? {
                        Ok(()) => actual.push(json!(true)),
                        Err(failure) => {
                            holds = false;
                            actual.push(failure.actual);
                        }
                    }
                }
                verdict(holds, json!(actual))
            }
            Check::EditableFields(expected) => {
                let count = targets::editable_fields(input.window);
                verdict(count == *expected, json!(count))
            }
            Check::Description(check) => match locate(&check.target)? {
                None => Err(mismatch(json!("not on screen"))),
                Some(located) => {
                    let description = targets::accessible_description(&located.widget);
                    verdict(check.text.matches(&description), json!(description))
                }
            },
            Check::Mentions(mention) => match locate(&mention.target)? {
                None => Err(mismatch(json!("not on screen"))),
                Some(located) => {
                    let mut widgets = targets::presented(&located.widget);
                    if let Some(role) = &mention.role {
                        widgets.retain(|widget| {
                            targets::role_name(widget) == role
                                && state::rendered_bounds(widget).is_some()
                        });
                    }
                    let found = match (&mention.text, &mention.words) {
                        _ if mention.equals.is_some() => {
                            let equals = mention.equals.as_deref().unwrap_or_default();
                            widgets
                                .iter()
                                .filter(|widget| state::rendered_bounds(widget).is_some())
                                .any(|widget| {
                                    state::collapse_whitespace(&targets::accessible_name(widget))
                                        == equals
                                })
                        }
                        (Some(text), _) => widgets.iter().any(|widget| {
                            targets::accessible_name(widget).contains(text.as_str())
                                || targets::accessible_description(widget).contains(text.as_str())
                        }),
                        // `dialog.find(predicate=...)` looks only at rendered nodes.
                        (None, Some(words)) => {
                            let words = state::collapse_whitespace(words);
                            widgets
                                .iter()
                                .filter(|widget| state::rendered_bounds(widget).is_some())
                                .any(|widget| {
                                    let shown = format!(
                                        "{} {}",
                                        targets::accessible_name(widget),
                                        targets::text_content(widget).unwrap_or_default()
                                    );
                                    state::collapse_whitespace(&shown).contains(&words)
                                })
                        }
                        (None, None) => false,
                    };
                    verdict(found, json!(false))
                }
            },
            Check::Presented(target) => {
                let found = targets::presented_target(
                    input.window,
                    input.injector,
                    Kept {
                        memory: &input.memory.borrow(),
                        widgets: &input.widgets.borrow(),
                        boxes: &input.boxes.borrow(),
                    },
                    target,
                )
                .map_err(|error| error.message().to_owned())?;
                verdict(found.is_some(), json!(false))
            }
            Check::File(file) => {
                let exists = Path::new(&file.path).symlink_metadata().is_ok();
                verdict(exists == file.exists, json!(exists))
            }
            Check::FocusedWidget(expected) => {
                let focused = targets::presented(input.window.upcast_ref())
                    .into_iter()
                    .find(|widget| {
                        targets::states(widget).focused && state::rendered_bounds(widget).is_some()
                    })
                    .map(|widget| targets::accessible_name(&widget));
                let holds = match (expected, &focused) {
                    (None, None) => true,
                    (Some(expected), Some(focused)) => expected.matches(focused),
                    _ => false,
                };
                verdict(holds, json!(focused))
            }
            Check::Inside(inside) => match (locate(&inside.target)?, locate(&inside.of)?) {
                (Some(target), Some(of)) => {
                    let (inner, outer) = (target.rect.pixels(), of.rect.pixels());
                    verdict(
                        state::inside(inner, outer, inside.axis),
                        json!({ "target": inner, "of": outer }),
                    )
                }
                _ => Err(mismatch(json!("not on screen"))),
            },
            Check::EditorWithinWindow(check) => {
                let Some(editor) = input.field(&FieldRef {
                    of: Some(check.of.clone()),
                    name: None,
                })?
                else {
                    return Ok(Err(mismatch(json!("no field"))));
                };
                let window = input.window.upcast_ref::<gtk::Widget>();
                let scene = state::scan(input.window);
                let pane = scene
                    .pane_widgets(check.pane.as_deref())
                    .and_then(|pane| pane.pane.compute_bounds(window));
                match (editor.compute_bounds(window), pane) {
                    (Some(editor), Some(pane)) => {
                        let editor = [editor.x(), editor.y(), editor.width(), editor.height()]
                            .map(|value| f64::from(value).floor() as i64);
                        let pane_x = f64::from(pane.x()).floor() as i64;
                        let width = i64::from(input.window.width());
                        verdict(
                            state::within_window(editor, pane_x, width),
                            json!({ "editor": editor, "pane_x": pane_x, "window_width": width }),
                        )
                    }
                    _ => Err(mismatch(json!("the field or the pane has no bounds"))),
                }
            }
            Check::Replaced(replaced) => {
                let Some(kept) = input.widgets.borrow().get(&replaced.since).cloned() else {
                    return Err(if input.memory.borrow().contains_key(&replaced.since) {
                        format!(
                            "{:?} was remembered as a field, not a widget",
                            replaced.since
                        )
                    } else {
                        format!("no widget was remembered as {:?}", replaced.since)
                    });
                };
                match locate(&replaced.target)? {
                    None => Err(mismatch(json!("not on screen"))),
                    Some(now) => {
                        verdict(kept.upgrade().as_ref() != Some(&now.widget), json!(false))
                    }
                }
            }
            Check::Pixel(pixel) => match locate(&pixel.target)? {
                None => Err(mismatch(json!("not on screen"))),
                Some(located) => {
                    let [x, y, width, height] = located.rect.pixels();
                    let (x, y) = (x + width.div_euclid(2), y + height.div_euclid(2));
                    let colour = grab(input.injector, [x, y, 1, 1])?.pixels[0];
                    verdict(colour == pixel.rgb, json!(colour))
                }
            },
            Check::AccentPixels(accent) => {
                let wanted = theme_accent()?;
                match locate(&accent.target)? {
                    None => Err(mismatch(json!("not on screen"))),
                    Some(located) => {
                        let [x, y, width, height] = located.rect.pixels();
                        let inset = i64::from(accent.inset);
                        let capture = grab(
                            input.injector,
                            [x + inset, y + inset, width - 2 * inset, height - 2 * inset],
                        )?;
                        let count = capture
                            .pixels
                            .iter()
                            .filter(|colour| **colour == wanted)
                            .count();
                        verdict(
                            count > accent.more_than,
                            json!({ "accent": wanted, "count": count }),
                        )
                    }
                }
            }
            Check::AnyResultRow(wanted) => {
                let rows: Vec<Value> = targets::result_rows(input.window.upcast_ref())
                    .iter()
                    .map(|row| {
                        let states = targets::states(row);
                        json!([
                            targets::accessible_name(row),
                            states.selected,
                            states.focused
                        ])
                    })
                    .collect();
                let holds = targets::result_rows(input.window.upcast_ref())
                    .iter()
                    .any(|row| {
                        let states = targets::states(row);
                        wanted.matches(states.selected, states.focused)
                    });
                verdict(holds, json!(rows))
            }
            Check::HitPath(hit) => {
                // `TXT_HITS[order[0]]` raises a KeyError for a hit it does not list.
                if !hit.hits.contains_key(&hit.name) {
                    return Err(format!("{:?} is not one of the hits", hit.name));
                }
                let shown = hit_path(input.window, hit);
                verdict(
                    shown.as_deref() == hit.hits.get(&hit.name).map(String::as_str),
                    json!(shown),
                )
            }
            Check::Compare(comparison) => {
                let left = evaluate(input, &comparison.left)?;
                let right = evaluate(input, &comparison.right)?;
                match (left, right) {
                    (Ok(left), Ok(right)) => verdict(
                        comparison.op.holds(left, right),
                        json!({ "left": number_json(left), "right": number_json(right) }),
                    ),
                    (Err(missing), _) | (_, Err(missing)) => Err(mismatch(json!(missing))),
                }
            }
            Check::PanesWith(panes) => {
                let Target::Role {
                    role,
                    other_roles,
                    name,
                    description,
                    name_ends_with,
                    nth,
                    states,
                    name_matches,
                    ..
                } = &panes.target
                else {
                    return Err("panes_with takes a role target".to_owned());
                };
                let wanted = targets::Wanted {
                    role,
                    other_roles,
                    name: name.as_deref(),
                    description: description.as_deref(),
                    name_ends_with,
                    nth: *nth,
                    states,
                    name_matches: targets::pattern(name_matches.as_deref())
                        .map_err(|error| error.message().to_owned())?,
                };
                let scene = state::scan(input.window);
                let mut owners = Vec::new();
                for (index, pane) in scene.observation.panes.iter().enumerate() {
                    let Some(widgets) = scene.pane_widgets_at(index) else {
                        continue;
                    };
                    match wanted.find(&widgets.pane) {
                        Ok(_) => owners.push(pane.name.clone()),
                        Err(LookupError::Missing(_)) => {}
                        Err(LookupError::Broken(error)) => return Err(error),
                    }
                }
                verdict(panes.names.matches(&owners), json!(owners))
            }
            Check::Name(check) | Check::Text(check) => {
                let found = if check.rendered {
                    locate(&check.target)?.map(|located| located.widget)
                } else {
                    targets::presented_target(
                        input.window,
                        input.injector,
                        Kept {
                            memory: &input.memory.borrow(),
                            widgets: &input.widgets.borrow(),
                            boxes: &input.boxes.borrow(),
                        },
                        &check.target,
                    )
                    .map_err(|error| error.message().to_owned())?
                };
                match found {
                    None => Err(mismatch(json!("not found"))),
                    Some(widget) => {
                        let shown = if matches!(predicate.check, Check::Text(_)) {
                            targets::text_content(&widget).unwrap_or_default()
                        } else {
                            targets::accessible_name(&widget)
                        };
                        verdict(check.text.matches(&shown), json!(shown))
                    }
                }
            }
            Check::LargestImage(largest) => match locate(&largest.within)? {
                None => Err(mismatch(json!("not on screen"))),
                Some(located) => {
                    let mut boxes = Vec::new();
                    for widget in targets::presented(&located.widget) {
                        if targets::role_name(&widget) != "image"
                            || state::rendered_bounds(&widget).is_none()
                        {
                            continue;
                        }
                        let [_, _, width, height] = targets::screen_rect(&widget, input.injector)
                            .map_err(|error| error.message().to_owned())?
                            .pixels();
                        if width > largest.wider_than {
                            boxes.push([width, height]);
                        }
                    }
                    // `max(images, key=area)` keeps the first of equal areas.
                    let biggest = boxes.iter().copied().reduce(|best, next| {
                        if next[0] * next[1] > best[0] * best[1] {
                            next
                        } else {
                            best
                        }
                    });
                    let holds = match (biggest, largest.size, largest.exceeds) {
                        (None, _, _) => false,
                        (Some(found), Some(size), _) => found == size,
                        (Some([width, height]), None, Some([most_width, most_height])) => {
                            width > most_width || height > most_height
                        }
                        (Some(_), None, None) => true,
                    };
                    verdict(holds, json!(boxes))
                }
            },
            Check::Children(children) => {
                let found = child_processes(&children.comm_prefix)?;
                verdict(found.len() == children.count, json!(found))
            }
            Check::ListSections(sections) => match locate(&sections.within)? {
                None => Err(mismatch(json!("not on screen"))),
                Some(located) => {
                    let grouped = list_sections(&located.widget);
                    let rows = grouped
                        .iter()
                        .find(|(heading, _)| *heading == sections.section)
                        .map(|(_, rows)| rows.clone())
                        .unwrap_or_default();
                    verdict(sections.names.matches(&rows), json!(grouped))
                }
            },
            Check::EditableText(text) => {
                let shown = targets::editable_texts(input.window);
                verdict(shown.iter().any(|shown| shown == text), json!(shown))
            }
            Check::VisibleTop(names) => {
                let visible = visible_top(input)?;
                verdict(names.matches(&visible), json!(visible))
            }
            _ => state::scan(input.window).observation.evaluate(predicate),
        })
    }
}

/// `(pid, state)` of each process whose parent is this one and whose command
/// name starts with `prefix`, as `test_terminal._launcher_children` reads
/// `/proc/<pid>/stat`; a process gone between listing and reading is skipped.
fn child_processes(prefix: &str) -> Result<Vec<(u32, String)>, String> {
    let parent = std::process::id();
    let mut found = Vec::new();
    for entry in std::fs::read_dir("/proc").map_err(|error| format!("/proc: {error}"))? {
        let Ok(entry) = entry else { continue };
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|name| name.parse::<u32>().ok())
        else {
            continue;
        };
        let Ok(stat) = std::fs::read_to_string(entry.path().join("stat")) else {
            continue;
        };
        // The command name sits in parentheses and may hold spaces.
        let (Some(open), Some(close)) = (stat.find(" ("), stat.rfind(") ")) else {
            continue;
        };
        let comm = &stat[open + 2..close];
        let mut rest = stat[close + 2..].split_whitespace();
        let (Some(state), Some(ppid)) = (rest.next(), rest.next()) else {
            continue;
        };
        if ppid.parse::<u32>().ok() == Some(parent) && comm.starts_with(prefix) {
            found.push((pid, state.to_owned()));
        }
    }
    Ok(found)
}

/// The rendered list items below `root` in DFS order, grouped under each
/// unselectable heading row by the first named child of that row, as
/// `test_open_with_names_rows_and_tabs_out_of_the_list` groups them.
fn list_sections(root: &gtk::Widget) -> Vec<(String, Vec<String>)> {
    let mut sections: Vec<(String, Vec<String>)> = Vec::new();
    for row in targets::presented(root) {
        if targets::role_name(&row) != "list item" || state::rendered_bounds(&row).is_none() {
            continue;
        }
        if !targets::states(&row).selectable {
            let mut child = row.first_child();
            let mut heading = String::new();
            while let Some(current) = child {
                child = current.next_sibling();
                if !current.is_visible() {
                    continue;
                }
                let name = targets::accessible_name(&current);
                if !name.is_empty() {
                    heading = name;
                    break;
                }
            }
            if !sections.iter().any(|(existing, _)| *existing == heading) {
                sections.push((heading.clone(), Vec::new()));
            }
            // A heading seen again keeps its rows; later rows land under it.
            let index = sections
                .iter()
                .position(|(existing, _)| *existing == heading)
                .unwrap_or_default();
            let section = sections.remove(index);
            sections.push(section);
        } else if let Some((_, rows)) = sections.last_mut() {
            rows.push(targets::accessible_name(&row));
        }
    }
    sections
}

/// A second connection to the application's display, as
/// `Gdk.Display.open(strata.display.display)` opens one beside the
/// application's own.
fn private_display() -> Result<gdk::Display, String> {
    let name = gdk::Display::default()
        .map(|display| display.name())
        .ok_or("the application has no display")?;
    gdk::Display::open(Some(&name)).ok_or_else(|| format!("cannot open the display {name}"))
}

/// The clipboard's text through `display`, waiting as `strata.wait` would;
/// none when nothing came in time.
async fn read_clipboard(display: &gdk::Display) -> Option<String> {
    let read = display.clipboard().read_text_future();
    match glib::future_with_timeout(protocol::DEFAULT_WAIT, read).await {
        Ok(Ok(Some(text))) => Some(text.to_string()),
        _ => None,
    }
}

/// Long clipboard text cut down for a failure report.
fn abridged(text: &str) -> String {
    const LIMIT: usize = 200;
    match text.char_indices().nth(LIMIT) {
        Some((end, _)) => format!("{}… ({} characters)", &text[..end], text.chars().count()),
        None => text.to_owned(),
    }
}

/// A whole number as an integer, as Python prints `int`s, else a float.
#[expect(
    clippy::cast_possible_truncation,
    reason = "only whole numbers well inside the i64 range are converted"
)]
fn number_json(number: f64) -> Value {
    if number.fract() == 0.0 && number.abs() < 1e15 {
        json!(number as i64)
    } else {
        json!(number)
    }
}

/// Works an [`Expr`] out against the window now. The outer error is a broken
/// lookup or a malformed number; the inner one is something not there yet,
/// such as a target off screen, which a wait keeps polling through, as a
/// twin's condition raising inside `strata.wait` is retried.
fn evaluate(input: &Input<'_>, expr: &Expr) -> Result<Result<f64, String>, String> {
    let pair = |items: &[Expr; 2]| -> Result<Result<(f64, f64), String>, String> {
        Ok(
            match (evaluate(input, &items[0])?, evaluate(input, &items[1])?) {
                (Ok(left), Ok(right)) => Ok((left, right)),
                (Err(missing), _) | (_, Err(missing)) => Err(missing),
            },
        )
    };
    Ok(match expr {
        Expr::Number(number) => Ok(number.as_f64().unwrap_or_default()),
        Expr::Of { target, get } => match input.locate_now(target, None, Aim::Point) {
            Err(LookupError::Broken(error)) => return Err(error),
            Err(LookupError::Missing(missing)) => Err(missing),
            Ok(located) => {
                let [x, y, width, height] = located.rect.pixels();
                #[expect(
                    clippy::cast_precision_loss,
                    reason = "screen coordinates are far below 2^52"
                )]
                let pixels = |value: i64| Ok(value as f64);
                match get {
                    Measure::X => pixels(x),
                    Measure::Y => pixels(y),
                    Measure::Width => pixels(width),
                    Measure::Height => pixels(height),
                    Measure::Right => pixels(x + width),
                    Measure::Bottom => pixels(y + height),
                    Measure::CenterX => pixels(x + width.div_euclid(2)),
                    Measure::CenterY => pixels(y + height.div_euclid(2)),
                    Measure::Value => located
                        .widget
                        .downcast_ref::<gtk::Range>()
                        .map(|range| range.value())
                        .ok_or_else(|| "the target has no numeric value".to_owned()),
                }
            }
        },
        Expr::Pref { key, default } => match preferences()?.get(key) {
            Some(Value::Number(number)) => Ok(number.as_f64().unwrap_or_default()),
            Some(Value::String(text)) => text
                .trim()
                .parse::<f64>()
                .map_err(|_| format!("{key} is {text:?}, not a number")),
            Some(other) => Err(format!("{key} is {other}, not a number")),
            None => default
                .as_ref()
                .and_then(serde_json::Number::as_f64)
                .ok_or_else(|| format!("settings.toml has no {key}")),
        },
        Expr::Count(spec) => {
            let scope = match &spec.within {
                None => input.window.clone().upcast::<gtk::Widget>(),
                Some(within) => match input.locate_now(within, None, Aim::Point) {
                    Err(LookupError::Broken(error)) => return Err(error),
                    Err(LookupError::Missing(missing)) => return Ok(Err(missing)),
                    Ok(located) => located.widget,
                },
            };
            #[expect(
                clippy::cast_precision_loss,
                reason = "a widget count is far below 2^52"
            )]
            let count = targets::presented(&scope)
                .iter()
                .filter(|widget| {
                    spec.role
                        .as_deref()
                        .is_none_or(|role| targets::role_name(widget) == role)
                        && spec
                            .name
                            .as_deref()
                            .is_none_or(|name| targets::accessible_name(widget) == name)
                        && spec.description.as_deref().is_none_or(|description| {
                            targets::accessible_description(widget) == description
                        })
                        && (!spec.rendered || state::rendered_bounds(widget).is_some())
                        && spec
                            .states
                            .iter()
                            .all(|wanted| targets::has_state(widget, *wanted))
                })
                .count() as f64;
            Ok(count)
        }
        Expr::Sum(items) => {
            let mut total = 0.0;
            for item in items {
                match evaluate(input, item)? {
                    Ok(value) => total += value,
                    Err(missing) => return Ok(Err(missing)),
                }
            }
            Ok(total)
        }
        Expr::Minus(items) => pair(items)?.map(|(left, right)| left - right),
        Expr::Times(items) => pair(items)?.map(|(left, right)| left * right),
        Expr::Divide(items) => pair(items)?.and_then(|(left, right)| {
            if right == 0.0 {
                Err("division by zero".to_owned())
            } else {
                Ok(left / right)
            }
        }),
        Expr::FloorDivide(items) => pair(items)?.and_then(|(left, right)| {
            if right == 0.0 {
                Err("division by zero".to_owned())
            } else {
                Ok((left / right).floor())
            }
        }),
        Expr::Ceil(inner) => evaluate(input, inner)?.map(f64::ceil),
        Expr::Abs(inner) => evaluate(input, inner)?.map(f64::abs),
    })
}

/// `test_monitor_selection.visible_entries`: the deepest pane's entries whose
/// top edge lies inside the scroll viewport around its entry list.
fn visible_top(input: &Input<'_>) -> Result<Vec<String>, String> {
    let scene = state::scan(input.window);
    let (Some(pane), Some(widgets)) = (scene.observation.pane(None), scene.pane_widgets(None))
    else {
        return Ok(Vec::new());
    };
    let Some(viewport) = widgets.container.as_ref().and_then(|container| {
        std::iter::successors(container.parent(), WidgetExt::parent)
            .find(|widget| widget.is::<gtk::ScrolledWindow>())
    }) else {
        return Ok(Vec::new());
    };
    let screen = |widget: &gtk::Widget| {
        targets::screen_rect(widget, input.injector).map(|rect| rect.pixels())
    };
    let Ok([_, top, _, height]) = screen(&viewport) else {
        return Ok(Vec::new());
    };
    Ok(pane
        .entries
        .iter()
        .zip(&widgets.entries)
        .filter(|(_, widget)| screen(widget).is_ok_and(|[_, y, _, _]| top <= y && y < top + height))
        .map(|(entry, _)| entry.name.clone())
        .collect())
}

/// The active theme's accent colour, as the twin reads `settings["theme"]`
/// and looks its `accent` up in `data/themes/catalog.toml`.
fn theme_accent() -> Result<[u8; 3], String> {
    let preferences = preferences()?;
    let theme = preferences
        .get("theme")
        .and_then(Value::as_str)
        .ok_or("settings.toml names no theme")?;
    catalog_accent(include_str!("../data/themes/catalog.toml"), theme)
}

/// `ImageColor.getrgb(theme["accent"])` for the catalog theme with this id.
fn catalog_accent(catalog: &str, theme: &str) -> Result<[u8; 3], String> {
    let catalog: toml::Table = toml::from_str(catalog).map_err(|error| error.to_string())?;
    let accent = catalog
        .get("themes")
        .and_then(toml::Value::as_array)
        .into_iter()
        .flatten()
        .find(|entry| entry.get("id").and_then(toml::Value::as_str) == Some(theme))
        .and_then(|entry| entry.get("accent"))
        .and_then(toml::Value::as_str)
        .ok_or_else(|| format!("the theme catalog has no accent for {theme:?}"))?;
    let hex = accent
        .strip_prefix('#')
        .filter(|hex| hex.len() == 6)
        .ok_or_else(|| format!("{accent:?} is not a #rrggbb colour"))?;
    let channel = |index: usize| {
        u8::from_str_radix(&hex[index..index + 2], 16).map_err(|error| error.to_string())
    };
    Ok([channel(0)?, channel(2)?, channel(4)?])
}

/// `footer_hit`: beside the footer mark, the first panel whose rendered
/// labels, joined, are one of the hits' paths.
fn hit_path(window: &gtk::Window, hit: &protocol::HitPath) -> Option<String> {
    let mark = targets::presented(window.upcast_ref())
        .into_iter()
        .find(|widget| {
            targets::role_name(widget) == "label"
                && targets::accessible_name(widget) == hit.mark
                && state::rendered_bounds(widget).is_some()
        })?;
    let parent = mark.parent()?;
    let mut part = parent.first_child();
    while let Some(current) = part {
        part = current.next_sibling();
        if !current.is_visible() || targets::role_name(&current) != "panel" {
            continue;
        }
        let mut text = String::new();
        let mut label = current.first_child();
        while let Some(child) = label {
            label = child.next_sibling();
            if child.is_visible()
                && targets::role_name(&child) == "label"
                && state::rendered_bounds(&child).is_some()
            {
                text.push_str(&targets::accessible_name(&child));
            }
        }
        if hit.hits.values().any(|path| *path == text) {
            return Some(text);
        }
    }
    None
}

/// The samples as the twin's `assert ..., samples` prints them, its message:
/// `[(elapsed, (position, contrast)), ...]` with seconds as Python floats.
fn python_samples(samples: &[(Duration, f64, u64)]) -> String {
    let items: Vec<String> = samples
        .iter()
        .map(|(elapsed, position, contrast)| {
            format!("({:?}, ({position:?}, {contrast}))", elapsed.as_secs_f64())
        })
        .collect();
    format!("[{}]", items.join(", "))
}

fn drop_failure(message: &str, samples: &[(Duration, f64, u64)]) -> Outcome {
    let actual: Vec<Value> = samples
        .iter()
        .map(|(elapsed, position, contrast)| {
            json!({ "elapsed_ms": elapsed.as_millis(), "position": position, "contrast": contrast })
        })
        .collect();
    Outcome {
        failures: vec![Failure {
            predicate: json!({ "drop_samples": message }),
            actual: json!(actual),
            message: Some(message.to_owned()),
        }],
        error: Some(message.to_owned()),
        ..Outcome::default()
    }
}

/// The screen's pixels in a box that must lie wholly on the screen, as
/// `text_position` indexes a screenshot's pixels and fails past its edge.
fn grab_on_screen(injector: &Injector, rect: [i64; 4]) -> Result<Capture, String> {
    let [x, y, width, height] = rect;
    let (screen_width, screen_height) = injector.screen_size()?;
    if x < 0
        || y < 0
        || x + width > i64::from(screen_width)
        || y + height > i64::from(screen_height)
    {
        return Err(format!(
            "the box {rect:?} leaves the {screen_width}x{screen_height} screen"
        ));
    }
    grab(injector, rect)
}

/// The screen's pixels in a whole-pixel box `[x, y, width, height]`; any
/// part off the screen is black, as PIL's `crop` pads a screenshot.
fn grab(injector: &Injector, [x, y, width, height]: [i64; 4]) -> Result<Capture, String> {
    let size = |value: i64| usize::try_from(value).map_err(|error| format!("{value}: {error}"));
    if width <= 0 || height <= 0 {
        return Err(format!("cannot grab an empty box {width}x{height}"));
    }
    let (screen_width, screen_height) = injector.screen_size()?;
    let (left, top) = (x.max(0), y.max(0));
    let right = (x + width).min(i64::from(screen_width));
    let bottom = (y + height).min(i64::from(screen_height));
    let mut pixels = vec![[0, 0, 0]; size(width)? * size(height)?];
    if left < right && top < bottom {
        let narrow = |value: i64| i16::try_from(value).map_err(|error| format!("{value}: {error}"));
        let extent = |value: i64| u16::try_from(value).map_err(|error| format!("{value}: {error}"));
        let visible = injector.grab(
            narrow(left)?,
            narrow(top)?,
            extent(right - left)?,
            extent(bottom - top)?,
        )?;
        let visible_width = size(right - left)?;
        for (row, line) in visible.chunks(visible_width).enumerate() {
            let start = (size(top - y)? + row) * size(width)? + size(left - x)?;
            pixels[start..start + visible_width].copy_from_slice(line);
        }
    }
    Ok(Capture {
        x,
        y,
        width: size(width)?,
        height: size(height)?,
        pixels,
    })
}

/// A row wholly inside its pane's scroll viewport.
struct VisibleRow {
    name: String,
    selected: bool,
    /// The row's first label's box, else the row's, on the X screen.
    name_box: [i64; 4],
}

/// `_visible_entries`: the pane's rows whose boxes lie wholly inside the
/// scroll viewport around its entry list, top to bottom.
fn visible_rows(input: &Input<'_>, directory: Option<&str>) -> Result<Vec<VisibleRow>, String> {
    let scene = state::scan(input.window);
    let (Some(pane), Some(widgets)) = (
        scene.observation.pane(directory),
        scene.pane_widgets(directory),
    ) else {
        return Ok(Vec::new());
    };
    let Some(viewport) = widgets.container.as_ref().and_then(|container| {
        std::iter::successors(container.parent(), WidgetExt::parent)
            .find(|widget| widget.is::<gtk::ScrolledWindow>())
    }) else {
        return Ok(Vec::new());
    };
    let screen = |widget: &gtk::Widget| {
        targets::screen_rect(widget, input.injector).map(|rect| rect.pixels())
    };
    let Ok([_, top, _, height]) = screen(&viewport) else {
        return Ok(Vec::new());
    };
    let mut rows = Vec::new();
    for (entry, widget) in pane.entries.iter().zip(&widgets.entries) {
        let Ok([_, y, _, row_height]) = screen(widget) else {
            continue;
        };
        if row_height <= 0 || y < top || y + row_height > top + height {
            continue;
        }
        let label = targets::presented(widget)
            .into_iter()
            .find(|label| targets::role_name(label) == "label");
        let name_box = label
            .as_ref()
            .and_then(|label| screen(label).ok())
            .map_or_else(|| screen(widget), Ok)
            .map_err(|error| error.message().to_owned())?;
        rows.push(VisibleRow {
            name: entry.name.clone(),
            selected: entry.selected,
            name_box,
        });
    }
    Ok(rows)
}

/// The first scroll bar of any orientation showing in the pane's viewport,
/// `viewport.find(role="scroll bar")`, and its value.
fn scroll_value(input: &Input<'_>, directory: Option<&str>) -> Result<Option<f64>, String> {
    Ok(pane_scroll(input, directory)?.map(|(_, scrollbar)| scrollbar.adjustment().value()))
}

fn pane_scroll(
    input: &Input<'_>,
    directory: Option<&str>,
) -> Result<Option<(gtk::Widget, gtk::Scrollbar)>, String> {
    let scene = state::scan(input.window);
    let Some(viewport) = scene
        .pane_widgets(directory)
        .and_then(|widgets| widgets.container.clone())
        .and_then(|container| {
            std::iter::successors(container.parent(), WidgetExt::parent)
                .find(|widget| widget.is::<gtk::ScrolledWindow>())
        })
    else {
        return Ok(None);
    };
    let scrollbar = targets::presented(&viewport)
        .into_iter()
        .find_map(|widget| {
            state::rendered_bounds(&widget)?;
            widget.downcast::<gtk::Scrollbar>().ok()
        });
    Ok(scrollbar.map(|scrollbar| (viewport, scrollbar)))
}

/// `initial_scroll + max(anchor.y + anchor.height - viewport.y + 1, 1)`.
fn scroll_target(input: &Input<'_>, of: &Target, directory: Option<&str>) -> Result<Value, String> {
    let Some((viewport, scrollbar)) = pane_scroll(input, directory)? else {
        return Err("the pane has no scroll bar showing".to_owned());
    };
    let anchor = input
        .locate_now(of, None, Aim::Point)
        .map_err(|error| error.message().to_owned())?
        .rect
        .pixels();
    let [_, top, ..] = targets::screen_rect(&viewport, input.injector)
        .map_err(|error| error.message().to_owned())?
        .pixels();
    #[expect(
        clippy::cast_precision_loss,
        reason = "screen coordinates are far below 2^52"
    )]
    let past = (anchor[1] + anchor[3] - top + 1).max(1) as f64;
    Ok(json!(scrollbar.adjustment().value() + past))
}

/// `footer_counts_the_remaining_fill`: with both marquee ends listed,
/// cleared, and on screen, a label whose description starts with the count
/// the cleared rows leave, worded as the twin words it.
fn footer_fill(
    input: &Input<'_>,
    fill: &protocol::FooterFill,
) -> Result<Result<(), Value>, String> {
    let scene = state::scan(input.window);
    let (Some(pane), Some(widgets)) = (scene.observation.pane(None), scene.pane_widgets(None))
    else {
        return Ok(Err(json!("no pane")));
    };
    let window = targets::screen_rect(input.window.upcast_ref(), input.injector)
        .map_err(|error| error.message().to_owned())?
        .pixels();
    let ends: Vec<(&state::EntryObservation, &gtk::Widget)> = pane
        .entries
        .iter()
        .zip(&widgets.entries)
        .filter(|(entry, _)| fill.ends.contains(&entry.name))
        .collect();
    let ends_ready = ends.len() == fill.ends.len()
        && ends.iter().all(|(entry, widget)| {
            !entry.selected
                && targets::screen_rect(widget, input.injector)
                    .is_ok_and(|rect| state::on_screen(rect.pixels(), window))
        });
    if !ends_ready {
        return Ok(Err(
            json!({ "ends": ends.iter().map(|(entry, _)| (&entry.name, entry.selected)).collect::<Vec<_>>() }),
        ));
    }
    let cleared = pane.entries.iter().filter(|entry| !entry.selected).count();
    let remaining = format!(
        "{} of {} items selected",
        fill.total.saturating_sub(cleared),
        fill.total
    );
    let found = targets::presented(input.window.upcast_ref())
        .iter()
        .any(|widget| {
            targets::role_name(widget) == "label"
                && state::rendered_bounds(widget).is_some()
                && crate::ui::accessibility_qa::naming(widget)
                    .and_then(|naming| naming.description)
                    .is_some_and(|description| description.starts_with(&remaining))
        });
    Ok(if found {
        Ok(())
    } else {
        Err(json!({ "wanted": remaining }))
    })
}

/// `settings.toml` as JSON, read from disk as `read_preferences` reads it;
/// an absent file is no preferences.
fn preferences() -> Result<Value, String> {
    let path = crate::storage::config_directory().join("settings.toml");
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(format!("cannot read {}: {error}", path.display())),
    };
    let table: toml::Table = toml::from_str(&text)
        .map_err(|error| format!("cannot parse {}: {error}", path.display()))?;
    serde_json::to_value(table).map_err(|error| error.to_string())
}

/// How a `note` reshapes a list it reads, applied in this order.
struct Shape<'a> {
    from: Option<&'a str>,
    through: Option<&'a str>,
    middle: Option<usize>,
    plus: &'a [String],
    sorted: bool,
    pick: Option<i64>,
}

impl Shape<'_> {
    fn apply(&self, names: Vec<String>) -> Result<Value, String> {
        // `names[names.index(from):names.index(through) + 1]`.
        let index = |name: &str| {
            names
                .iter()
                .position(|candidate| candidate == name)
                .ok_or_else(|| format!("{name:?} is not in {names:?}"))
        };
        let start = self.from.map(index).transpose()?.unwrap_or(0);
        let end = self
            .through
            .map(index)
            .transpose()?
            .map_or(names.len(), |end| end + 1);
        let mut names: Vec<String> = names
            .get(start..end.max(start))
            .unwrap_or_default()
            .to_vec();
        if let Some(count) = self.middle {
            let start = names.len() / 2;
            names = names[start..(start + count).min(names.len())].to_vec();
        }
        if !self.plus.is_empty() {
            names.extend(self.plus.iter().cloned());
            names.sort();
            names.dedup();
        }
        if self.sorted {
            names.sort();
        }
        match self.pick {
            None => Ok(json!(names)),
            Some(pick) => {
                let length = i64::try_from(names.len()).map_err(|error| error.to_string())?;
                let index = if pick < 0 { length + pick } else { pick };
                usize::try_from(index)
                    .ok()
                    .and_then(|index| names.get(index))
                    .map(|name| json!(name))
                    .ok_or_else(|| format!("index {pick} is outside {names:?}"))
            }
        }
    }
}

/// A page-object accessor's value now, for `note`; see [`Accessor`].
fn note(
    window: &gtk::Window,
    value: Accessor,
    directory: Option<&str>,
    shape: &Shape<'_>,
    unless_first: Option<&str>,
) -> Result<Value, String> {
    let scene = state::scan(window);
    let observation = &scene.observation;
    let list = |names: Vec<String>| shape.apply(names);
    match value {
        Accessor::Directory => Ok(json!(observation.directory())),
        Accessor::Pane => Ok(json!(observation.panes.last().map(|pane| &pane.name))),
        Accessor::Panes => list(
            observation
                .panes
                .iter()
                .map(|pane| pane.name.clone())
                .collect(),
        ),
        Accessor::Entries => list(observation.entry_names(directory)),
        Accessor::Selected => list(observation.selected_names(directory)),
        Accessor::AllSelected => list(observation.all_selected()),
        Accessor::Matches => list(observation.matches(directory)),
        Accessor::Focused => Ok(json!(observation.focused_entry)),
        Accessor::FocusedPane => Ok(json!(observation.focused_pane_name())),
        Accessor::MiddleVisible => {
            let root = window.upcast_ref::<gtk::Widget>();
            let bounds = |widget: &gtk::Widget| {
                widget.compute_bounds(root).map(|bounds| {
                    [bounds.x(), bounds.y(), bounds.width(), bounds.height()]
                        .map(|value| f64::from(value).floor() as i64)
                })
            };
            let (Some(pane), Some(widgets)) = (observation.pane(None), scene.pane_widgets(None))
            else {
                return Ok(Value::Null);
            };
            let viewport = widgets.container.as_ref().and_then(|container| {
                std::iter::successors(container.parent(), WidgetExt::parent)
                    .find(|widget| widget.is::<gtk::ScrolledWindow>())
            });
            let Some(viewport) = viewport.as_ref().and_then(bounds) else {
                return Ok(Value::Null);
            };
            let entries: Vec<(String, [i64; 4])> = pane
                .entries
                .iter()
                .zip(&widgets.entries)
                .filter_map(|(entry, widget)| Some((entry.name.clone(), bounds(widget)?)))
                .collect();
            Ok(json!(state::middle_visible(
                &entries,
                viewport,
                unless_first.unwrap_or_default()
            )))
        }
        Accessor::Visible
        | Accessor::ScrollTarget
        | Accessor::Bounds
        | Accessor::Number
        | Accessor::VisibleTop => Err(
            "visible, visible_top, scroll_target, bounds, and number are read with the X screen"
                .to_owned(),
        ),
        Accessor::EditableFields => Ok(json!(targets::editable_fields(window))),
        Accessor::MenuItems => list(observation.menu.clone().unwrap_or_default()),
    }
}

/// `Atspi.Value.set_current_value` on the first scroll bar with this
/// orientation, which GTK applies to the scroll bar's adjustment.
fn set_scrollbar(window: &gtk::Window, orientation: Orientation, value: f64) -> Result<(), String> {
    let wanted = match orientation {
        Orientation::Horizontal => gtk::Orientation::Horizontal,
        Orientation::Vertical => gtk::Orientation::Vertical,
    };
    let scrollbar = targets::presented(window.upcast_ref())
        .into_iter()
        .find_map(|widget| {
            widget
                .downcast::<gtk::Scrollbar>()
                .ok()
                .filter(|scrollbar| scrollbar.orientation() == wanted)
        })
        .ok_or_else(|| format!("no {orientation:?} scroll bar"))?;
    let adjustment = scrollbar.adjustment();
    adjustment.set_value(value);
    // The twin asserts `set_current_value` reports success.
    if (adjustment.value() - value).abs() > f64::EPSILON {
        return Err(format!(
            "the {orientation:?} scroll bar took {} instead of {value}",
            adjustment.value()
        ));
    }
    Ok(())
}

/// A pointer aim: the target, `at` on it, and the name of a kept box whose
/// left edge the aim may not pass, as `max(bounds.x, browser_left) + 12`.
struct Aimed<'a> {
    target: &'a Target,
    at: Option<&'a At>,
    left_bound: Option<&'a str>,
}

impl<'a> Aimed<'a> {
    fn plain(target: &'a Target) -> Self {
        Self {
            target,
            at: None,
            left_bound: None,
        }
    }
}

struct DragEnds<'a> {
    from: &'a Target,
    to: &'a Target,
    from_at: Option<&'a At>,
    to_at: Option<&'a At>,
}

/// A predicate holding, a mismatch, or a lookup that broke.
type Verdict = Result<Result<(), Failure>, String>;

/// A drag's `after_press` checks, and how to check one predicate.
struct AfterPress<'a> {
    checks: &'a [PressCheck],
    check: &'a dyn Fn(&Predicate) -> Verdict,
}

impl AfterPress<'_> {
    /// Runs the checks in order, polling a wait as `strata.wait` does; the
    /// first that fails comes back with its message.
    async fn run(&self) -> Result<Option<Failure>, String> {
        for press in self.checks {
            let deadline = press.wait.map(|wait| Instant::now() + wait);
            loop {
                match (self.check)(&press.predicate)? {
                    Ok(()) => break,
                    Err(mut failure) => {
                        if deadline.is_some_and(|deadline| Instant::now() < deadline) {
                            glib::timeout_future(POLL_INTERVAL).await;
                            continue;
                        }
                        failure.message.clone_from(&press.message);
                        return Ok(Some(failure));
                    }
                }
            }
        }
        Ok(None)
    }
}

/// Lets queued X events, the idle work they schedule, and the next frame's
/// layout run before the next event or the reply.
async fn settle(window: Option<&gtk::Window>) {
    glib::timeout_future_with_priority(glib::Priority::LOW, Duration::ZERO).await;
    if let Some(clock) = window.and_then(WidgetExt::frame_clock) {
        let (sender, receiver) = futures_channel::oneshot::channel();
        let sender = RefCell::new(Some(sender));
        let handler = clock.connect_after_paint(move |_| {
            if let Some(sender) = sender.take() {
                let _ = sender.send(());
            }
        });
        clock.request_phase(gdk::FrameClockPhase::AFTER_PAINT);
        let _ = glib::future_with_timeout(FRAME_TIMEOUT, receiver).await;
        clock.disconnect(handler);
    }
    glib::timeout_future_with_priority(glib::Priority::LOW, Duration::ZERO).await;
}

struct Input<'a> {
    injector: &'a Injector,
    window: &'a gtk::Window,
    memory: &'a RefCell<Memory>,
    widgets: &'a RefCell<Memory>,
    /// Boxes and points `settle` and `remember` kept, for `left_bound` and `kept` targets.
    boxes: &'a RefCell<Boxes>,
}

impl Input<'_> {
    /// At least `gap`, as the Python harness sleeps, and at least one frame.
    async fn pause(&self, gap: Duration) {
        let started = Instant::now();
        settle(Some(self.window)).await;
        if let Some(remaining) = gap.checked_sub(started.elapsed()) {
            glib::timeout_future(remaining).await;
        }
    }

    async fn emit(&self, event: Event, gap: Duration) -> Result<(), String> {
        self.injector.send(event)?;
        self.pause(gap).await;
        Ok(())
    }

    async fn key(&self, keycode: u8, pressed: bool) -> Result<(), String> {
        self.emit(Event::Key { keycode, pressed }, EVENT_GAP).await
    }

    /// `connection.key(keysym, pressed)`: a modifier's name or a key's, no tap.
    async fn raw_key(&self, name: &str, pressed: bool) -> Result<(), String> {
        let keysym = match Modifier::parse(name) {
            Ok(modifier) => input::modifier_keysym(modifier)?,
            Err(_) => input::keysym(name)?,
        };
        let (keycode, _) = self.injector.keycode(keysym)?;
        self.emit(Event::Key { keycode, pressed }, Duration::ZERO)
            .await
    }

    async fn hold(&self, modifiers: &[Modifier]) -> Result<Vec<u8>, String> {
        let keycodes = modifiers
            .iter()
            .map(|modifier| {
                let (keycode, _) = self.injector.keycode(input::modifier_keysym(*modifier)?)?;
                Ok(keycode)
            })
            .collect::<Result<Vec<u8>, String>>()?;
        let mut held = Vec::new();
        for keycode in keycodes {
            if let Err(error) = self.key(keycode, true).await {
                self.release(&held).await;
                return Err(error);
            }
            held.push(keycode);
        }
        Ok(held)
    }

    async fn release(&self, held: &[u8]) {
        for keycode in held.iter().rev() {
            let _ = self.key(*keycode, false).await;
        }
    }

    async fn press(&self, chord: &Chord) -> Result<(), String> {
        let (keycode, needs_shift) = self.injector.keycode(input::keysym(&chord.key)?)?;
        if needs_shift {
            self.injector
                .keycode(input::modifier_keysym(Modifier::Shift)?)?;
        }
        let held = self.hold(&chord.modifiers).await?;
        let tapped = self.tap_key(keycode, needs_shift).await;
        self.release(&held).await;
        tapped
    }

    /// The key comes up `EVENT_GAP` after it went down, as the twin's sleep
    /// between them has it, with the server timing the release: waiting for
    /// a frame in between, as other events do, could hold the key past the X
    /// server's 660 ms autorepeat delay while the app is busy with what the
    /// press started, and repeat it.
    async fn tap_key(&self, keycode: u8, needs_shift: bool) -> Result<(), String> {
        let shift = if needs_shift {
            self.hold(&[Modifier::Shift]).await?
        } else {
            Vec::new()
        };
        let up = Event::Key {
            keycode,
            pressed: false,
        };
        let mut result = self.injector.send(Event::Key {
            keycode,
            pressed: true,
        });
        if result.is_ok() {
            result = match self.injector.send_after(up, EVENT_GAP) {
                Ok(()) => {
                    glib::timeout_future(EVENT_GAP).await;
                    self.injector.sync()
                }
                // A key left down would autorepeat until the next press.
                Err(error) => self.injector.send(up).and(Err(error)),
            };
            self.pause(EVENT_GAP).await;
        }
        self.release(&shift).await;
        result
    }

    /// The target as it is now; see [`Self::locate`] for the patient lookup.
    fn locate_now(
        &self,
        target: &Target,
        at: Option<&At>,
        aim: Aim,
    ) -> Result<targets::Located, LookupError> {
        targets::locate(
            self.window,
            self.injector,
            Kept {
                memory: &self.memory.borrow(),
                widgets: &self.widgets.borrow(),
                boxes: &self.boxes.borrow(),
            },
            target,
            at,
            aim,
        )
    }

    /// The field a check names; a remembered one that is gone is no field.
    fn field(&self, field: &FieldRef) -> Result<Option<gtk::Widget>, String> {
        targets::field(self.window, &self.memory.borrow(), field).map_err(|error| match error {
            LookupError::Missing(message) | LookupError::Broken(message) => message,
        })
    }

    /// `Node.activate`: waits for the control as `strata.wait` would, then
    /// invokes its default action, which GTK's AT-SPI bridge maps to
    /// `gtk_widget_activate`.
    /// A role target without `within` may be scrolled away, as a twin's
    /// `window.find(..., rendered=False)` reaches it; any other target is
    /// looked up on screen, as `dialog.find(role="button", name=...)` is.
    async fn activate(&self, target: &Target, message: Option<&str>) -> Result<(), String> {
        let widget = if let Target::Role {
            role,
            other_roles,
            name: Some(name),
            description: None,
            within: None,
            name_ends_with,
            nth: None,
            states,
            name_matches: None,
        } = target
            && name_ends_with.is_empty()
            && states.is_empty()
            && other_roles.is_empty()
        {
            let deadline = Instant::now() + LOCATE_TIMEOUT;
            loop {
                if let Some(widget) = targets::find_presented(self.window.upcast_ref(), role, name)
                {
                    break widget;
                }
                if Instant::now() >= deadline {
                    let waiting = message.map_or_else(String::new, |message| {
                        format!("timed out waiting for {message}: ")
                    });
                    return Err(format!(
                        "{waiting}no {role} named {name:?} after {} s",
                        LOCATE_TIMEOUT.as_secs()
                    ));
                }
                glib::timeout_future(POLL_INTERVAL).await;
            }
        } else {
            self.find(target, None, Aim::Point)
                .await
                .map_err(|error| match message {
                    Some(message) => format!("timed out waiting for {message}: {error}"),
                    None => error,
                })?
                .widget
        };
        if !widget.activate() {
            return Err(format!("{} has no default action", target.to_json()));
        }
        settle(Some(self.window)).await;
        Ok(())
    }

    /// Polls the snapshot until `holds`, as `strata.wait` does, on behalf of
    /// a `select_with_keyboard` for `name`.
    async fn wait_for(
        &self,
        name: &str,
        holds: impl Fn(&state::Observation) -> bool,
        message: String,
    ) -> Result<(), Outcome> {
        let deadline = Instant::now() + protocol::DEFAULT_WAIT;
        loop {
            let observation = state::scan(self.window).observation;
            if holds(&observation) {
                return Ok(());
            }
            if Instant::now() >= deadline {
                let failure = Failure {
                    predicate: json!({ "select_with_keyboard": name }),
                    actual: json!(observation.focused_entry),
                    message: Some(message),
                };
                return Err(Outcome::timed_out(
                    self.window,
                    failure,
                    protocol::DEFAULT_WAIT,
                ));
            }
            glib::timeout_future(POLL_INTERVAL).await;
        }
    }

    /// A failing `select_with_keyboard` names the scenario's step before the
    /// helper's own wait, so its failure lines up with the twin's step.
    async fn select_with_keyboard(&self, name: &str, step: Option<&str>) -> Outcome {
        let mut outcome = self.walk_to(name).await;
        if let Some(step) = step {
            for failure in &mut outcome.failures {
                failure.message = Some(match failure.message.take() {
                    Some(detail) => format!("{step}: {detail}"),
                    None => step.to_owned(),
                });
            }
        }
        outcome
    }

    /// `select_entry_with_keyboard`: Home, then the next-entry key until the
    /// cursor reaches the entry, waiting for each step to move it.
    async fn walk_to(&self, name: &str) -> Outcome {
        if let Err(outcome) = self
            .wait_for(
                name,
                |observation| observation.view_mode().is_some(),
                "a browser pane to be on screen".to_owned(),
            )
            .await
        {
            return outcome;
        }
        let icons = state::scan(self.window).observation.view_mode() == Some("Icons");
        let next = Chord {
            modifiers: Vec::new(),
            key: if icons { "Right" } else { "Down" }.to_owned(),
        };
        let home = Chord {
            modifiers: Vec::new(),
            key: "Home".to_owned(),
        };
        if let Err(error) = self.press(&home).await {
            return Outcome::error(error);
        }
        if let Err(outcome) = self
            .wait_for(
                name,
                |observation| observation.focused_entry.is_some(),
                "the keyboard cursor".to_owned(),
            )
            .await
        {
            return outcome;
        }
        for _ in 0..KEYBOARD_SELECT_STEPS {
            let previous = state::scan(self.window).observation.focused_entry;
            if previous.as_deref() == Some(name) {
                return match self
                    .wait_for(
                        name,
                        |observation| observation.entry(name, None).is_some(),
                        format!("the entry {name:?} to appear"),
                    )
                    .await
                {
                    Ok(()) => Outcome::default(),
                    Err(outcome) => outcome,
                };
            }
            if let Err(error) = self.press(&next).await {
                return Outcome::error(error);
            }
            if let Err(outcome) = self
                .wait_for(
                    name,
                    |observation| observation.focused_entry != previous,
                    format!("the cursor to move past {previous:?}"),
                )
                .await
            {
                return outcome;
            }
        }
        Outcome {
            failures: vec![Failure {
                predicate: json!({ "select_with_keyboard": name }),
                actual: json!(state::scan(self.window).observation.focused_entry),
                message: Some(format!("the keyboard cursor to reach {name:?}")),
            }],
            error: Some(format!("the keyboard cursor never reached {name:?}")),
            ..Outcome::default()
        }
    }

    /// Waits for the target like the Python lookups do, and for its box to
    /// come through a painted frame unchanged: a row a listing has just added
    /// or replaced can report a box before its first allocation lands, which
    /// the Python lookups, slower by an accessibility round trip, never see.
    async fn find(
        &self,
        target: &Target,
        at: Option<&At>,
        aim: Aim,
    ) -> Result<targets::Located, String> {
        let deadline = Instant::now() + LOCATE_TIMEOUT;
        loop {
            match self.locate_now(target, at, aim) {
                Ok(located) => {
                    settle(Some(self.window)).await;
                    match self.locate_now(target, at, aim) {
                        Ok(painted) if painted.rect.pixels() == located.rect.pixels() => {
                            return Ok(painted);
                        }
                        Err(LookupError::Broken(error)) => return Err(error),
                        _ if Instant::now() >= deadline => {
                            return Err(format!(
                                "the target kept moving for {} s",
                                LOCATE_TIMEOUT.as_secs()
                            ));
                        }
                        _ => {}
                    }
                }
                Err(LookupError::Broken(error)) => return Err(error),
                Err(LookupError::Missing(error)) if Instant::now() >= deadline => {
                    return Err(format!("{error} after {} s", LOCATE_TIMEOUT.as_secs()));
                }
                Err(LookupError::Missing(_)) => glib::timeout_future(POLL_INTERVAL).await,
            }
        }
    }

    /// Waits for the target, then the X screen point to aim at.
    async fn locate(
        &self,
        target: &Target,
        at: Option<&At>,
        aim: Aim,
    ) -> Result<(i16, i16), String> {
        let (x, y) = self.find(target, at, aim).await?.point;
        Ok((coordinate(x)?, coordinate(y)?))
    }

    /// Waits for the aim's target, then the X screen point it names.
    async fn aim(&self, aim: &Aimed<'_>) -> Result<(i16, i16), String> {
        let located = self.find(aim.target, aim.at, Aim::Point).await?;
        let (x, y) = match aim.left_bound {
            None => located.point,
            Some(name) => {
                let bound = self.boxes.borrow().get(name).copied().ok_or_else(|| {
                    format!("left_bound needs a box kept as {name:?} by remember or settle")
                })?;
                let left = located.rect.x.max(bound.x);
                Rect {
                    x: left,
                    width: located.rect.x + located.rect.width - left,
                    ..located.rect
                }
                .point(aim.at)
            }
        };
        Ok((coordinate(x)?, coordinate(y)?))
    }

    /// `Pointer.click(node, modifiers=...)` on a widget a step already holds,
    /// once its box comes through a painted frame unchanged.
    async fn click_widget(
        &self,
        widget: &gtk::Widget,
        modifiers: &[Modifier],
    ) -> Result<(), String> {
        let deadline = Instant::now() + LOCATE_TIMEOUT;
        let rect = loop {
            let before = targets::screen_rect(widget, self.injector)
                .map_err(|error| error.message().to_owned())?;
            settle(Some(self.window)).await;
            let after = targets::screen_rect(widget, self.injector)
                .map_err(|error| error.message().to_owned())?;
            if after.pixels() == before.pixels() {
                break after;
            }
            if Instant::now() >= deadline {
                return Err(format!(
                    "the row kept moving for {} s",
                    LOCATE_TIMEOUT.as_secs()
                ));
            }
        };
        let (x, y) = rect.centre();
        self.click_at((coordinate(x)?, coordinate(y)?), 1, 1, modifiers)
            .await
    }

    /// `Pointer.click` and `double_click`: aim, hold the modifiers, tap, let go.
    async fn click(
        &self,
        aim: &Aimed<'_>,
        button: u8,
        count: u32,
        modifiers: &[Modifier],
    ) -> Result<(), String> {
        let point = self.aim(aim).await?;
        self.click_at(point, button, count, modifiers).await
    }

    async fn click_at(
        &self,
        (x, y): (i16, i16),
        button: u8,
        count: u32,
        modifiers: &[Modifier],
    ) -> Result<(), String> {
        self.move_to(x, y).await?;
        let held = self.hold(modifiers).await?;
        let mut result = Ok(());
        for click in 1..=count {
            result = self.tap_button(button, BUTTON_HOLD, click == count).await;
            if result.is_err() {
                break;
            }
            if click < count {
                glib::timeout_future(DOUBLE_CLICK_GAP).await;
            }
        }
        self.release(&held).await;
        self.pause(POINTER_GAP).await;
        result
    }

    /// Only the last release of a multi-click settles; a frame between the
    /// clicks could push them out of the double-click window.
    async fn tap_button(&self, number: u8, hold: Duration, last: bool) -> Result<(), String> {
        self.injector.send(Event::Button {
            number,
            pressed: true,
        })?;
        glib::timeout_future(hold).await;
        self.injector.send(Event::Button {
            number,
            pressed: false,
        })?;
        self.injector.record_release();
        if last {
            self.pause(EVENT_GAP).await;
        } else {
            glib::timeout_future(EVENT_GAP).await;
        }
        Ok(())
    }

    /// `click_releasing_modifiers_before_up`: the modifiers come up while
    /// the button is held, and once more after it, as the twin's `finally` does.
    async fn click_releasing_modifiers_first(
        &self,
        aim: &Aimed<'_>,
        button: u8,
        modifiers: &[Modifier],
    ) -> Result<(), String> {
        let (x, y) = self.aim(aim).await?;
        self.move_to(x, y).await?;
        let held = self.hold(modifiers).await?;
        let pressed = self.injector.send(Event::Button {
            number: button,
            pressed: true,
        });
        let mut result = pressed;
        if result.is_ok() {
            glib::timeout_future(BUTTON_HOLD).await;
            self.release(&held).await;
            result = self.injector.send(Event::Button {
                number: button,
                pressed: false,
            });
            self.injector.record_release();
            self.pause(EVENT_GAP).await;
        }
        self.release(&held).await;
        self.pause(POINTER_GAP).await;
        result
    }

    async fn move_onto(&self, aim: &Aimed<'_>) -> Result<(), String> {
        let (x, y) = self.aim(aim).await?;
        self.move_to(x, y).await
    }

    /// `Pointer.move_to` the pointer's position plus an offset.
    async fn move_by(&self, x: i16, y: i16) -> Result<(), String> {
        let (from_x, from_y) = self.injector.pointer_position()?;
        self.move_to(from_x.saturating_add(x), from_y.saturating_add(y))
            .await
    }

    /// `Pointer.scroll`: buttons 4 and 5 are the wheel turning up and down.
    async fn wheel(
        &self,
        target: &Target,
        at: Option<&At>,
        clicks: u32,
        down: bool,
        modifiers: &[Modifier],
    ) -> Result<(), String> {
        let (x, y) = self.locate(target, at, Aim::Point).await?;
        self.move_to(x, y).await?;
        let held = self.hold(modifiers).await?;
        let mut result = Ok(());
        for _ in 0..clicks {
            result = self
                .tap_button(if down { 5 } else { 4 }, EVENT_GAP, true)
                .await;
            if result.is_err() {
                break;
            }
        }
        self.release(&held).await;
        result
    }

    /// `Pointer.drag` and `drag_points`: press on the source's icon, cross to
    /// the target in even steps so GTK passes its drag threshold and the drop
    /// site sees the hover, then release unless asked to keep holding.
    /// A failed `after_press` check comes back as the failure, with the
    /// button already let go.
    async fn drag(
        &self,
        ends: &DragEnds<'_>,
        steps: u32,
        release: bool,
        modifiers: &[Modifier],
        after_press: &AfterPress<'_>,
    ) -> Result<Option<Failure>, String> {
        let (start, end) = if ends.from_at.is_some() {
            let start = self.locate(ends.from, ends.from_at, Aim::Point).await?;
            (start, self.locate(ends.to, ends.to_at, Aim::Point).await?)
        } else {
            // `Pointer.drag(source, target)` reads both boxes only once both
            // lookups returned, the target's first: a row the wait for the
            // target let a listing refresh move is aimed where it is now.
            self.find(ends.from, None, Aim::DragOrigin).await?;
            let end = self.locate(ends.to, ends.to_at, Aim::Point).await?;
            (self.locate(ends.from, None, Aim::DragOrigin).await?, end)
        };
        let held = self.hold(modifiers).await?;
        let result = self
            .drag_between(start, end, steps, release, after_press)
            .await;
        self.release(&held).await;
        result
    }

    async fn drag_between(
        &self,
        start: (i16, i16),
        end: (i16, i16),
        steps: u32,
        release: bool,
        after_press: &AfterPress<'_>,
    ) -> Result<Option<Failure>, String> {
        self.move_to(start.0, start.1).await?;
        if let Some(remaining) = self
            .injector
            .last_release()
            .and_then(|released| SEPARATE_CLICK_GAP.checked_sub(released.elapsed()))
        {
            glib::timeout_future(remaining).await;
        }
        self.emit(
            Event::Button {
                number: 1,
                pressed: true,
            },
            POINTER_GAP,
        )
        .await?;
        let checked = after_press.run().await;
        if !matches!(checked, Ok(None)) {
            self.injector.send(Event::Button {
                number: 1,
                pressed: false,
            })?;
            return checked;
        }
        let travel = async {
            for step in 1..=steps {
                let x = step_towards(start.0, end.0, step, steps);
                let y = step_towards(start.1, end.1, step, steps);
                self.emit(Event::Motion { x, y }, EVENT_GAP).await?;
            }
            self.emit(Event::Motion { x: end.0, y: end.1 }, POINTER_GAP)
                .await
        };
        let travelled = travel.await;
        if release || travelled.is_err() {
            self.emit(
                Event::Button {
                    number: 1,
                    pressed: false,
                },
                POINTER_GAP,
            )
            .await?;
        }
        travelled.map(|()| None)
    }

    async fn select(&self, target: &EntryTarget) -> Outcome {
        let wanted = Target::Entry(target.clone());
        let selected = || {
            state::scan(self.window)
                .observation
                .entry_selected(&target.entry, target.dir.as_deref())
        };
        for patience in [SELECT_RETRY_AFTER, protocol::DEFAULT_WAIT] {
            if let Err(error) = self.click(&Aimed::plain(&wanted), 1, 1, &[]).await {
                return Outcome::error(error);
            }
            let deadline = Instant::now() + patience;
            while Instant::now() < deadline {
                if selected() {
                    return Outcome::default();
                }
                glib::timeout_future(POLL_INTERVAL).await;
            }
        }
        Outcome {
            result: json!({ "tree": state::dump(self.window) }),
            failures: vec![Failure {
                predicate: json!({ "select": target }),
                actual: json!(false),
                message: Some(format!("{:?} to become selected", target.entry)),
            }],
            error: Some("the entry never became selected".to_owned()),
        }
    }

    /// `Pointer.move_to`: approach, arrive, and confirm the server moved the
    /// pointer, since a press while it is elsewhere lands on the wrong widget.
    async fn move_to(&self, x: i16, y: i16) -> Result<(), String> {
        self.emit(
            Event::Motion {
                x: x.saturating_sub(APPROACH_OFFSET),
                y: y.saturating_sub(APPROACH_OFFSET),
            },
            EVENT_GAP,
        )
        .await?;
        self.emit(Event::Motion { x, y }, POINTER_GAP).await?;
        for _ in 0..ARRIVAL_ATTEMPTS {
            if self.injector.pointer_position()? == (x, y) {
                return Ok(());
            }
            self.emit(Event::Motion { x, y }, POINTER_GAP).await?;
        }
        Err(format!(
            "the pointer did not reach ({x}, {y}); it is at {:?}",
            self.injector.pointer_position()?
        ))
    }
}

/// Python's floor division, so both harnesses visit the same pixels.
fn step_towards(from: i16, to: i16, step: u32, steps: u32) -> i16 {
    let travelled =
        ((i64::from(to) - i64::from(from)) * i64::from(step)).div_euclid(i64::from(steps));
    i16::try_from(i64::from(from) + travelled).unwrap_or(to)
}

fn coordinate(value: f64) -> Result<i16, String> {
    let value = value.floor();
    if value < f64::from(i16::MIN) || value > f64::from(i16::MAX) {
        return Err(format!("the target lies outside the screen at {value}"));
    }
    #[expect(
        clippy::cast_possible_truncation,
        reason = "the range was checked just above"
    )]
    Ok(value as i16)
}

#[cfg(test)]
mod tests;
