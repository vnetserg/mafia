use crate::login_service::{User, UserId, UserEvent};
use crate::locale::{Locale, HELP_EN};

use futures::{
    prelude::*,
    select,
    channel::mpsc::{UnboundedSender, UnboundedReceiver, unbounded}
};

use chrono::prelude::*;

use std::{
    sync::Arc,
    collections::HashMap,
};

pub type PlayerId = UserId;

#[derive(Clone)]
pub struct Player {
    user: User,
    channel: UnboundedSender<ChatRequest>,
}

pub struct ChatService {
    event_handler: UnboundedSender<GameEvent>,
    user_sender: UnboundedSender<UserEvent>,
    user_receiver: UnboundedReceiver<UserEvent>,
    request_sender: UnboundedSender<ChatRequest>,
    request_receiver: UnboundedReceiver<ChatRequest>,
    users: HashMap<UserId, UserInfo>,
    login_id: HashMap<Box<str>, UserId>,
    locale: Locale,
}

pub enum GameEvent {
    Connected(Player),
    Disconnected(PlayerId),
    Action(PlayerId, Box<str>),
    CommandList(PlayerId),
    CommandObserve(PlayerId),
    CommandPlay(PlayerId),
    CommandPause(PlayerId),
    CommandStart(PlayerId),
}

struct UserInfo {
    user: User,
    mute: MuteLevel,
}

pub enum MuteLevel {
    AllowAll,
    DenyPublic(&'static str),
    DenyAll(&'static str),
}

enum ChatRequest {
    MutePlayer(PlayerId, MuteLevel),
}

enum Message<'a> {
    Public(&'a str),
    Private(&'a str, Box<[&'a str]>),
    Command(&'a str),
    Action(&'a str),
}

impl ChatService {
    pub fn new(event_handler: UnboundedSender<GameEvent>, locale: Locale) -> Self {
        let (user_sender, user_receiver) = unbounded();
        let (request_sender, request_receiver) = unbounded();
        ChatService {
            event_handler,
            user_sender,
            user_receiver,
            request_sender,
            request_receiver,
            locale,
            users: HashMap::new(),
            login_id: HashMap::new(),
        }
    }

    pub fn make_user_handler(&self) -> UnboundedSender<UserEvent> {
        self.user_sender.clone()
    }

    pub async fn run(&mut self) {
        loop {
            select! {
                user_event = self.user_receiver.next().fuse() =>
                    match user_event {
                        Some(UserEvent::NewUser(user)) => self.handle_new_user(user),
                        Some(UserEvent::NewMessage(id, data)) => self.handle_new_message(id, data),
                        Some(UserEvent::DropUser(id)) => self.handle_drop_user(id),
                        None => panic!("ChatService user_receiver terminated"),
                    },
                request = self.request_receiver.next().fuse() =>
                    match request {
                        Some(ChatRequest::MutePlayer(id, level)) => self.handle_mute_request(id, level),
                        None => panic!("ChatService request_receiver terminated"),
                    },
            }
        }
    }

    fn handle_new_user(&mut self, user: User) {
        self.broadcast(format!("{} Connected: {}\n",
                               Local::now().format("%H:%M"),
                               user.get_login()).into());
        // Send event
        let player = Player{user: user.clone(), channel: self.request_sender.clone()};
        let event = GameEvent::Connected(player);
        self.event_handler.unbounded_send(event).expect("ChanService event_handler failed");
        // Process new user
        let id = user.get_id();
        self.login_id.insert(user.get_login().into(), id);
        let info = UserInfo{
            user,
            mute: MuteLevel::DenyAll("You are observer, you can not use chat.\n"),
        };
        self.users.insert(id, info);
    }

    fn handle_new_message(&self, id: UserId, line: Box<str>) {
        let info = match self.users.get(&id) {
            Some(info) => info,
            None => return,
        };
        match Message::parse(&line) {
            Message::Public(message) => self.handle_public_message(info, message),
            Message::Private(message, mut recipients) =>
                self.handle_private_message(info, message, &mut recipients),
            Message::Command(command) => self.handle_command(&info.user, command),
            Message::Action(login) => self.handle_action(&info.user, login),
        }
    }

    fn handle_public_message(&self, info: &UserInfo, message: &str) {
        let &UserInfo{ref user, ref mute, ..} = info;
        if !mute.public_allowed() {
            user.send_static(mute.get_reason());
            return;
        }
        if !message.is_empty() {
            self.broadcast(format!("{} [{}] {}\n",
                                   Local::now().format("%H:%M"),
                                   user.get_login(),
                                   message).into());
        }
    }

    fn handle_private_message(&self, info: &UserInfo, message: &str, recipients: &mut [&str]) {
        let &UserInfo{ref user, ref mute, ..} = info;
        // Do validation
        if !mute.private_allowed() {
            user.send_static(mute.get_reason());
            return;
        }
        if message.is_empty() {
            user.send_static("Can't send an empty private message.\n");
            return;
        }
        if recipients.is_empty() {  // Shouldn't happen, but just to be sure
            user.send_static("No recipients in your private message.\n");
            return;
        }
        // Check that all recipients exist
        let mut unknown_logins = vec![];
        for &login in recipients.iter() {
            if !self.login_id.contains_key(login) {
                unknown_logins.push(login);
            }
        }
        if !unknown_logins.is_empty() {
            user.send(format!("Unknown user(s): {}\n", unknown_logins.join(", ")));
            return;
        }
        // Build message
        let message: Arc<str> = format!("{} [{}]->[{}] {}\n",
                                        Local::now().format("%H:%M"),
                                        user.get_login(),
                                        recipients.join("]+["),
                                        message).into();
        // Delete duplicates and sender from recepients
        recipients.sort();
        let (recipients, _) = recipients.partition_dedup();
        // Send message
        for &login in recipients.iter() {
            if login != user.get_login() {
                let other_user = self.get_user_by_login(login).expect("ChatService user is missing");
                other_user.send_arc(message.clone());
            }
        }
        user.send_arc(message);
    }

    fn handle_command(&self, user: &User, command: &str) {
        let mut game_event = None;
        match command {
            "help" => user.send_static(HELP_EN),
            "quit" => user.drop(),
            "list" => game_event = Some(GameEvent::CommandList(user.get_id())),
            "observe" => game_event = Some(GameEvent::CommandObserve(user.get_id())),
            "play" => game_event = Some(GameEvent::CommandPlay(user.get_id())),
            "pause" => game_event = Some(GameEvent::CommandPause(user.get_id())),
            "start" => game_event = Some(GameEvent::CommandStart(user.get_id())),
            _ => user.send_static("Unknown command.\n"),
        }
        if let Some(event) = game_event {
            self.event_handler.unbounded_send(event).expect("ChatService event_hadler failed");
        }
    }

    fn handle_action(&self, user: &User, other: &str) {
        let event = GameEvent::Action(user.get_id(), other.into());
        self.event_handler.unbounded_send(event).expect("ChatService event_hadler failed");
    }

    fn handle_drop_user(&mut self, id: UserId) {
        if let Some(info) = self.users.remove(&id) {
            self.broadcast(format!("{} Disconnected: {}\n",
                                   Local::now().format("%H:%M"),
                                   info.user.get_login()).into());
            let event = GameEvent::Disconnected(info.user.get_id());
            self.event_handler.unbounded_send(event).expect("ChatService event_hadler failed");
        }
    }

    fn handle_mute_request(&mut self, id: UserId, level: MuteLevel) {
        if let Some(mut info) = self.users.get_mut(&id) {
            info.mute = level;
        }
    }

    fn get_user_by_login(&self, login: &str) -> Option<&User> {
        Some(&self.users.get(self.login_id.get(login)?)?.user)
    }

    fn broadcast(&self, message: Arc<str>) {
        for info in self.users.values() {
            info.user.send_arc(message.clone());
        }
    }
}

impl<'a> Message<'a> {
    pub fn parse(line: &'a str) -> Self {
        match line.chars().next() {
            Some('+') => Message::parse_private(line),
            Some('!') => Message::parse_command(line),
            _ => Message::Public(line),
        }
    }

    fn parse_private(line: &'a str) -> Self {
        let mut recipients = vec![];
        for word in line.split_whitespace() {
            if Message::first_char(word) == '+' {
                recipients.push(Message::remove_first_char(word));
            } else {
                let offset = (word.as_ptr() as usize) - (line.as_ptr() as usize);
                return Message::Private(&line[offset..], recipients.into());
            }
        }
        Message::Private("", recipients.into())
    }

    fn parse_command(line: &'a str) -> Self {
        let line = Message::remove_first_char(line);
        if let Some('!') = line.chars().next() {
            Message::Action(Message::remove_first_char(line))
        } else {
            Message::Command(line)
        }
    }

    fn remove_first_char(slice: &str) -> &str {
        slice.chars().next().map(|c| &slice[c.len_utf8()..]).expect("No first character")
    }

    fn first_char(slice: &str) -> char {
        slice.chars().next().expect("No first character")
    }
}

impl MuteLevel {
    pub fn public_allowed(&self) -> bool {
        if let MuteLevel::AllowAll = self {
            true
        } else {
            false
        }
    }

    pub fn private_allowed(&self) -> bool {
        if let MuteLevel::DenyAll(_) = self {
            false
        } else {
            true
        }
    }

    pub fn get_reason(&self) -> &'static str {
        match self {
            MuteLevel::AllowAll => "",
            MuteLevel::DenyPublic(reason) => reason,
            MuteLevel::DenyAll(reason) => reason,
        }
    }
}

impl Player {
    pub fn get_id(&self) -> PlayerId {
        self.user.get_id()
    }

    pub fn get_login(&self) -> &str {
        self.user.get_login()
    }

    pub fn send(&self, message: String) {
        self.user.send(message)
    }

    pub fn send_boxed(&self, message: Box<str>) {
        self.user.send_boxed(message)
    }

    pub fn send_arc(&self, message: Arc<str>) {
        self.user.send_arc(message)
    }

    pub fn send_static(&self, message: &'static str) {
        self.user.send_static(message)
    }

    pub fn disconnect(&self) {
        self.user.drop()
    }

    pub fn mute(&self, level: MuteLevel) {
        let request = ChatRequest::MutePlayer(self.get_id(), level);
        self.channel.unbounded_send(request).expect("Player channel failed");
    }
}
