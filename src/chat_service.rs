use crate::login_service::{User, UserId, UserEvent};
use crate::locale::Locale;

use futures::{
    prelude::*,
    channel::mpsc::{UnboundedSender, UnboundedReceiver, unbounded}
};

use chrono::prelude::*;

use std::collections::HashMap;

pub struct ChatService {
    user_sender: UnboundedSender<UserEvent>,
    user_receiver: UnboundedReceiver<UserEvent>,
    users: HashMap<UserId, User>,
    login_id: HashMap<String, UserId>,
    locale: Locale,
}

enum Message<'a> {
    Public(&'a str),
    Private(&'a str, Vec<&'a str>),
    Command(&'a str, Vec<&'a str>),
}

impl ChatService {
    pub fn new(locale: Locale) -> Self {
        let (user_sender, user_receiver) = unbounded();
        ChatService {
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
        self.broadcast(&format!("{} Connected: {}\n",
                       Local::now().format("%H:%M"),
                       user.get_login()));
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
            Message::Command(command, args) => self.handle_command(user, command, &args),
        }
    }

    fn handle_public_message(&self, user: &User, message: &str) {
        if !message.is_empty() {
            let message = format!("{} [{}] {}\n",
                                  Local::now().format("%H:%M"),
                                  user.get_login(),
                                  message);
            self.broadcast(&message);
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
        let message = format!("{} [{}]->[{}] {}\n",
                              Local::now().format("%H:%M"),
                              user.get_login(),
                              recipients.join("]+["),
                              message);
        // Send message
        recipients.sort();
        let (recipients, _) = recipients.partition_dedup();
        for &login in recipients.iter() {
            if login != user.get_login() {
                let other_user = self.get_user_by_login(login).expect("ChatService user is missing");
                other_user.send(message.clone());
            }
        }
        user.send(message);
    }

    fn handle_command(&self, user: &User, command: &str, args: &[&str]) {

    }

    fn handle_drop_user(&mut self, id: UserId) {
        if let Some(user) = self.users.remove(&id) {
            self.broadcast(&format!("{} Disconnected: {}\n",
                                    Local::now().format("%H:%M"),
                                    user.get_login()));
        }
    }

    fn get_user_by_login(&self, login: &str) -> Option<&User> {
        self.users.get(self.login_id.get(login)?)
    }

    fn broadcast(&self, message: &str) {
        for user in self.users.values() {
            user.send(message.to_owned());
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
        Message::Command(line, vec![])
    }

    fn remove_first_char(slice: &str) -> &str {
        slice.chars().next().map(|c| &slice[c.len_utf8()..]).expect("No first character")
    }

    fn first_char(slice: &str) -> char {
        slice.chars().next().expect("No first character")
    }
}
