use std::collections::HashMap;
use std::net::{SocketAddr, TcpStream};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc, Arc, Mutex,
};
use std::thread::spawn;
use std::time::Duration;
use tracing::{info, warn};

use websocket::sender::Writer;
use websocket::sync::Server;
use websocket::OwnedMessage;

use crate::input::mouse_device::Mouse;
#[cfg(target_os = "linux")]
use crate::input::uinput_device::GraphicTablet;
use crate::stream_handler::{PointerStreamHandler, ScreenStreamHandler, StreamHandler};

use crate::screen_capture::generic::ScreenCaptureGeneric;

#[cfg(target_os = "linux")]
use crate::screen_capture::linux::ScreenCaptureX11;
#[cfg(target_os = "linux")]
use crate::x11helper::WindowInfo;

pub enum Ws2GuiMessage {
    Error(String),
    Warning(String),
    Info(String),
}
pub enum Gui2WsMessage {
    Shutdown,
}

pub fn run(
    sender: mpsc::Sender<Ws2GuiMessage>,
    receiver: mpsc::Receiver<Gui2WsMessage>,
    ws_pointer_socket_addr: SocketAddr,
    ws_video_socket_addr: SocketAddr,
    password: Option<&str>,
    screen_update_interval: Duration,
    stylus_support: bool,
    faster_capture: bool,
    capture_window: WindowInfo,
) {
    let clients = Arc::new(Mutex::new(HashMap::<
        SocketAddr,
        Arc<Mutex<Writer<TcpStream>>>,
    >::new()));
    let clients2 = clients.clone();
    let clients3 = clients.clone();
    let shutdown = Arc::new(AtomicBool::new(false));
    let shutdown2 = shutdown.clone();
    let shutdown3 = shutdown.clone();
    let sender2 = sender.clone();
    let sender3 = sender.clone();

    spawn(move || loop {
        match receiver.recv() {
            Err(_) | Ok(Gui2WsMessage::Shutdown) => {
                let clients = clients.lock().unwrap();
                for client in clients.values() {
                    let client = client.lock().unwrap();
                    if let Err(err) = client.shutdown_all() {
                        sender.send(Ws2GuiMessage::Error(format!(
                            "Could not shutdown websocket: {}",
                            err
                        )));
                    }
                }
                shutdown.store(true, Ordering::Relaxed);
                return;
            }
        }
    });
    let pass: Option<String> = password.map_or(None, |s| Some(s.to_string()));
    #[cfg(target_os = "linux")]
    {
        let capture_window = capture_window.clone();
        if stylus_support {
            spawn(move || {
                listen_websocket(
                    ws_pointer_socket_addr,
                    pass,
                    clients2,
                    shutdown2,
                    sender2,
                    move || create_graphic_tablet_stream_handler(capture_window),
                )
            });
        } else {
            spawn(move || {
                listen_websocket(
                    ws_pointer_socket_addr,
                    pass,
                    clients2,
                    shutdown2,
                    sender2,
                    move || create_mouse_stream_handler(capture_window),
                )
            });
        }
    }

    #[cfg(not(target_os = "linux"))]
    spawn(move || {
        listen_websocket(
            ws_pointer_socket_addr,
            pass,
            clients2,
            shutdown2,
            sender2,
            create_mouse_stream_handler,
        )
    });

    let pass: Option<String> = password.map_or(None, |s| Some(s.to_string()));
    #[cfg(target_os = "linux")]
    {
        if faster_capture {
            spawn(move || {
                listen_websocket(
                    ws_video_socket_addr,
                    pass,
                    clients3,
                    shutdown3,
                    sender3,
                    move || create_xscreen_stream_handler(capture_window, screen_update_interval),
                )
            });
        } else {
            spawn(move || {
                listen_websocket(
                    ws_video_socket_addr,
                    pass,
                    clients3,
                    shutdown3,
                    sender3,
                    move || create_screen_stream_handler(screen_update_interval),
                )
            });
        }
    }

    #[cfg(not(target_os = "linux"))]
    spawn(move || {
        listen_websocket(
            ws_video_socket_addr,
            pass,
            clients3,
            shutdown3,
            sender3,
            move || create_screen_stream_handler(screen_update_interval),
        )
    });
}

#[cfg(target_os = "linux")]
fn create_graphic_tablet_stream_handler(winfo: WindowInfo
) -> Result<PointerStreamHandler<GraphicTablet>, Box<dyn std::error::Error>> {
    Ok(PointerStreamHandler::new(GraphicTablet::new(winfo)?))
}

fn create_mouse_stream_handler(winfo: WindowInfo) -> Result<PointerStreamHandler<Mouse>, Box<dyn std::error::Error>>
{
    Ok(PointerStreamHandler::new(Mouse::new(winfo)))
}

#[cfg(target_os = "linux")]
fn create_xscreen_stream_handler(
    capture_window: WindowInfo,
    update_interval: Duration,
) -> Result<ScreenStreamHandler<ScreenCaptureX11>, Box<dyn std::error::Error>> {
    Ok(ScreenStreamHandler::new(
        ScreenCaptureX11::new(capture_window)?,
        update_interval,
    ))
}

fn create_screen_stream_handler(
    update_interval: Duration,
) -> Result<ScreenStreamHandler<ScreenCaptureGeneric>, Box<dyn std::error::Error>> {
    Ok(ScreenStreamHandler::new(
        ScreenCaptureGeneric::new(),
        update_interval,
    ))
}

fn listen_websocket<T, F>(
    addr: SocketAddr,
    password: Option<String>,
    clients: Arc<Mutex<HashMap<SocketAddr, Arc<Mutex<Writer<TcpStream>>>>>>,
    shutdown: Arc<AtomicBool>,
    sender: mpsc::Sender<Ws2GuiMessage>,
    create_stream_handler: F,
) where
    T: StreamHandler,
    F: Fn() -> Result<T, Box<dyn std::error::Error>> + Send + 'static + Clone,
{
    let server = Server::bind(addr);
    if let Err(err) = server {
        sender.send(Ws2GuiMessage::Error(format!(
            "Failed binding to socket: {}",
            err
        )));
        return;
    }
    let mut server = server.unwrap();
    if let Err(err) = server.set_nonblocking(true) {
        sender.send(Ws2GuiMessage::Warning(format!(
            "Could not set websocket to non-blocking, graceful shutdown may be impossible now: {}",
            err
        )));
    }

    loop {
        std::thread::sleep(std::time::Duration::from_millis(10));
        let sender = sender.clone();
        if shutdown.load(Ordering::Relaxed) {
            sender.send(Ws2GuiMessage::Info(format!(
                "Shutting down websocket: {}",
                addr
            )));
            return;
        }
        let clients = clients.clone();
        let password = password.clone();
        let create_stream_handler = create_stream_handler.clone();
        match server.accept() {
            Ok(request) => {
                spawn(move || {
                    let client = request.accept();
                    if let Err((_, err)) = client {
                        sender.send(Ws2GuiMessage::Warning(format!(
                            "Failed to accept client: {}",
                            err
                        )));
                        return;
                    }
                    let client = client.unwrap();
                    let peer_addr = client.peer_addr();
                    if let Err(err) = peer_addr {
                        sender.send(Ws2GuiMessage::Warning(format!(
                            "Failed to retrieve client address: {}",
                            err
                        )));
                        return;
                    }
                    let peer_addr = peer_addr.unwrap();
                    let client = client.split();
                    if let Err(err) = client {
                        sender.send(Ws2GuiMessage::Warning(format!(
                            "Failed to setup connection: {}",
                            err
                        )));
                        return;
                    }
                    let (mut ws_receiver, ws_sender) = client.unwrap();

                    let ws_sender = Arc::new(Mutex::new(ws_sender));

                    {
                        let mut clients = clients.lock().unwrap();
                        clients.insert(peer_addr, ws_sender.clone());
                    }

                    let stream_handler = create_stream_handler();
                    if let Err(err) = stream_handler {
                        sender.send(Ws2GuiMessage::Error(format!(
                            "Failed to create stream handler: {}",
                            err
                        )));
                        return;
                    }

                    let mut authed = password.is_none();
                    let password = password.unwrap_or("".into());
                    let mut stream_handler = stream_handler.unwrap();
                    for msg in ws_receiver.incoming_messages() {
                        match msg {
                            Ok(msg) => {
                                if !authed {
                                    if let OwnedMessage::Text(pw) = &msg {
                                        if pw == &password {
                                            authed = true;
                                        } else {
                                            return;
                                        }
                                    }
                                } else {
                                    stream_handler.process(ws_sender.clone(), &msg);
                                }
                                if msg.is_close() {
                                    return;
                                }
                            }
                            Err(err) => {
                                warn!("Error reading message from websocket, closing ({})", err);
                                return;
                            }
                        }
                    }
                });
            }
            Err(_) => {
                if shutdown.load(Ordering::Relaxed) {
                    return;
                }
            }
        };
    }
}