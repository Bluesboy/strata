// SPDX-License-Identifier: MIT

//! Runs the `.jsonl` scenarios in `tests/qa/scenarios` against `strata --qa`.
//!
//! Each run gets its own Xvfb, HOME, XDG directories, and fixture tree, seeded
//! from `tests/fixtures/harness.json` exactly as the Python harness seeds
//! them. App commands stream to the process; `layout` and `fs` steps run here.
//! Run through `./scripts/qa.sh`; see `docs/e2e-testing.md`.

use std::{
    collections::BTreeMap,
    fmt::Write as _,
    fs::{self, File},
    io::{BufRead, BufReader, Write},
    os::{fd::AsRawFd, unix::fs::PermissionsExt},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, Stdio},
    sync::{
        Mutex,
        mpsc::{self, Receiver, RecvTimeoutError},
    },
    time::{Duration, Instant, SystemTime},
};

use serde::Deserialize;
use serde_json::{Map, Value, json};
use x11rb::{
    protocol::{xkb, xproto},
    rust_connection::RustConnection,
    wrapper::ConnectionExt as _,
};

macro_rules! scenarios {
    ($($name:ident),* $(,)?) => {
        $(
            #[test]
            #[ignore = "QA scenarios run in the pinned container through ./scripts/qa.sh"]
            fn $name() {
                run_scenario(stringify!($name));
            }
        )*
        const REGISTERED: &[&str] = &[$(stringify!($name)),*];
    };
}

// One test per file in tests/qa/scenarios, named after it with underscores.
scenarios! {
    arrow_keys_move_focus,
}

const REPOSITORY: &str = env!("CARGO_MANIFEST_DIR");
const BINARY: &str = env!("CARGO_BIN_EXE_strata");
const DEFAULT_WAIT: Duration = Duration::from_millis(10_000);
const LAUNCH_TIMEOUT: Duration = Duration::from_secs(90);
const COMMAND_TIMEOUT: Duration = Duration::from_secs(60);
/// Covers the process's own wait plus the drains around it.
const WAIT_MARGIN: Duration = Duration::from_secs(30);
const EXIT_TIMEOUT: Duration = Duration::from_secs(10);
const POLL_INTERVAL: Duration = Duration::from_millis(50);
const FIRST_DISPLAY: u32 = 90;
const DISPLAY_TRIES: u32 = 256;
const XVFB_START_TIMEOUT: Duration = Duration::from_secs(15);
const DEFAULT_LAYOUT: &str = "us";
const ALL_MODIFIERS: u16 = 0xff;
/// The error `strata --qa` replies with before it exits on a harness bug.
const CONTROLLER_PANIC: &str = "the QA controller panicked";
const APP_COMMANDS: [&str; 6] = ["key", "click", "select", "wait", "expect", "query"];
const RUNNER_COMMANDS: [&str; 2] = ["layout", "fs"];
/// Per-test paths the manifest leaves out of its static environment.
const XDG_DIRECTORIES: [(&str, &str); 5] = [
    ("HOME", "home"),
    ("XDG_CONFIG_HOME", "config"),
    ("XDG_DATA_HOME", "data"),
    ("XDG_CACHE_HOME", "cache"),
    ("XDG_STATE_HOME", "state"),
];
/// No AT-SPI here: the process reads its own widget tree. A session bus
/// address that cannot connect keeps GLib from autolaunching one.
const QA_ENVIRONMENT: [(&str, &str); 2] = [("GTK_A11Y", "none"), ("NO_AT_BRIDGE", "1")];

// --------------------------------------------------------------- policy

fn require_container() {
    if std::env::var_os("STRATA_QA_CONTAINER").is_some_and(|value| value == "1") {
        return;
    }
    if std::env::var_os("STRATA_QA_HOST").is_some_and(|value| value == "1") {
        eprintln!(
            "UNSUPPORTED: STRATA_QA_HOST=1 runs QA scenarios on the host toolkit. \
             This run is not validation evidence; use ./scripts/qa.sh."
        );
        return;
    }
    panic!(
        "QA scenarios run only in the pinned container: use ./scripts/qa.sh \
         (STRATA_QA_CONTAINER=1 is set there). A host run needs the owner's explicit \
         consent and STRATA_QA_HOST=1, and is never validation evidence."
    );
}

// ------------------------------------------------------------- manifest

/// `tests/fixtures/harness.json`, shared with the Python harness.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    mtime: i64,
    trees: BTreeMap<String, Value>,
    preferences: OrderedMap,
    gtk_settings: String,
    portal_offer_marker: String,
    environment: BTreeMap<String, String>,
    layouts: BTreeMap<String, Layout>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Layout {
    layouts: String,
    group: u8,
    caps_lock: bool,
    variants: String,
}

/// Keeps document order, so `settings.toml` comes out as the Python harness writes it.
#[derive(Clone, Debug, Default)]
struct OrderedMap(Vec<(String, Value)>);

impl OrderedMap {
    /// Like Python's `dict.update`: replaced keys keep their place, new keys go last.
    fn update(&mut self, key: &str, value: Value) {
        match self.0.iter_mut().find(|(existing, _)| existing == key) {
            Some(entry) => entry.1 = value,
            None => self.0.push((key.to_owned(), value)),
        }
    }
}

impl<'de> Deserialize<'de> for OrderedMap {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Visitor;
        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = OrderedMap;
            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("an object")
            }
            fn visit_map<A: serde::de::MapAccess<'de>>(
                self,
                mut map: A,
            ) -> Result<OrderedMap, A::Error> {
                let mut entries = Vec::new();
                while let Some(entry) = map.next_entry::<String, Value>()? {
                    entries.push(entry);
                }
                Ok(OrderedMap(entries))
            }
        }
        deserializer.deserialize_map(Visitor)
    }
}

fn manifest() -> Manifest {
    let path = Path::new(REPOSITORY).join("tests/fixtures/harness.json");
    let text = fs::read_to_string(&path).expect("the harness manifest should be readable");
    serde_json::from_str(&text).expect("the harness manifest should parse")
}

fn render_toml(values: &OrderedMap) -> String {
    let mut output = String::new();
    for (key, value) in &values.0 {
        let _ = writeln!(output, "{key} = {}", render_value(value));
    }
    output
}

fn render_value(value: &Value) -> String {
    match value {
        Value::Bool(value) => value.to_string(),
        Value::Number(number) => number.to_string(),
        Value::Array(items) => format!(
            "[{}]",
            items
                .iter()
                .map(render_value)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Value::String(text) => format!("\"{}\"", text.replace('\\', "\\\\").replace('"', "\\\"")),
        Value::Null | Value::Object(_) => panic!("settings.toml cannot hold {value}"),
    }
}

fn populate(directory: &Path, tree: &Value) {
    let Value::Object(entries) = tree else {
        panic!("a fixture directory must be an object");
    };
    for (name, value) in entries {
        let path = directory.join(name);
        match value {
            Value::Object(_) => {
                fs::create_dir_all(&path).expect("the fixture directory should be created");
                fs::set_permissions(&path, fs::Permissions::from_mode(0o755))
                    .expect("the fixture directory mode should be set");
                populate(&path, value);
            }
            Value::String(contents) => {
                fs::write(&path, contents).expect("the fixture file should be written");
                fs::set_permissions(&path, fs::Permissions::from_mode(0o644))
                    .expect("the fixture file mode should be set");
            }
            _ => panic!("fixture {name} must be a file's text or a directory"),
        }
    }
}

fn pin_times(path: &Path, mtime: SystemTime) {
    if path.is_dir() {
        let mut children: Vec<PathBuf> = fs::read_dir(path)
            .expect("the fixture directory should be listable")
            .map(|entry| entry.expect("the fixture entry should be readable").path())
            .collect();
        children.sort();
        for child in children {
            pin_times(&child, mtime);
        }
    }
    File::open(path)
        .and_then(|file| {
            file.set_times(fs::FileTimes::new().set_accessed(mtime).set_modified(mtime))
        })
        .expect("the fixture time should be pinned");
}

fn listing(root: &Path) -> String {
    fn walk(root: &Path, directory: &Path, lines: &mut Vec<String>) {
        let Ok(entries) = fs::read_dir(directory) else {
            return;
        };
        let mut paths: Vec<PathBuf> = entries.flatten().map(|entry| entry.path()).collect();
        paths.sort();
        for path in paths {
            let relative = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .display()
                .to_string();
            if path.is_symlink() {
                let target = fs::read_link(&path).unwrap_or_default();
                lines.push(format!("{relative} -> {}", target.display()));
            } else if path.is_dir() {
                lines.push(format!("{relative}/"));
                walk(root, &path, lines);
            } else {
                let size = fs::metadata(&path).map_or(0, |metadata| metadata.len());
                lines.push(format!("{relative} ({size} bytes)"));
            }
        }
    }
    let mut lines = Vec::new();
    walk(root, root, &mut lines);
    lines.join("\n")
}

// ------------------------------------------------------------- scenario

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Header {
    scenario: String,
    modes: Vec<String>,
    #[serde(default)]
    prefs: Map<String, Value>,
    #[serde(default = "default_layout")]
    layout: String,
    twin: String,
    mutation: Option<String>,
    known_bug: Option<String>,
    /// Runs of the file that differ in preferences, layout, or `$variables`.
    #[serde(default)]
    variants: Vec<Variant>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct Variant {
    id: String,
    #[serde(default)]
    modes: Option<Vec<String>>,
    #[serde(default)]
    prefs: Map<String, Value>,
    layout: Option<String>,
    #[serde(default)]
    vars: BTreeMap<String, String>,
}

fn default_layout() -> String {
    DEFAULT_LAYOUT.to_owned()
}

struct Scenario {
    header: Header,
    steps: Vec<Value>,
}

fn scenario_path(name: &str) -> PathBuf {
    Path::new(REPOSITORY)
        .join("tests/qa/scenarios")
        .join(format!("{}.jsonl", name.replace('_', "-")))
}

fn load_scenario(path: &Path) -> Result<Scenario, String> {
    let text = fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))?;
    let mut lines = text
        .lines()
        .enumerate()
        .filter(|(_, line)| !line.trim().is_empty());
    let (_, first) = lines.next().ok_or("the scenario is empty")?;
    let header: Header =
        serde_json::from_str(first).map_err(|error| format!("line 1, the header: {error}"))?;
    let expected = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or_default();
    if header.scenario != expected {
        return Err(format!(
            "the header names {:?} but the file is {expected:?}",
            header.scenario
        ));
    }
    for mode in header.modes.iter().chain(
        header
            .variants
            .iter()
            .flat_map(|variant| variant.modes.iter().flatten()),
    ) {
        if !matches!(mode.as_str(), "columns" | "icons" | "list") {
            return Err(format!("unknown mode {mode:?}"));
        }
    }
    let mut steps = Vec::new();
    for (index, line) in lines {
        let step: Value =
            serde_json::from_str(line).map_err(|error| format!("line {}: {error}", index + 1))?;
        let Some(object) = step.as_object() else {
            return Err(format!("line {}: a step must be an object", index + 1));
        };
        let commands: Vec<&str> = APP_COMMANDS
            .iter()
            .chain(RUNNER_COMMANDS.iter())
            .copied()
            .filter(|name| object.contains_key(*name))
            .collect();
        if commands.len() != 1 {
            return Err(format!(
                "line {}: a step needs exactly one command, found {commands:?}",
                index + 1
            ));
        }
        steps.push(step);
    }
    Ok(Scenario { header, steps })
}

struct Run {
    mode: String,
    variant: Variant,
    label: String,
}

fn runs(header: &Header) -> Vec<Run> {
    let variants = if header.variants.is_empty() {
        vec![Variant::default()]
    } else {
        header.variants.clone()
    };
    let named = !header.variants.is_empty();
    variants
        .into_iter()
        .flat_map(|variant| {
            let modes = variant
                .modes
                .clone()
                .unwrap_or_else(|| header.modes.clone());
            modes.into_iter().map(move |mode| Run {
                label: if named {
                    format!("{mode}-{}", variant.id)
                } else {
                    mode.clone()
                },
                mode,
                variant: variant.clone(),
            })
        })
        .collect()
}

/// A run's `$variables`, plus `$root`: the fixture directory's name, which
/// is also its pane's name.
fn variables(run: &Run, root: &str) -> Vec<(String, String)> {
    let mut variables: Vec<(String, String)> = run.variant.vars.clone().into_iter().collect();
    variables.push(("root".to_owned(), root.to_owned()));
    variables.sort_by_key(|(name, _)| std::cmp::Reverse(name.len()));
    variables
}

/// Replaces `$name` in every string of a step; longer names first so `$next` never eats `$nextish`.
fn substitute(value: &Value, variables: &[(String, String)]) -> Value {
    match value {
        Value::String(text) => {
            let mut text = text.clone();
            for (name, replacement) in variables {
                text = text.replace(&format!("${name}"), replacement);
            }
            Value::String(text)
        }
        Value::Array(items) => Value::Array(
            items
                .iter()
                .map(|item| substitute(item, variables))
                .collect(),
        ),
        Value::Object(object) => Value::Object(
            object
                .iter()
                .map(|(key, item)| (key.clone(), substitute(item, variables)))
                .collect(),
        ),
        other => other.clone(),
    }
}

// --------------------------------------------------------------- display

static DISPLAYS: Mutex<()> = Mutex::new(());
/// Held around every spawn, so only Xvfb ever inherits its `-displayfd` pipe.
static SPAWN: Mutex<()> = Mutex::new(());

fn spawn(command: &mut Command) -> Result<Child, String> {
    let _spawning = SPAWN.lock().map_err(|error| error.to_string())?;
    command.spawn().map_err(|error| error.to_string())
}

/// SIGTERM first, so Xvfb removes its lock file and socket.
fn terminate(child: &mut Child) {
    if matches!(child.try_wait(), Ok(Some(_))) {
        return;
    }
    let _ = rustix::process::kill_process(
        rustix::process::Pid::from_child(child),
        rustix::process::Signal::TERM,
    );
    let deadline = Instant::now() + EXIT_TIMEOUT;
    while Instant::now() < deadline {
        if matches!(child.try_wait(), Ok(Some(_))) {
            return;
        }
        std::thread::sleep(POLL_INTERVAL);
    }
    let _ = child.kill();
    let _ = child.wait();
}

struct Display {
    name: String,
    server: Child,
    log: PathBuf,
    connection: RustConnection,
}

impl Display {
    /// An explicit number keeps clear of other displays; `-displayfd` confirms
    /// that this server, not a concurrently starting one, owns it.
    fn start(log: PathBuf) -> Result<Self, String> {
        let _guard = DISPLAYS.lock().map_err(|error| error.to_string())?;
        for number in FIRST_DISPLAY..FIRST_DISPLAY + DISPLAY_TRIES {
            if Path::new(&format!("/tmp/.X{number}-lock")).exists()
                || Path::new(&format!("/tmp/.X11-unix/X{number}")).exists()
            {
                continue;
            }
            if let Some(display) = Self::try_start(number, &log)? {
                return Ok(display);
            }
        }
        Err(format!(
            "no free X display in :{FIRST_DISPLAY}-:{}",
            FIRST_DISPLAY + DISPLAY_TRIES - 1
        ))
    }

    fn try_start(number: u32, log: &Path) -> Result<Option<Self>, String> {
        let io = |error: std::io::Error| error.to_string();
        let output = File::create(log).map_err(io)?;
        let errors = output.try_clone().map_err(io)?;
        let (reader, writer) = std::io::pipe().map_err(io)?;
        let mut command = Command::new("Xvfb");
        command
            .arg(format!(":{number}"))
            .args(["-displayfd", &writer.as_raw_fd().to_string()])
            .args(["-screen", "0", "1440x900x24", "-dpi", "96"])
            .args(["-nolisten", "tcp", "-noreset", "-ac"])
            .env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .stdin(Stdio::null())
            .stdout(output)
            .stderr(errors);
        let mut server = {
            let _spawning = SPAWN.lock().map_err(|error| error.to_string())?;
            rustix::io::fcntl_setfd(&writer, rustix::io::FdFlags::empty())
                .map_err(|error| error.to_string())?;
            let spawned = command.spawn();
            rustix::io::fcntl_setfd(&writer, rustix::io::FdFlags::CLOEXEC)
                .map_err(|error| error.to_string())?;
            spawned.map_err(|error| format!("cannot start Xvfb: {error}"))?
        };
        let (sender, advertised) = mpsc::channel();
        std::thread::spawn(move || {
            let mut line = String::new();
            let _ = BufReader::new(reader).read_line(&mut line);
            let _ = sender.send(line);
        });
        let deadline = Instant::now() + XVFB_START_TIMEOUT;
        let owned = loop {
            match advertised.recv_timeout(POLL_INTERVAL) {
                Ok(line) => break line == format!("{number}\n"),
                Err(RecvTimeoutError::Timeout)
                    if Instant::now() < deadline && matches!(server.try_wait(), Ok(None)) => {}
                Err(_) => break false,
            }
        };
        // Xvfb writes the number and the newline separately; the pipe stays
        // open until both arrived so the newline cannot hit EPIPE.
        drop(writer);
        let name = format!(":{number}");
        let connection = owned.then(|| x11rb::connect(Some(&name)).ok()).flatten();
        let Some((connection, _)) = connection else {
            terminate(&mut server);
            return Ok(None);
        };
        xkb::use_extension(&connection, 1, 0)
            .map_err(|error| error.to_string())?
            .reply()
            .map_err(|error| error.to_string())?;
        Ok(Some(Self {
            name,
            server,
            log: log.to_owned(),
            connection,
        }))
    }

    fn setxkbmap(&self, arguments: &[&str]) -> Result<String, String> {
        // The explicit display and the PATH-only environment keep this off the desktop.
        let output = spawn(
            Command::new("setxkbmap")
                .args(["-display", &self.name])
                .args(arguments)
                .env_clear()
                .env("PATH", std::env::var_os("PATH").unwrap_or_default())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped()),
        )
        .and_then(|child| child.wait_with_output().map_err(|error| error.to_string()))
        .map_err(|error| format!("cannot run setxkbmap: {error}"))?;
        if !output.status.success() {
            return Err(format!(
                "setxkbmap {} failed: {}",
                arguments.join(" "),
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }

    /// Locks `group`; of the `affect` modifiers, locks those in `locks` and unlocks the rest.
    fn lock(&self, group: u8, affect: u16, locks: u16) -> Result<(), String> {
        xkb::latch_lock_state(
            &self.connection,
            xkb::ID::USE_CORE_KBD.into(),
            xproto::ModMask::from(affect),
            xproto::ModMask::from(locks),
            true,
            xkb::Group::from(group),
            xproto::ModMask::from(0u16),
            false,
            0,
        )
        .map_err(|error| error.to_string())?;
        self.connection.sync().map_err(|error| error.to_string())
    }

    /// As `harness/keyboard_layout.py` does: load the layouts, then lock the group.
    fn switch_layout(&self, layout: &Layout) -> Result<(), String> {
        let mut arguments = vec!["-layout", layout.layouts.as_str()];
        if !layout.variants.is_empty() {
            arguments.extend(["-variant", layout.variants.as_str()]);
        }
        self.setxkbmap(&arguments)?;
        let caps = if layout.caps_lock {
            u16::from(xproto::ModMask::LOCK)
        } else {
            0
        };
        self.lock(layout.group, caps, caps)
    }

    /// setxkbmap keeps locks, so they go first; then US without variants, verified.
    fn restore_layout(&self) -> Result<(), String> {
        self.lock(0, ALL_MODIFIERS, 0)?;
        self.setxkbmap(&["-layout", DEFAULT_LAYOUT])?;
        let query = self.setxkbmap(&["-query"])?;
        let field = |name: &str| {
            query.lines().find_map(|line| {
                let (key, value) = line.split_once(':')?;
                (key.trim() == name).then(|| value.trim().to_owned())
            })
        };
        let state = xkb::get_state(&self.connection, xkb::ID::USE_CORE_KBD.into())
            .map_err(|error| error.to_string())?
            .reply()
            .map_err(|error| error.to_string())?;
        let clean = u8::from(state.group) == 0
            && u8::from(state.locked_group) == 0
            && u16::from(state.locked_mods) == 0;
        if field("layout").as_deref() != Some(DEFAULT_LAYOUT)
            || field("variant").is_some()
            || !clean
        {
            return Err(format!(
                "the keyboard was not restored: layout {:?}, variant {:?}, group {}, locked modifiers {:#x}",
                field("layout"),
                field("variant"),
                u8::from(state.group),
                u16::from(state.locked_mods)
            ));
        }
        Ok(())
    }

    fn screenshot(&self, destination: &Path) -> Result<(), String> {
        let status = spawn(
            Command::new("import")
                .args(["-display", &self.name, "-window", "root"])
                .arg(destination)
                .env_clear()
                .env("PATH", std::env::var_os("PATH").unwrap_or_default())
                .stdout(Stdio::null())
                .stderr(Stdio::null()),
        )
        .and_then(|mut child| child.wait().map_err(|error| error.to_string()))
        .map_err(|error| format!("cannot run import: {error}"))?;
        status
            .success()
            .then_some(())
            .ok_or_else(|| format!("import exited with {status}"))
    }
}

impl Drop for Display {
    fn drop(&mut self) {
        terminate(&mut self.server);
    }
}

// ----------------------------------------------------------- application

struct Application {
    child: Child,
    stdin: Option<ChildStdin>,
    replies: Receiver<String>,
    log: PathBuf,
    /// False once a reply timed out or the process went away.
    responsive: bool,
}

impl Application {
    fn start(
        manifest: &Manifest,
        home: &Path,
        fixture: &Path,
        display: &Display,
        runtime: &Path,
        log: PathBuf,
    ) -> Result<Self, String> {
        let output = File::create(&log).map_err(|error| error.to_string())?;
        let mut command = Command::new(BINARY);
        command
            .arg("--qa")
            .arg(fixture)
            .current_dir(fixture)
            .env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .envs(&manifest.environment)
            .envs(QA_ENVIRONMENT)
            .env("DISPLAY", &display.name)
            .env("XDG_RUNTIME_DIR", runtime)
            .env(
                "DBUS_SESSION_BUS_ADDRESS",
                format!("unix:path={}", runtime.join("no-session-bus").display()),
            )
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(output);
        for (variable, directory) in XDG_DIRECTORIES {
            command.env(variable, home.join(directory));
        }
        let mut child =
            spawn(&mut command).map_err(|error| format!("cannot start {BINARY}: {error}"))?;
        let stdout = child.stdout.take().ok_or("no stdout")?;
        let (sender, replies) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                if sender.send(line).is_err() {
                    break;
                }
            }
        });
        Ok(Self {
            stdin: child.stdin.take(),
            child,
            replies,
            log,
            responsive: true,
        })
    }

    fn receive(&mut self, timeout: Duration) -> Result<Value, String> {
        let received = self.replies.recv_timeout(timeout);
        self.responsive = received.is_ok();
        match received {
            Ok(line) => serde_json::from_str(&line)
                .map_err(|error| format!("unreadable reply {line:?}: {error}")),
            Err(RecvTimeoutError::Timeout) => {
                Err(format!("no reply within {} s", timeout.as_secs()))
            }
            Err(RecvTimeoutError::Disconnected) => Err(format!(
                "strata exited ({}); log tail:\n{}",
                self.child
                    .try_wait()
                    .ok()
                    .flatten()
                    .map_or_else(|| "still running".to_owned(), |status| status.to_string()),
                tail(&self.log)
            )),
        }
    }

    fn send(&mut self, command: &Value, timeout: Duration) -> Result<Value, String> {
        let stdin = self.stdin.as_mut().ok_or("stdin is closed")?;
        writeln!(stdin, "{command}")
            .and_then(|()| stdin.flush())
            .map_err(|error| format!("cannot send {command}: {error}"))?;
        self.receive(timeout)
    }

    fn stop(&mut self) {
        drop(self.stdin.take());
        let deadline = Instant::now() + EXIT_TIMEOUT;
        while Instant::now() < deadline {
            if matches!(self.child.try_wait(), Ok(Some(_))) {
                return;
            }
            std::thread::sleep(POLL_INTERVAL);
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for Application {
    fn drop(&mut self) {
        self.stop();
    }
}

fn tail(path: &Path) -> String {
    let text = fs::read_to_string(path).unwrap_or_default();
    let lines: Vec<&str> = text.lines().collect();
    lines[lines.len().saturating_sub(40)..].join("\n")
}

// ------------------------------------------------------------ filesystem

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FsStep {
    fs: FsCheck,
    timeout_ms: Option<u64>,
}

/// One of `exists`, `text`, or `names` about `path` under the fixture root.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FsCheck {
    path: String,
    exists: Option<bool>,
    text: Option<String>,
    names: Option<Vec<String>>,
}

impl FsCheck {
    fn observe(&self, root: &Path) -> Result<Result<(), String>, String> {
        let path = root.join(&self.path);
        match (self.exists, &self.text, &self.names) {
            (Some(exists), None, None) => {
                let actual = path.symlink_metadata().is_ok();
                Ok((actual == exists)
                    .then_some(())
                    .ok_or(format!("exists is {actual}")))
            }
            (None, Some(text), None) => {
                let actual = fs::read_to_string(&path).map_err(|error| error.to_string());
                Ok(match actual {
                    Ok(actual) if actual == *text => Ok(()),
                    Ok(actual) => Err(format!("text is {actual:?}")),
                    Err(error) => Err(error),
                })
            }
            (None, None, Some(names)) => {
                // Sorted with hidden files, as `FixtureTree.names` lists them.
                let mut actual: Vec<String> = fs::read_dir(&path)
                    .map_err(|error| error.to_string())?
                    .flatten()
                    .map(|entry| entry.file_name().to_string_lossy().into_owned())
                    .collect();
                actual.sort();
                Ok((actual == *names)
                    .then_some(())
                    .ok_or(format!("names are {actual:?}")))
            }
            _ => Err("fs needs a path and exactly one of exists, text, or names".to_owned()),
        }
    }
}

// ------------------------------------------------------------------ runs

/// `errors` are the harness's own problems (setup, launch, layouts,
/// teardown, artifacts) and always fail a run, even one marked `known_bug`;
/// `failures` are the scenario's.
#[derive(Default)]
struct Outcome {
    errors: Vec<String>,
    failures: Vec<String>,
    last_tree: Option<String>,
    transcript: String,
}

fn run_scenario(name: &str) {
    require_container();
    let path = scenario_path(name);
    let scenario =
        load_scenario(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    let manifest = manifest();
    let mut failed = Vec::new();
    for run in runs(&scenario.header) {
        let Outcome {
            errors, failures, ..
        } = execute(&scenario, &run, &manifest);
        if !errors.is_empty() {
            failed.push(format!(
                "{}/{} could not run:\n  {}",
                scenario.header.scenario,
                run.label,
                errors
                    .iter()
                    .chain(&failures)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join("\n  ")
            ));
            continue;
        }
        match (&scenario.header.known_bug, failures.is_empty()) {
            (None, true) => println!("PASS {}/{}", scenario.header.scenario, run.label),
            (Some(bug), false) => println!(
                "KNOWN BUG {}/{} still fails as expected ({bug})",
                scenario.header.scenario, run.label
            ),
            (Some(bug), true) => failed.push(format!(
                "{}/{} passes but is marked known_bug {bug}; remove the marker",
                scenario.header.scenario, run.label
            )),
            (None, false) => failed.push(format!(
                "{}/{} failed:\n  {}",
                scenario.header.scenario,
                run.label,
                failures.join("\n  ")
            )),
        }
    }
    assert!(failed.is_empty(), "{}", failed.join("\n\n"));
}

fn execute(scenario: &Scenario, run: &Run, manifest: &Manifest) -> Outcome {
    let scratch = tempfile::Builder::new()
        .prefix("strata-qa-")
        .tempdir_in("/tmp")
        .expect("the run's scratch directory should be created");
    let fixture = tempfile::Builder::new()
        .prefix("strata-qa-fixture-")
        .tempdir_in("/tmp")
        .expect("the fixture directory should be created");
    let artifacts = Path::new(REPOSITORY)
        .join("target/qa-artifacts")
        .join(&scenario.header.scenario)
        .join(&run.label);
    let _ = fs::remove_dir_all(&artifacts);
    let setup_error = |error: String| Outcome {
        errors: vec![error],
        ..Outcome::default()
    };

    let home = scratch.path().join("home");
    let runtime = scratch.path().join("runtime");
    if let Err(error) = seed(manifest, scenario, run, &home, &runtime, fixture.path()) {
        return setup_error(format!("setup: {error}"));
    }
    let display = match Display::start(scratch.path().join("xvfb.log")) {
        Ok(display) => display,
        Err(error) => return setup_error(format!("setup: {error}")),
    };
    let mut application = match Application::start(
        manifest,
        &home,
        fixture.path(),
        &display,
        &runtime,
        scratch.path().join("strata.log"),
    ) {
        Ok(application) => application,
        Err(error) => return setup_error(format!("launch: {error}")),
    };

    let root_name = fixture
        .path()
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut layout = DEFAULT_LAYOUT.to_owned();
    let mut outcome = drive(
        &mut application,
        &Context {
            scenario,
            initial_layout: run
                .variant
                .layout
                .as_deref()
                .unwrap_or(&scenario.header.layout),
            variables: &variables(run, &root_name),
            manifest,
            display: &display,
            fixture: fixture.path(),
        },
        &mut layout,
    );
    if layout != DEFAULT_LAYOUT
        && let Err(error) = display.restore_layout()
    {
        outcome.errors.push(format!("teardown: {error}"));
    }
    let failed = !outcome.errors.is_empty() || !outcome.failures.is_empty();
    let collect =
        failed || std::env::var_os("STRATA_QA_KEEP_ARTIFACTS").is_some_and(|value| value == "1");
    if collect {
        if outcome.last_tree.is_none()
            && application.responsive
            && let Ok(reply) = application.send(&json!({ "query": "tree" }), COMMAND_TIMEOUT)
        {
            outcome.last_tree = reply["result"].as_str().map(str::to_owned);
        }
        let _ = fs::create_dir_all(&artifacts);
        if let Err(error) = display.screenshot(&artifacts.join("screenshot.png")) {
            outcome.errors.push(format!("artifacts: {error}"));
        }
    }
    application.stop();
    if collect {
        let mut files = vec![
            ("transcript.jsonl", std::mem::take(&mut outcome.transcript)),
            ("fixture.txt", listing(fixture.path())),
            (
                "strata.log",
                fs::read_to_string(&application.log).unwrap_or_default(),
            ),
            (
                "xvfb.log",
                fs::read_to_string(&display.log).unwrap_or_default(),
            ),
        ];
        if failed {
            let all: Vec<String> = outcome
                .errors
                .iter()
                .chain(&outcome.failures)
                .cloned()
                .collect();
            files.push(("failures.txt", all.join("\n")));
        }
        if let Some(tree) = outcome.last_tree.take() {
            files.push(("tree.txt", tree));
        }
        for (file, contents) in files {
            let _ = fs::write(artifacts.join(file), contents);
        }
        if failed {
            outcome
                .failures
                .push(format!("artifacts: {}", artifacts.display()));
        }
    }
    outcome
}

fn seed(
    manifest: &Manifest,
    scenario: &Scenario,
    run: &Run,
    home: &Path,
    runtime: &Path,
    fixture: &Path,
) -> Result<(), String> {
    let io = |error: std::io::Error| error.to_string();
    for (_, directory) in XDG_DIRECTORIES {
        fs::create_dir_all(home.join(directory)).map_err(io)?;
    }
    fs::create_dir_all(runtime).map_err(io)?;
    fs::set_permissions(runtime, fs::Permissions::from_mode(0o700)).map_err(io)?;
    let gtk = home.join("config/gtk-4.0");
    fs::create_dir_all(&gtk).map_err(io)?;
    fs::write(gtk.join("settings.ini"), &manifest.gtk_settings).map_err(io)?;
    let strata = home.join("config/strata");
    fs::create_dir_all(&strata).map_err(io)?;
    // Pre-answers the first-run portal offer, which would cover every scenario.
    fs::write(strata.join(&manifest.portal_offer_marker), "1\n").map_err(io)?;

    let mut preferences = manifest.preferences.clone();
    for (key, value) in scenario.header.prefs.iter().chain(run.variant.prefs.iter()) {
        preferences.update(key, value.clone());
    }
    preferences.update("browser_mode", Value::String(run.mode.clone()));
    fs::write(strata.join("settings.toml"), render_toml(&preferences)).map_err(io)?;

    let tree = manifest
        .trees
        .get("canonical")
        .ok_or("no canonical fixture tree")?;
    populate(fixture, tree);
    pin_times(fixture, fixture_time(manifest));
    Ok(())
}

fn fixture_time(manifest: &Manifest) -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(manifest.mtime.unsigned_abs())
}

fn switch(manifest: &Manifest, display: &Display, name: &str) -> Result<(), String> {
    if name == DEFAULT_LAYOUT {
        return display.restore_layout();
    }
    let layout = manifest
        .layouts
        .get(name)
        .ok_or_else(|| format!("no layout {name:?} in the manifest"))?;
    display.switch_layout(layout)
}

struct Context<'a> {
    scenario: &'a Scenario,
    initial_layout: &'a str,
    variables: &'a [(String, String)],
    manifest: &'a Manifest,
    display: &'a Display,
    fixture: &'a Path,
}

fn drive(application: &mut Application, context: &Context, layout: &mut String) -> Outcome {
    let mut outcome = Outcome::default();
    let hello = application.receive(LAUNCH_TIMEOUT);
    if let Err(error) = record(&mut outcome, "launch", &json!({ "launch": true }), hello) {
        outcome.errors.push(format!("launch: {error}"));
        return outcome;
    }
    // Only now: the process read the US keymap at startup, and key names
    // keep meaning US key positions under any layout.
    if context.initial_layout != DEFAULT_LAYOUT {
        match switch(context.manifest, context.display, context.initial_layout) {
            Ok(()) => context.initial_layout.clone_into(layout),
            Err(error) => {
                outcome.errors.push(format!("layout: {error}"));
                return outcome;
            }
        }
    }
    for (index, step) in context.scenario.steps.iter().enumerate() {
        let step = substitute(step, context.variables);
        let label = format!("step {} {step}", index + 1);
        let object = step.as_object().cloned().unwrap_or_default();
        if let Some(name) = object.get("layout") {
            let name = name.as_str().unwrap_or_default().to_owned();
            match switch(context.manifest, context.display, &name) {
                Ok(()) => *layout = name,
                Err(error) => {
                    outcome.errors.push(format!("{label}: {error}"));
                    return outcome;
                }
            }
            continue;
        }
        let result = if object.contains_key("fs") {
            wait_for_fs(&step, context.fixture)
        } else {
            let timeout = match (object.get("wait"), object.get("timeout_ms")) {
                (Some(_), Some(Value::Number(ms))) => {
                    Duration::from_millis(ms.as_u64().unwrap_or_default()) + WAIT_MARGIN
                }
                (Some(_), _) => DEFAULT_WAIT + WAIT_MARGIN,
                _ => COMMAND_TIMEOUT,
            };
            let reply = application.send(&step, timeout);
            record(&mut outcome, &label, &step, reply)
        };
        if let Err(error) = result {
            let failure = format!("{label}: {error}");
            if error.contains(CONTROLLER_PANIC) {
                outcome.errors.push(failure);
            } else {
                outcome.failures.push(failure);
            }
            return outcome;
        }
    }
    outcome
}

/// Logs the exchange, keeps expect mismatches and criticals as failures, and
/// stops the run on anything else that went wrong.
fn record(
    outcome: &mut Outcome,
    label: &str,
    step: &Value,
    reply: Result<Value, String>,
) -> Result<(), String> {
    let reply = reply?;
    let _ = writeln!(
        outcome.transcript,
        "{}",
        json!({ "command": step, "reply": reply })
    );
    if let Some(tree) = reply["result"]["tree"].as_str() {
        outcome.last_tree = Some(tree.to_owned());
    }
    for critical in reply["criticals"].as_array().into_iter().flatten() {
        outcome.failures.push(format!(
            "{label}: GLib warning or worse: {}",
            critical.as_str().unwrap_or_default()
        ));
    }
    if reply["ok"].as_bool() == Some(true) {
        return Ok(());
    }
    let failures = reply["failures"].as_array().cloned().unwrap_or_default();
    let explanation = failures
        .iter()
        .map(|failure| {
            format!(
                "{} expected {}, actual {}",
                failure["message"].as_str().unwrap_or("predicate"),
                failure["predicate"],
                failure["actual"]
            )
        })
        .collect::<Vec<_>>()
        .join("; ");
    match (reply["error"].as_str(), step.get("expect")) {
        (None, Some(_)) => {
            outcome.failures.push(format!("{label}: {explanation}"));
            Ok(())
        }
        (error, _) => Err(format!(
            "{}{}{explanation}",
            error.unwrap_or_default(),
            if explanation.is_empty() { "" } else { ": " }
        )),
    }
}

fn wait_for_fs(step: &Value, fixture: &Path) -> Result<(), String> {
    let step: FsStep = serde_json::from_value(step.clone())
        .map_err(|error| format!("invalid fs step: {error}"))?;
    let deadline = Instant::now() + step.timeout_ms.map_or(DEFAULT_WAIT, Duration::from_millis);
    loop {
        match step.fs.observe(fixture)? {
            Ok(()) => return Ok(()),
            Err(actual) if Instant::now() >= deadline => {
                return Err(format!("timed out; {} {actual}", step.fs.path));
            }
            Err(_) => std::thread::sleep(POLL_INTERVAL),
        }
    }
}

// ------------------------------------------------------- scenario checks

#[test]
fn every_scenario_file_is_registered_and_well_formed() {
    let directory = Path::new(REPOSITORY).join("tests/qa/scenarios");
    let mut files: Vec<String> = fs::read_dir(&directory)
        .expect("the scenario directory should be listable")
        .map(|entry| entry.expect("the scenario entry should be readable").path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "jsonl")
        })
        .map(|path| {
            load_scenario(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
            let stem = path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .unwrap_or_default();
            stem.replace('-', "_")
        })
        .collect();
    files.sort();
    let mut registered: Vec<String> = REGISTERED.iter().map(|name| (*name).to_owned()).collect();
    registered.sort();
    assert_eq!(
        files, registered,
        "register each scenario file in scenarios! {{}}"
    );
    let manifest = manifest();
    for name in REGISTERED {
        let scenario = load_scenario(&scenario_path(name)).expect("the scenario should load");
        let header = &scenario.header;
        assert!(
            Path::new(REPOSITORY)
                .join(header.twin.split("::").next().unwrap_or_default())
                .is_file(),
            "{name}: the twin {} should exist",
            header.twin
        );
        if let Some(mutation) = &header.mutation {
            assert!(
                Path::new(REPOSITORY)
                    .join(format!("tests/e2e/mutations/{mutation}.patch"))
                    .is_file(),
                "{name}: the mutation {mutation} should exist"
            );
        }
        for run in runs(header) {
            let variables = variables(&run, "root");
            let mut layouts = vec![
                run.variant
                    .layout
                    .clone()
                    .unwrap_or_else(|| header.layout.clone()),
            ];
            layouts.extend(scenario.steps.iter().filter_map(|step| {
                substitute(step, &variables)["layout"]
                    .as_str()
                    .map(str::to_owned)
            }));
            for layout in layouts {
                assert!(
                    manifest.layouts.contains_key(&layout),
                    "{name}/{}: no layout {layout:?}",
                    run.label
                );
            }
        }
    }
}
