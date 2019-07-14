use crate::chat_service::{GameEvent, Player, PlayerId, MuteLevel};
use crate::locale::Locale;
use crate::util::Timer;

use futures::{
    prelude::*,
    select,
    channel::mpsc::{UnboundedSender, UnboundedReceiver, unbounded}
};

use std::collections::HashMap;

pub struct GameService {
    event_sender: UnboundedSender<GameEvent>,
    event_receiver: UnboundedReceiver<GameEvent>,
    stage: GameStage,
    timer: Timer<u64>,
}

enum GameStage {
    Lobby(LobbyStage),
    Day(DayStage),
    Night(NightStage),
}

struct LobbyStage {
    locale: Locale,
    players: HashMap<PlayerId, PlayerInfo>,
    epoch: u64,
    can_start: bool,
}

struct DayStage;
struct NightStage;

struct PlayerInfo {
    player: Player,
    state: PlayerState,
}

enum PlayerState {
    Active,
    Observer,
}

impl GameService {
    pub fn new(locale: Locale) -> Self {
        let (event_sender, event_receiver) = unbounded();
        let stage = GameStage::Lobby(LobbyStage{
            locale: locale,
            players: HashMap::new(),
            epoch: 0,
            can_start: true
        });
        GameService {
            event_sender,
            event_receiver,
            stage,
            timer: Timer::new(),
        }
    }

    pub fn make_event_handler(&self) -> UnboundedSender<GameEvent> {
        self.event_sender.clone()
    }

    pub async fn run(mut self) {
        loop {
            select! {
                maybe_event = self.event_receiver.next().fuse() =>
                    match maybe_event {
                        Some(event) => self.stage = self.stage.handle_game_event(event, &mut self.timer),
                        None => panic!("GameService event_receiver terminated"),
                    },
                _ = self.timer.next().fuse() => {
                    self.stage = self.stage.handle_timer_event(&mut self.timer);
                },
            }
        }
    }
}

impl GameStage {
    fn handle_game_event(self, event: GameEvent, timer: &mut Timer<u64>) -> Self {
        self
    }

    fn handle_timer_event(self, timer: &mut Timer<u64>) -> Self {
        self
    }
}
