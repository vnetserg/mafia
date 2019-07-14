use crate::login_service::{User, UserId, UserEvent};
use crate::locale::{Locale, HELP_EN};

use futures::{
    prelude::*,
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
    id: PlayerId,
    user: User,
}

pub struct ChatService {
    event_handler: UnboundedSender<GameEvent>,
    user_sender: UnboundedSender<UserEvent>,
    user_receiver: UnboundedReceiver<UserEvent>,
    users: HashMap<UserId, User>,
    login_id: HashMap<String, UserId>,
    locale: Locale,
}

pub enum GameEvent {
    NewPlayer(Player),
    DropPlayer(PlayerId),
    Action(PlayerId, Box<str>),
    CommandObserve(PlayerId),
    CommandPlay(PlayerId),
    CommandPause(PlayerId),
    CommandStart(PlayerId),
}

enum Message<'a> {
    Public(&'a str),
    Private(&'a str, Vec<&'a str>),
    Command(&'a str),
    Action(&'a str),
}

impl ChatService {
    pub fn new(event_handler: UnboundedSender<GameEvent>, locale: Locale) -> Self {
        let (user_sender, user_receiver) = unbounded();
        ChatService {
            event_handler,
            user_sender,
            user_receiver,
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
            match self.user_receiver.next().await {
                Some(UserEvent::NewUser(user)) => self.handle_new_user(user),
                Some(UserEvent::NewMessage(id, data)) => self.handle_new_message(id, data),
                Some(UserEvent::DropUser(id)) => self.handle_drop_user(id),
                None => panic!("ChatService user_receiver terminated"),
            }
        }
    }

    fn handle_new_user(&mut self, user: User) {
        let id = user.get_id();
        self.broadcast(format!("{} Connected: {}\n",
                               Local::now().format("%H:%M"),
                               user.get_login()).into());
        self.login_id.insert(user.get_login().to_owned(), id);
        self.users.insert(id, user);
    }

    fn handle_new_message(&self, id: UserId, line: String) {
        let user = match self.users.get(&id) {
            Some(user) => user,
            None => return,
        };
        match Message::parse(&line) {
            Message::Public(message) => self.handle_public_message(user, message),
            Message::Private(message, mut recipients) =>
                self.handle_private_message(user, message, &mut recipients),
            Message::Command(command) => self.handle_command(user, command),
            Message::Action(login) => self.handle_action(user, login),
        }
    }

    fn handle_public_message(&self, user: &User, message: &str) {
        if !message.is_empty() {
            self.broadcast(format!("{} [{}] {}\n",
                                   Local::now().format("%H:%M"),
                                   user.get_login(),
                                   message).into());
        }
    }

    fn handle_private_message(&self, user: &User, message: &str, recipients: &mut [&str]) {
        // Do validation
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
            "list" => {
                let users: Vec<&str> = self.login_id.keys().map(|s| s.as_str()).collect();
                user.send(format!("Players online:\n * {}\n", users.join("\n * ")));
            },
            "observe" => game_event = Some(GameEvent::CommandObserve(user.get_id())),
            "play" => game_event = Some(GameEvent::CommandPlay(user.get_id())),
            "pause" => game_event = Some(GameEvent::CommandPause(user.get_id())),
            "start" => game_event = Some(GameEvent::CommandStart(user.get_id())),
            _ => user.send_static("Unknown command.\n"),
        }
        if let Some(event) = game_event {
            // TODO
        }
    }

    fn handle_action(&self, user: &User, other: &str) {
        // TODO
    }

    fn handle_drop_user(&mut self, id: UserId) {
        if let Some(user) = self.users.remove(&id) {
            self.broadcast(format!("{} Disconnected: {}\n",
                                   Local::now().format("%H:%M"),
                                   user.get_login()).into());
        }
    }

    fn get_user_by_login(&self, login: &str) -> Option<&User> {
        self.users.get(self.login_id.get(login)?)
    }

    fn broadcast(&self, message: Arc<str>) {
        for user in self.users.values() {
            user.send_arc(message.clone());
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
                return Message::Private(&line[offset..], recipients);
            }
        }
        Message::Private("", recipients)
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
