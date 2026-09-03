use std::{
    io::{Read, Write},
    sync::{Mutex, MutexGuard},
    thread,
    time::{Duration, Instant},
};

use serde::{Deserialize, Serialize};
use serialport::{ClearBuffer, SerialPort};
use tauri::State;
use tracing::{error, info};

const BAUD_RATE: u32 = 460_800;
const STICK_CENTER: u16 = 0x800;

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ManualButton {
    A,
    B,
    X,
    Y,
    Up,
    Down,
    Left,
    Right,
    L,
    R,
    Zl,
    Zr,
    Plus,
    Minus,
    LStick,
    RStick,
    Home,
    Capture,
}

impl ManualButton {
    fn mask(self) -> [u8; 3] {
        match self {
            Self::A => [0x08, 0, 0],
            Self::B => [0x04, 0, 0],
            Self::X => [0x02, 0, 0],
            Self::Y => [0x01, 0, 0],
            Self::R => [0x40, 0, 0],
            Self::Zr => [0x80, 0, 0],
            Self::Minus => [0, 0x01, 0],
            Self::Plus => [0, 0x02, 0],
            Self::RStick => [0, 0x04, 0],
            Self::LStick => [0, 0x08, 0],
            Self::Home => [0, 0x10, 0],
            Self::Capture => [0, 0x20, 0],
            Self::Down => [0, 0, 0x01],
            Self::Up => [0, 0, 0x02],
            Self::Right => [0, 0, 0x04],
            Self::Left => [0, 0, 0x08],
            Self::L => [0, 0, 0x40],
            Self::Zl => [0, 0, 0x80],
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ManualStick {
    Left,
    Right,
}

#[derive(Clone, Copy, Debug, Default, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum ConnectionState {
    #[default]
    Disconnected,
    Connected,
    Error,
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ManualControllerStatus {
    state: ConnectionState,
    port: Option<String>,
    available_ports: Vec<String>,
    error: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct InputState {
    buttons: [u8; 3],
    left: (u16, u16),
    right: (u16, u16),
}

impl Default for InputState {
    fn default() -> Self {
        Self {
            buttons: [0; 3],
            left: (STICK_CENTER, STICK_CENTER),
            right: (STICK_CENTER, STICK_CENTER),
        }
    }
}

impl InputState {
    fn set_button(&mut self, button: ManualButton, pressed: bool) {
        let mask = button.mask();
        for (current, bit) in self.buttons.iter_mut().zip(mask) {
            if pressed {
                *current |= bit;
            } else {
                *current &= !bit;
            }
        }
    }

    fn encoded(self) -> String {
        let left = pack_stick(self.left.0, self.left.1);
        let right = pack_stick(self.right.0, self.right.1);
        format!(
            "{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}",
            self.buttons[0],
            self.buttons[1],
            self.buttons[2],
            left[0],
            left[1],
            left[2],
            right[0],
            right[1],
            right[2]
        )
    }
}

fn pack_stick(x: u16, y: u16) -> [u8; 3] {
    [
        x as u8,
        ((x >> 8) as u8) | (((y & 0x0f) << 4) as u8),
        (y >> 4) as u8,
    ]
}

struct ControllerConnection {
    port: Box<dyn SerialPort>,
}

impl ControllerConnection {
    fn connect(port_name: &str) -> Result<Self, String> {
        let mut port = serialport::new(port_name, BAUD_RATE)
            .timeout(Duration::from_millis(50))
            .open()
            .map_err(|error| format!("Failed to open {port_name}: {error}"))?;
        port.write_data_terminal_ready(true)
            .map_err(|error| format!("Failed to enable DTR on {port_name}: {error}"))?;
        thread::sleep(Duration::from_millis(200));
        port.clear(ClearBuffer::Input)
            .map_err(|error| format!("Failed to clear {port_name} input: {error}"))?;

        let mut connection = Self { port };
        let identity = connection.query("+ID ", Duration::from_millis(500))?;
        if !identity.lines().any(|line| line.trim() == "+2wiCC") {
            return Err(format!("Controller identity check failed: {identity:?}"));
        }
        let usb = connection.query("+GCS ", Duration::from_millis(500))?;
        if !usb.lines().any(|line| line.trim() == "+GCS 1") {
            return Err(format!("Controller USB is not connected: {usb:?}"));
        }
        connection.write_line("+SPM RT")?;
        connection.write_state(InputState::default())?;
        Ok(connection)
    }

    fn write_state(&mut self, state: InputState) -> Result<(), String> {
        self.write_line(&format!("+QF {}", state.encoded()))
    }

    fn write_line(&mut self, command: &str) -> Result<(), String> {
        self.port
            .write_all(format!("{command}\n").as_bytes())
            .and_then(|()| self.port.flush())
            .map_err(|error| format!("Controller write failed: {error}"))
    }

    fn query(&mut self, command: &str, duration: Duration) -> Result<String, String> {
        self.write_line(command)?;
        let deadline = Instant::now() + duration;
        let mut reply = Vec::new();
        let mut buffer = [0_u8; 256];
        while Instant::now() < deadline {
            match self.port.read(&mut buffer) {
                Ok(count) => reply.extend_from_slice(&buffer[..count]),
                Err(error) if error.kind() == std::io::ErrorKind::TimedOut => {}
                Err(error) => return Err(format!("Controller read failed: {error}")),
            }
        }
        Ok(String::from_utf8_lossy(&reply).into_owned())
    }
}

#[derive(Default)]
struct ManualControllerInner {
    connection: Option<ControllerConnection>,
    port: Option<String>,
    input: InputState,
    error: Option<String>,
}

#[derive(Default)]
pub struct ManualControllerManager {
    inner: Mutex<ManualControllerInner>,
}

impl ManualControllerManager {
    fn status(&self) -> ManualControllerStatus {
        let inner = lock(&self.inner);
        ManualControllerStatus {
            state: if inner.connection.is_some() {
                ConnectionState::Connected
            } else if inner.error.is_some() {
                ConnectionState::Error
            } else {
                ConnectionState::Disconnected
            },
            port: inner.port.clone(),
            available_ports: available_ports(),
            error: inner.error.clone(),
        }
    }

    fn connect(&self, port: String) -> Result<ManualControllerStatus, String> {
        let port = port.trim().to_owned();
        if port.is_empty() {
            return Err("Select a controller COM port".to_owned());
        }
        let mut inner = lock(&self.inner);
        if let Some(connection) = inner.connection.as_mut() {
            connection.write_state(InputState::default())?;
        }
        inner.connection = None;
        inner.input = InputState::default();
        inner.error = None;

        match ControllerConnection::connect(&port) {
            Ok(connection) => {
                inner.connection = Some(connection);
                inner.port = Some(port.clone());
                info!(%port, "manual controller connected");
                drop(inner);
                Ok(self.status())
            }
            Err(message) => {
                inner.port = Some(port);
                inner.error = Some(message.clone());
                Err(message)
            }
        }
    }

    fn disconnect(&self) -> Result<ManualControllerStatus, String> {
        let mut inner = lock(&self.inner);
        let result = if let Some(connection) = inner.connection.as_mut() {
            connection.write_state(InputState::default())
        } else {
            Ok(())
        };
        inner.connection = None;
        inner.input = InputState::default();
        inner.error = result.as_ref().err().cloned();
        drop(inner);
        result.map(|()| self.status())
    }

    fn update(&self, change: impl FnOnce(&mut InputState)) -> Result<(), String> {
        let mut inner = lock(&self.inner);
        if inner.connection.is_none() {
            return Err("Manual controller is not connected".to_owned());
        }
        let previous = inner.input;
        change(&mut inner.input);
        let input = inner.input;
        let result = inner
            .connection
            .as_mut()
            .expect("checked above")
            .write_state(input);
        if let Err(message) = result {
            inner.input = previous;
            inner.error = Some(message.clone());
            inner.connection = None;
            error!(%message, "manual controller write failed");
            return Err(message);
        }
        inner.error = None;
        Ok(())
    }

    fn neutralize(&self) -> Result<(), String> {
        let mut inner = lock(&self.inner);
        inner.input = InputState::default();
        let input = inner.input;
        if let Some(connection) = inner.connection.as_mut() {
            if let Err(message) = connection.write_state(input) {
                inner.error = Some(message.clone());
                inner.connection = None;
                return Err(message);
            }
        }
        inner.error = None;
        Ok(())
    }
}

impl Drop for ManualControllerManager {
    fn drop(&mut self) {
        if let Ok(inner) = self.inner.get_mut() {
            if let Some(connection) = inner.connection.as_mut() {
                let _ = connection.write_state(InputState::default());
            }
        }
    }
}

fn available_ports() -> Vec<String> {
    let mut ports: Vec<String> = serialport::available_ports()
        .map(|ports| ports.into_iter().map(|port| port.port_name).collect())
        .unwrap_or_default();
    ports.sort();
    ports
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[tauri::command]
pub fn get_manual_controller_status(
    manager: State<'_, ManualControllerManager>,
) -> ManualControllerStatus {
    manager.status()
}

#[tauri::command(async)]
pub fn connect_manual_controller(
    port: String,
    manager: State<'_, ManualControllerManager>,
) -> Result<ManualControllerStatus, String> {
    manager.connect(port)
}

#[tauri::command(async)]
pub fn disconnect_manual_controller(
    manager: State<'_, ManualControllerManager>,
) -> Result<ManualControllerStatus, String> {
    manager.disconnect()
}

#[tauri::command(async)]
pub fn set_manual_controller_button(
    button: ManualButton,
    pressed: bool,
    manager: State<'_, ManualControllerManager>,
) -> Result<(), String> {
    manager.update(|input| input.set_button(button, pressed))
}

#[tauri::command(async)]
pub fn set_manual_controller_stick(
    stick: ManualStick,
    x: u16,
    y: u16,
    manager: State<'_, ManualControllerManager>,
) -> Result<(), String> {
    if x > 0x0fff || y > 0x0fff {
        return Err("Stick coordinates must be between 0 and 4095".to_owned());
    }
    manager.update(|input| match stick {
        ManualStick::Left => input.left = (x, y),
        ManualStick::Right => input.right = (x, y),
    })
}

#[tauri::command(async)]
pub fn neutralize_manual_controller(
    manager: State<'_, ManualControllerManager>,
) -> Result<(), String> {
    manager.neutralize()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn neutral_state_matches_2wicc_protocol() {
        assert_eq!(InputState::default().encoded(), "000000000880000880");
    }

    #[test]
    fn combines_buttons_and_sticks() {
        let mut state = InputState::default();
        state.set_button(ManualButton::A, true);
        state.set_button(ManualButton::Up, true);
        state.left = (0x0e00, 0x0800);
        assert_eq!(state.encoded(), "080002000E80000880");

        state.set_button(ManualButton::A, false);
        assert_eq!(state.encoded(), "000002000E80000880");
    }
}
