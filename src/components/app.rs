use cosmic::iced_native::window::Id as SurfaceId;
use iced::{Application, Command, Element, Subscription};
use iced_sctk::{
    application::SurfaceIdWrapper,
    command::platform_specific::wayland::layer_surface::SctkLayerSurfaceSettings,
    commands::layer_surface::destroy_layer_surface, settings::InitialSurface,
};
use sctk::shell::layer::{KeyboardInteractivity, Layer};
use std::collections::BTreeMap;

use crate::{
    components::polkit_dialog,
    subscriptions::{dbus, polkit_agent, settings_daemon},
};

#[derive(Debug)]
pub enum Msg {
    DBus(dbus::Event),
    PolkitAgent(polkit_agent::Event),
    PolkitDialog((SurfaceId, polkit_dialog::Msg)),
    SettingsDaemon(settings_daemon::Event),
    Closed(SurfaceIdWrapper),
}

enum Surface {
    PolkitDialog(polkit_dialog::State),
}

#[derive(Default)]
struct App {
    connection: Option<zbus::Connection>,
    system_connection: Option<zbus::Connection>,
    surfaces: BTreeMap<SurfaceId, Surface>,
}

impl App {
    // Get lowest unused ID
    // XXX way hashing is used in iced here may not be ideal
    fn next_surface_id(&self) -> SurfaceId {
        let mut id = 1;
        for i in self.surfaces.keys() {
            if *i == SurfaceId::new(id) {
                id += 1;
            } else {
                break;
            }
        }
        SurfaceId::new(id)
    }
}

impl Application for App {
    type Message = Msg;
    type Theme = cosmic::Theme;
    type Executor = iced::executor::Default;
    type Flags = ();

    fn new(_flags: ()) -> (Self, Command<Msg>) {
        (Self::default(), destroy_layer_surface(SurfaceId::new(0)))
    }

    fn title(&self) -> String {
        String::from("cosmic-osd")
    }

    fn update(&mut self, message: Msg) -> Command<Msg> {
        match message {
            Msg::DBus(event) => {
                match event {
                    dbus::Event::Connection(connection) => self.connection = Some(connection),
                    dbus::Event::SystemConnection(connection) => {
                        self.system_connection = Some(connection)
                    }
                    dbus::Event::Error(context, err) => {
                        eprintln!("Failed to {}: {}", context, err);
                    }
                }
                iced::Command::none()
            }
            Msg::PolkitAgent(event) => match event {
                polkit_agent::Event::CreateDialog(params) => {
                    println!("create: {}", params.cookie);
                    // TODO open surface
                    let id = self.next_surface_id();
                    let (state, cmd) = polkit_dialog::State::new(id, params);
                    self.surfaces
                        .insert(id.clone(), Surface::PolkitDialog(state));
                    cmd
                }
                polkit_agent::Event::CancelDialog { cookie } => {
                    println!("cancel: {}", cookie);
                    if let Some((id, _)) = self.surfaces.iter().find(|(_id, surface)| {
                        if let Surface::PolkitDialog(state) = surface {
                            state.params.cookie == cookie
                        } else {
                            false
                        }
                    }) {
                        let id = *id;
                        if let Surface::PolkitDialog(state) = self.surfaces.remove(&id).unwrap() {
                            state.cancel()
                        } else {
                            unreachable!()
                        }
                    } else {
                        Command::none()
                    }
                }
            },
            Msg::PolkitDialog((id, msg)) => {
                if let Some(surface) = self.surfaces.remove(&id) {
                    if let Surface::PolkitDialog(state) = surface {
                        let (state, cmd) = state.update(msg);
                        if let Some(state) = state {
                            self.surfaces.insert(id, Surface::PolkitDialog(state));
                        }
                        return cmd.map(move |msg| Msg::PolkitDialog((id, msg)));
                    }
                }
                Command::none()
            }
            Msg::SettingsDaemon(event) => {
                println!("{:?}", event);
                Command::none()
            }
            Msg::Closed(surface) => Command::none(),
        }
    }

    fn subscription(&self) -> Subscription<Msg> {
        let mut subscriptions = Vec::new();

        subscriptions.push(dbus::subscription().map(Msg::DBus));

        if let Some(connection) = self.system_connection.clone() {
            subscriptions.push(polkit_agent::subscription(connection).map(Msg::PolkitAgent));
        }

        if let Some(connection) = self.connection.clone() {
            subscriptions.push(settings_daemon::subscription(connection).map(Msg::SettingsDaemon));
        }

        subscriptions.extend(self.surfaces.iter().map(|(id, surface)| match surface {
            Surface::PolkitDialog(state) => state.subscription().with(*id).map(Msg::PolkitDialog),
        }));

        iced::Subscription::batch(subscriptions)
    }

    fn view(&self, surface: SurfaceIdWrapper) -> Element<'_, Msg, iced::Renderer<Self::Theme>> {
        if let SurfaceIdWrapper::LayerSurface(id) = surface {
            if let Some(surface) = self.surfaces.get(&id) {
                println!("FOO");
                return match surface {
                    Surface::PolkitDialog(state) => {
                        state.view().map(move |msg| Msg::PolkitDialog((id, msg)))
                    }
                };
            }
        }
        iced::widget::text("").into() // XXX
    }

    // TODO: Should be Option<Msg>?
    fn close_requested(&self, surface: SurfaceIdWrapper) -> Msg {
        Msg::Closed(surface)
    }
}

pub fn main() -> iced::Result {
    App::run(iced::Settings {
        antialiasing: true,
        exit_on_close_request: false,
        // XXX no initial surface?
        initial_surface: InitialSurface::LayerSurface(SctkLayerSurfaceSettings {
            keyboard_interactivity: KeyboardInteractivity::None,
            namespace: "ignore".into(),
            size: (Some(1), Some(1)),
            layer: Layer::Background,
            ..Default::default()
        }),
        ..Default::default()
    })
}