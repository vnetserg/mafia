use crate::socket_service::{ SocketId, SocketEvent, SocketProxy };
use crate::locale::Locale;

use futures::{
    prelude::*,
    channel::mpsc::{UnboundedSender, UnboundedReceiver, unbounded}
};

use std::{
    sync::Arc,
    collections::HashMap,
};

pub type UserId = SocketId;

#[derive(Clone)]
pub struct User {
    id: UserId,
    login: Box<str>,
    socket: SocketProxy,
}

pub enum UserEvent {
    NewUser(User),
    NewMessage(UserId, Box<str>),
    DropUser(UserId),
}

pub struct LoginService {
    event_handler: UnboundedSender<UserEvent>,
    socket_sender: UnboundedSender<SocketEvent>,
    socket_receiver: UnboundedReceiver<SocketEvent>,
    auth_state: HashMap<SocketId, AuthState>,
    login_state: HashMap<Box<str>, LoginState>,
    locale: Locale,
}

enum AuthState {
    Initial(SocketProxy),
    GotLogin(SocketProxy, Box<str>),
    Ok(User),
}

enum LoginState {
    Online(Box<str>),
    Offline(Box<str>),
}

impl LoginService {
    pub fn new(event_handler: UnboundedSender<UserEvent>, locale: Locale) -> Self {
        let (socket_sender, socket_receiver) = unbounded();
        LoginService {
            event_handler,
            socket_sender,
            socket_receiver,
            locale,
            auth_state: HashMap::new(),
            login_state: HashMap::new(),
        }
    }

    pub fn make_socket_handler(&self) -> UnboundedSender<SocketEvent> {
        self.socket_sender.clone()
    }

    pub async fn run(&mut self) {
        loop {
            match self.socket_receiver.next().await {
                Some(SocketEvent::NewSocket(proxy)) => self.handle_new_socket(proxy),
                Some(SocketEvent::NewMessage(id, data)) => self.handle_new_message(id, data),
                Some(SocketEvent::ClosedSocket(id)) => self.handle_closed_socket(id),
                None => panic!("LoginService socket_receiver terminated"),
            }
        }
    }

    fn handle_new_socket(&mut self, proxy: SocketProxy) {
        proxy.send_static("Welcome to the Mafia server!\nPlease enter your nickname: ");
        self.auth_state.insert(proxy.get_id(), AuthState::Initial(proxy));
    }

    fn handle_new_message(&mut self, id: SocketId, data: Box<str>) {
        let state = self.auth_state.remove(&id);
        let new_state = match state {
            Some(AuthState::Initial(proxy)) => {
                let login = data;
                match self.login_state.get(&login) {
                    Some(LoginState::Online(_)) => {
                        proxy.send(format!("Player \"{}\" is already online.\n\
                                            Please enter your nickname: ", login));
                        AuthState::Initial(proxy)
                    },
                    Some(LoginState::Offline(_)) => {
                        proxy.send(format!("Password for \"{}\": ", login));
                        AuthState::GotLogin(proxy, login)
                    },
                    None => {
                        proxy.send(format!("Creating player \"{}\". Enter password: ", login));
                        AuthState::GotLogin(proxy, login)
                    }
                }
            },
            Some(AuthState::GotLogin(proxy, login)) => {
                let password = data;
                let login_state = self.login_state.remove(&login);
                let (new_login_state, new_auth_state) = match login_state {
                    Some(LoginState::Online(password)) => {
                        proxy.send(format!("Player \"{}\" is already online.\n\
                                            Please enter your nickname: ", login));
                        (LoginState::Online(password), AuthState::Initial(proxy))
                    },
                    Some(LoginState::Offline(real_password)) => {
                        if password == real_password {
                            proxy.send(format!("Welcome back, {}!\n", login));
                            let user = User {
                                id: proxy.get_id(),
                                login: login.clone(),
                                socket: proxy,
                            };
                            self.event_handler.unbounded_send(UserEvent::NewUser(user.clone()))
                                .expect("LoginService event_handler stream error");
                            (LoginState::Online(real_password), AuthState::Ok(user))
                        } else {
                            proxy.send_static("Incorrect password.\nPlease enter your nickname: ");
                            (LoginState::Offline(real_password), AuthState::Initial(proxy))
                        }
                    },
                    None => {
                        proxy.send(format!("Password created. Welcome, {}!\n", login));
                        let user = User {
                            id: proxy.get_id(),
                            login: login.clone(),
                            socket: proxy,
                        };
                        self.event_handler.unbounded_send(UserEvent::NewUser(user.clone()))
                            .expect("LoginService event_handler stream error");
                        (LoginState::Online(password), AuthState::Ok(user))
                    }
                };
                self.login_state.insert(login, new_login_state);
                new_auth_state
            },
            Some(AuthState::Ok(user)) => {
                self.event_handler.unbounded_send(UserEvent::NewMessage(user.id, data))
                    .expect("LoginService event_handler stream error");
                AuthState::Ok(user)
            },
            None => return,
        };
        self.auth_state.insert(id, new_state);
    }

    fn handle_closed_socket(&mut self, id: SocketId) {
        if let Some(AuthState::Ok(user)) = self.auth_state.remove(&id) {
            if let Some(LoginState::Online(password)) = self.login_state.remove(&user.login) {
                self.login_state.insert(user.login, LoginState::Offline(password));
                self.event_handler.unbounded_send(UserEvent::DropUser(user.id))
                    .expect("LoginService event_handler stream error");
            } else {
                panic!("LoginService user is authenticated, but not online");
            }
        }
    }
}

impl User {
    pub fn get_id(&self) -> UserId {
        self.id
    }

    pub fn get_login(&self) -> &str {
        &self.login
    }

    pub fn send(&self, message: String) {
        self.socket.send(message)
    }

    pub fn send_boxed(&self, message: Box<str>) {
        self.socket.send_boxed(message)
    }

    pub fn send_arc(&self, message: Arc<str>) {
        self.socket.send_arc(message)
    }

    pub fn send_static(&self, message: &'static str) {
        self.socket.send_static(message)
    }

    pub fn drop(&self) {
        self.socket.close()
    }
}
