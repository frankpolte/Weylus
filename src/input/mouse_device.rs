use autopilot::mouse;
use autopilot::screen::size as screen_size;

use tracing::warn;

use crate::input::device::InputDevice;
use crate::protocol::Button;
use crate::protocol::PointerEvent;
use crate::protocol::PointerEventType;

#[cfg(target_os = "linux")]
use crate::x11helper::WindowInfo;

#[cfg(target_os = "linux")]
pub struct Mouse {
    winfo: WindowInfo,
}

#[cfg(not(target_os = "linux"))]
pub struct Mouse {}

#[cfg(target_os = "linux")]
impl Mouse {
    pub fn new(winfo: WindowInfo) -> Self {
        Self { winfo: winfo }
    }
}

#[cfg(not(target_os = "linux"))]
impl Mouse {
    pub fn new() -> Self {
        Self {}
    }
}

impl InputDevice for Mouse {
    fn send_event(&mut self, event: &PointerEvent) {
        if !event.is_primary {
            return;
        }
        #[cfg(target_os = "linux")]
        {
            if let Err(err) = self.winfo.activate() {
                warn!("Failed to activate window, sending no input ({})", err);
                return;
            }
            let geometry = self.winfo.geometry();
            if let Err(err) = geometry {
                warn!("Failed to get window geometry, sending no input ({})", err);
                return;
            }
            let geometry = geometry.unwrap();
            if let Err(err) = mouse::move_to(autopilot::geometry::Point::new(
                (event.x * geometry.width + geometry.x) * screen_size().width,
                (event.y * geometry.height + geometry.y) * screen_size().height,
            )) {
                warn!("Could not move mouse: {}", err);
            }
        }

        #[cfg(not(target_os = "linux"))]
        {
            if let Err(err) = mouse::move_to(autopilot::geometry::Point::new(
                event.x * screen_size().width,
                event.y * screen_size().height,
            )) {
                warn!("Could not move mouse: {}", err);
            }
        }
        match event.event_type {
            PointerEventType::DOWN => match event.button {
                Button::PRIMARY => mouse::toggle(mouse::Button::Left, true),
                Button::AUXILARY => mouse::toggle(mouse::Button::Middle, true),
                Button::SECONDARY => mouse::toggle(mouse::Button::Right, true),
                _ => (),
            },
            PointerEventType::UP => {
                mouse::toggle(mouse::Button::Left, false);
                mouse::toggle(mouse::Button::Middle, false);
                mouse::toggle(mouse::Button::Right, false);
            }
            _ => (),
        }
    }
}
