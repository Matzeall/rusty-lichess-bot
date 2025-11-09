mod random_engine;

use anyhow::Result;
use async_trait::async_trait;
use random_engine::RandomEngine;
use shakmaty::{Chess, Color, Move, uci::UciMove};

pub fn init_engine(initial_position: Chess, bot_color: Color) -> Box<dyn Engine> {
    let engine = RandomEngine::new(initial_position, bot_color);
    Box::new(engine)
}

#[async_trait]
pub trait Engine: Send + Sync {
    async fn update_board(&mut self, move_played: UciMove) -> Result<()>;

    async fn search(&mut self) -> Option<Move>;

    fn get_game_state(&self) -> &Chess;

    fn is_my_turn(&self) -> bool;
}
