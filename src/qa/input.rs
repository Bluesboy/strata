// SPDX-License-Identifier: MIT

//! Real X input sent through XTEST from inside the process, so GTK routes it
//! exactly as it routes a person's input: focus walks, grabs, and gestures
//! included. The same extension backs `tests/e2e/harness/xtest.py`.

use std::collections::HashMap;

use gtk::{gdk, glib::translate::IntoGlib, prelude::*};
use x11rb::{
    connection::{Connection, RequestConnection},
    protocol::{
        xproto::{self, ConnectionExt as _},
        xtest,
    },
    rust_connection::RustConnection,
    wrapper::ConnectionExt as _,
};

use super::protocol::Modifier;

pub(crate) struct Injector {
    connection: RustConnection,
    root: xproto::Window,
    /// Keysym to keycode and whether the second (shifted) level produces it.
    keymap: HashMap<u32, (u8, bool)>,
}

pub(crate) enum Event {
    Key { keycode: u8, pressed: bool },
    Button { number: u8, pressed: bool },
    Motion { x: i16, y: i16 },
}

impl Injector {
    /// Connects to `DISPLAY`, the display GTK is using.
    pub(crate) fn connect() -> Result<Self, String> {
        let (connection, screen) =
            x11rb::connect(None).map_err(|error| format!("cannot open the X display: {error}"))?;
        if connection
            .extension_information(xtest::X11_EXTENSION_NAME)
            .map_err(|error| error.to_string())?
            .is_none()
        {
            return Err("the X server has no XTEST extension".to_owned());
        }
        let root = connection.setup().roots[screen].root;
        // Without this, synthetic events are discarded while the application
        // holds a pointer grab, which is the state a drag runs in.
        xtest::grab_control(&connection, true).map_err(|error| error.to_string())?;
        let keymap = read_keymap(&connection)?;
        Ok(Self {
            connection,
            root,
            keymap,
        })
    }

    /// The keycode for a keysym and whether Shift must be held for it.
    pub(crate) fn keycode(&self, keysym: u32) -> Result<(u8, bool), String> {
        self.keymap
            .get(&keysym)
            .copied()
            .ok_or_else(|| format!("keysym {keysym:#06x} is not on the keyboard layout"))
    }

    pub(crate) fn send(&self, event: Event) -> Result<(), String> {
        let (kind, detail, x, y) = match event {
            Event::Key { keycode, pressed } => (
                if pressed {
                    xproto::KEY_PRESS_EVENT
                } else {
                    xproto::KEY_RELEASE_EVENT
                },
                keycode,
                0,
                0,
            ),
            Event::Button { number, pressed } => (
                if pressed {
                    xproto::BUTTON_PRESS_EVENT
                } else {
                    xproto::BUTTON_RELEASE_EVENT
                },
                number,
                0,
                0,
            ),
            Event::Motion { x, y } => (xproto::MOTION_NOTIFY_EVENT, 0, x, y),
        };
        xtest::fake_input(
            &self.connection,
            kind,
            detail,
            x11rb::CURRENT_TIME,
            self.root,
            x,
            y,
            0,
        )
        .map_err(|error| error.to_string())?;
        // The round trip guarantees the server has generated the events for
        // GTK's connection before the main loop is drained.
        self.connection.sync().map_err(|error| error.to_string())
    }

    pub(crate) fn pointer_position(&self) -> Result<(i16, i16), String> {
        let reply = self
            .connection
            .query_pointer(self.root)
            .map_err(|error| error.to_string())?
            .reply()
            .map_err(|error| error.to_string())?;
        Ok((reply.root_x, reply.root_y))
    }

    /// Where a GDK surface's top-left corner is on the X screen.
    pub(crate) fn surface_origin(&self, surface: &gdk::Surface) -> Result<(i16, i16), String> {
        let surface = surface
            .downcast_ref::<gdk4_x11::X11Surface>()
            .ok_or("the window is not on an X11 surface")?;
        let reply = self
            .connection
            .translate_coordinates(surface.xid() as xproto::Window, self.root, 0, 0)
            .map_err(|error| error.to_string())?
            .reply()
            .map_err(|error| error.to_string())?;
        Ok((reply.dst_x, reply.dst_y))
    }
}

/// Read once at startup, before a scenario switches layouts, so key names
/// keep meaning US key positions: under Russian, `j` presses the key that types о.
fn read_keymap(connection: &RustConnection) -> Result<HashMap<u32, (u8, bool)>, String> {
    let setup = connection.setup();
    let (first, last) = (setup.min_keycode, setup.max_keycode);
    let mapping = connection
        .get_keyboard_mapping(first, last - first + 1)
        .map_err(|error| error.to_string())?
        .reply()
        .map_err(|error| error.to_string())?;
    let width = usize::from(mapping.keysyms_per_keycode);
    let mut keymap = HashMap::new();
    if width == 0 {
        return Err("the X server returned no keyboard mapping".to_owned());
    }
    for (index, keysyms) in mapping.keysyms.chunks(width).enumerate() {
        let Ok(offset) = u8::try_from(index) else {
            break;
        };
        for (level, keysym) in keysyms.iter().take(2).enumerate() {
            if *keysym != 0 {
                keymap
                    .entry(*keysym)
                    .or_insert((first.saturating_add(offset), level == 1));
            }
        }
    }
    Ok(keymap)
}

/// GDK keyvals are X keysyms. A single character names its own key: Latin-1
/// keysyms are their code points, the rest are X's Unicode keysyms.
pub(crate) fn keysym(name: &str) -> Result<u32, String> {
    let mut characters = name.chars();
    if let (Some(character), None) = (characters.next(), characters.next()) {
        let code = u32::from(character);
        return Ok(if code < 0x100 {
            code
        } else {
            0x0100_0000 | code
        });
    }
    gdk::Key::from_name(name)
        .map(IntoGlib::into_glib)
        .ok_or_else(|| format!("no keysym named {name:?}"))
}

pub(crate) fn modifier_keysym(modifier: Modifier) -> Result<u32, String> {
    keysym(modifier.keysym_name())
}
