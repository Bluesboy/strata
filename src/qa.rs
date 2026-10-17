// SPDX-License-Identifier: MIT

//! `strata --qa <directory>`: the normal window, driven by JSON commands on
//! stdin, with one JSON reply per command on stdout. Built only with the `qa`
//! feature; `tests/qa.rs` is the runner and `docs/e2e-testing.md` the guide.

mod criticals;
mod input;
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
    protocol::{
        At, Check, Chord, Command, Direction, EntryTarget, Failure, FieldRef, Modifier, Predicate,
        Query, Reply, Target,
    },
    state::FieldState,
    targets::{Aim, LookupError, Memory, Rect},
};

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

pub(crate) struct Session {
    protocol: File,
    application_arguments: Vec<OsString>,
}

impl Session {
    /// Takes over stdout for replies before anything can log there; the
    /// application's own output, tracing included, goes to stderr instead.
    pub(crate) fn prepare(arguments: &[OsString]) -> Result<Self, String> {
        let [program, _, directory] = arguments else {
            return Err("usage: strata --qa <directory>".to_owned());
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
            previous: None,
            settled: RefCell::default(),
            memory: RefCell::default(),
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
    previous: Option<Value>,
    /// Where each target came to rest in its last `settle`, keyed by its JSON.
    settled: RefCell<HashMap<String, Rect>>,
    memory: RefCell<Memory>,
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
        self.application
            .active_window()
            .or_else(|| self.application.windows().into_iter().next())
    }

    async fn await_ready(&self) -> Result<(), String> {
        let deadline = Instant::now() + READY_TIMEOUT;
        loop {
            if self.window().is_some_and(|window| state::is_ready(&window)) {
                settle(self.window().as_ref()).await;
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
                Ok(located) => {
                    self.memory
                        .borrow_mut()
                        .insert(name, located.widget.downgrade());
                    Outcome::default()
                }
                Err(error) => Outcome::error(error),
            },
            Command::Click {
                target,
                button,
                count,
                modifiers,
                at,
            } => Outcome::from(
                input
                    .click(&target, button, count, &modifiers, at.as_ref())
                    .await,
            ),
            Command::Select(target) => input.select(&target).await,
            Command::Move { target, at } => {
                Outcome::from(input.move_onto(&target, at.as_ref()).await)
            }
            Command::Wheel {
                target,
                at,
                clicks,
                down,
                modifiers,
            } => Outcome::from(
                input
                    .wheel(&target, at.as_ref(), clicks, down, &modifiers)
                    .await,
            ),
            Command::Drag {
                from,
                to,
                steps,
                release,
                modifiers,
            } => Outcome::from(input.drag(&from, &to, steps, release, &modifiers).await),
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
                let resized = window
                    .surface()
                    .ok_or_else(|| "the window has no surface".to_owned())
                    .and_then(|surface| injector.resize(&surface, width, height));
                settle(Some(&window)).await;
                Outcome::from(resized)
            }
            Command::Settle { target, timeout } => {
                self.settle_target(&input, &target, timeout).await
            }
            Command::Wait {
                predicate,
                timeout,
                message,
            } => {
                let deadline = Instant::now() + timeout;
                loop {
                    match self.check(&input, &predicate) {
                        Err(error) => return Outcome::error(error),
                        Ok(Ok(())) => return Outcome::default(),
                        Ok(Err(mut failure)) if Instant::now() >= deadline => {
                            failure.message = message;
                            return Outcome::timed_out(&window, failure, timeout);
                        }
                        Ok(Err(_)) => glib::timeout_future(POLL_INTERVAL).await,
                    }
                }
            }
            Command::Expect { predicates } => {
                let mut failures = Vec::new();
                for predicate in &predicates {
                    match self.check(&input, predicate) {
                        Ok(Ok(())) => {}
                        Ok(Err(failure)) => failures.push(failure),
                        Err(error) => return Outcome::error(error),
                    }
                }
                Outcome {
                    failures,
                    ..Outcome::default()
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
        if let Err(error) = input.click(target, 1, 1, &[], None).await {
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
                if let Err(error) = input.click(target, 1, 1, &[], None).await {
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
                self.settled
                    .borrow_mut()
                    .insert(target.to_json().to_string(), located.rect);
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
                verdict(actual == check.value, actual)
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
                let key = movement.target.to_json().to_string();
                let Some(before) = self.settled.borrow().get(&key).copied() else {
                    return Err(format!("moved needs an earlier settle on {key}"));
                };
                match locate(&movement.target)? {
                    None => Err(mismatch(json!("not on screen"))),
                    Some(now) => {
                        let (settled, now) = (before.pixels(), now.rect.pixels());
                        let moved = match movement.direction {
                            Direction::Up => now[1] < settled[1],
                            Direction::Down => now[1] > settled[1],
                            Direction::Left => now[0] < settled[0],
                            Direction::Right => now[0] > settled[0],
                            Direction::None => now == settled,
                        };
                        verdict(moved, json!({ "settled": settled, "now": now }))
                    }
                }
            }
            _ => state::scan(input.window).observation.evaluate(predicate),
        })
    }
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

    async fn tap_key(&self, keycode: u8, needs_shift: bool) -> Result<(), String> {
        let shift = if needs_shift {
            self.hold(&[Modifier::Shift]).await?
        } else {
            Vec::new()
        };
        let mut result = self.key(keycode, true).await;
        if result.is_ok() {
            result = self.key(keycode, false).await;
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
            &self.memory.borrow(),
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
    async fn activate(&self, target: &Target, message: Option<&str>) -> Result<(), String> {
        let Target::Role { role, name } = target else {
            return Err("activate takes a role target".to_owned());
        };
        let deadline = Instant::now() + LOCATE_TIMEOUT;
        let widget = loop {
            if let Some(widget) = targets::find_presented(self.window.upcast_ref(), role, name) {
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
        };
        if !widget.activate() {
            return Err(format!("the {role} named {name:?} has no default action"));
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

    /// Waits for the target like the Python lookups do.
    async fn find(
        &self,
        target: &Target,
        at: Option<&At>,
        aim: Aim,
    ) -> Result<targets::Located, String> {
        let deadline = Instant::now() + LOCATE_TIMEOUT;
        loop {
            match self.locate_now(target, at, aim) {
                Ok(located) => return Ok(located),
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

    /// `Pointer.click` and `double_click`: aim, hold the modifiers, tap, let go.
    async fn click(
        &self,
        target: &Target,
        button: u8,
        count: u32,
        modifiers: &[Modifier],
        at: Option<&At>,
    ) -> Result<(), String> {
        let (x, y) = self.locate(target, at, Aim::Point).await?;
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

    async fn move_onto(&self, target: &Target, at: Option<&At>) -> Result<(), String> {
        let (x, y) = self.locate(target, at, Aim::Point).await?;
        self.move_to(x, y).await
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
    async fn drag(
        &self,
        from: &Target,
        to: &Target,
        steps: u32,
        release: bool,
        modifiers: &[Modifier],
    ) -> Result<(), String> {
        let start = self.locate(from, None, Aim::DragOrigin).await?;
        let end = self.locate(to, None, Aim::Point).await?;
        let held = self.hold(modifiers).await?;
        let result = self.drag_between(start, end, steps, release).await;
        self.release(&held).await;
        result
    }

    async fn drag_between(
        &self,
        start: (i16, i16),
        end: (i16, i16),
        steps: u32,
        release: bool,
    ) -> Result<(), String> {
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
        travelled
    }

    async fn select(&self, target: &EntryTarget) -> Outcome {
        let wanted = Target::Entry(target.clone());
        let selected = || {
            state::scan(self.window)
                .observation
                .entry_selected(&target.entry, target.dir.as_deref())
        };
        for patience in [SELECT_RETRY_AFTER, protocol::DEFAULT_WAIT] {
            if let Err(error) = self.click(&wanted, 1, 1, &[], None).await {
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
